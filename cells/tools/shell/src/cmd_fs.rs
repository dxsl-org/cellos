//! Filesystem-oriented shell built-ins: wc, head, tail, mkdir, rm, rmdir, touch.
//!
//! VFS-write operations (mkdir, rm, rmdir) send IPC to the VFS service cell
//! resolved via `sys_lookup_service`.  Read operations use the kernel's `sys_open`/`sys_read` path.

use ostd::prelude::*;
use ostd::syscall;

/// Resolve the live VFS service tid via the service registry.
/// Spins (yield-looping) until init has registered vfs — safe at startup
/// because init spawns vfs before shell and vfs registers before yielding.
fn vfs_endpoint() -> usize {
    use api::syscall::service;
    loop {
        if let Some(tid) = syscall::sys_lookup_service(service::VFS) {
            return tid;
        }
        ostd::task::yield_now();
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Return VFS metadata for a path without reading its contents.
pub(crate) fn stat_file_vfs(path: &str) -> Option<(usize, bool)> {
    let mut vfs = ostd::clients::VfsClient::new();
    match vfs.stat(path) {
        Ok((size, is_dir)) => usize::try_from(size).ok().map(|size| (size, is_dir)),
        Err(_) => None,
    }
}

/// Read the entire contents of `path` into a Vec<u8>.
fn read_file_bytes(path: &str) -> ViResult<Vec<u8>> {
    let fd = syscall::sys_open(path).map_err(|_| ViError::NotFound)?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        match syscall::sys_read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    syscall::sys_close(fd);
    Ok(bytes)
}

/// Collect all newline-terminated lines from `data` into a Vec of str slices.
fn collect_lines(data: &[u8]) -> Vec<&str> {
    let text = core::str::from_utf8(data).unwrap_or("");
    text.lines().collect()
}

/// Send a typed VfsRequest to the VFS cell and return whether it succeeded.
fn vfs_req_ok(req: &api::ipc::VfsRequest<'_>) -> bool {
    // Spec 17 §9 compliant exemplar: service_call_typed recvs MASKED to the VFS
    // tid (a wildcard recv here once decoded a queued keystroke as the reply —
    // "vwrite failed" while the write succeeded — and desynced every later VFS
    // exchange) and surfaces failure as a typed IpcError rather than emptiness.
    let mut send_buf = [0u8; 512];
    let mut reply = [0u8; 64];
    matches!(
        ostd::ipc::service_call_typed::<_, api::ipc::VfsResponse>(
            vfs_endpoint(),
            req,
            &mut send_buf,
            &mut reply,
        ),
        Ok(api::ipc::VfsResponse::Ok)
    )
}

// ─── wc ───────────────────────────────────────────────────────────────────────

/// `wc [file]` — print line, word, and byte counts.
///
/// When called without a file (in a pipeline), reads from `shell_stdin()`.
pub fn cmd_wc(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = args.next().unwrap_or("");
    let owned;
    let data: &[u8] = if path.is_empty() {
        owned = crate::executor::shell_stdin();
        if owned.is_empty() {
            crate::executor::shell_println("Usage: wc [file]");
            return Ok(());
        }
        &owned
    } else {
        owned = read_file_bytes(path).map_err(|_| {
            ostd::io::print("wc: cannot open '");
            ostd::io::print(path);
            ostd::io::println("'");
            ViError::NotFound
        })?;
        &owned
    };
    let bytes = data.len();
    let lines = data.iter().filter(|&&b| b == b'\n').count();
    let words = data
        .split(|b| b == &b' ' || b == &b'\n' || b == &b'\t')
        .filter(|w| !w.is_empty())
        .count();
    let label = if path.is_empty() { "" } else { path };
    crate::executor::shell_print(&alloc::format!("{} {} {} {}\n", lines, words, bytes, label));
    Ok(())
}

// ─── head ─────────────────────────────────────────────────────────────────────

/// `head [-n N] <file>` — print first N lines (default 10).
pub fn cmd_head(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut n: usize = 10;
    let mut path = "";
    // Simple arg parsing: if first arg is "-n", consume the count.
    loop {
        match args.next() {
            Some("-n") => {
                if let Some(num) = args.next() {
                    n = num.parse().unwrap_or(10);
                }
            }
            Some(p) => {
                path = p;
                break;
            }
            None => break,
        }
    }
    if path.is_empty() {
        crate::executor::shell_println("Usage: head [-n N] <file>");
        return Ok(());
    }
    let data = read_file_bytes(path).map_err(|_| {
        ostd::io::print("head: cannot open '");
        ostd::io::print(path);
        ostd::io::println("'");
        ViError::NotFound
    })?;
    for line in collect_lines(&data).into_iter().take(n) {
        crate::executor::shell_println(line);
    }
    Ok(())
}

// ─── tail ─────────────────────────────────────────────────────────────────────

/// `tail [-n N] <file>` — print last N lines (default 10).
pub fn cmd_tail(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut n: usize = 10;
    let mut path = "";
    loop {
        match args.next() {
            Some("-n") => {
                if let Some(num) = args.next() {
                    n = num.parse().unwrap_or(10);
                }
            }
            Some(p) => {
                path = p;
                break;
            }
            None => break,
        }
    }
    if path.is_empty() {
        crate::executor::shell_println("Usage: tail [-n N] <file>");
        return Ok(());
    }
    let data = read_file_bytes(path).map_err(|_| {
        ostd::io::print("tail: cannot open '");
        ostd::io::print(path);
        ostd::io::println("'");
        ViError::NotFound
    })?;
    let lines = collect_lines(&data);
    let start = lines.len().saturating_sub(n);
    for line in &lines[start..] {
        crate::executor::shell_println(line);
    }
    Ok(())
}

// ─── Path Resolution ──────────────────────────────────────────────────────────

/// Resolve a user-supplied path against the current shell CWD.
pub(crate) fn resolve_shell_path(path: &str) -> String {
    if path.is_empty() {
        return crate::cmd_cwd::get_shell_cwd().unwrap_or_else(|_| String::from("/"));
    }
    let mut out = String::new();
    out.push('/');
    if !path.starts_with('/') {
        if let Ok(cwd) = crate::cmd_cwd::get_shell_cwd() {
            for comp in cwd.split('/') {
                match comp {
                    "" | "." => {}
                    ".." => {
                        if out.len() > 1 {
                            let slash = out.rfind('/').unwrap_or(0);
                            out.truncate(slash.max(1));
                        }
                    }
                    c => {
                        if out.len() > 1 {
                            out.push('/');
                        }
                        out.push_str(c);
                    }
                }
            }
        }
    }
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                if out.len() > 1 {
                    let slash = out.rfind('/').unwrap_or(0);
                    out.truncate(slash.max(1));
                }
            }
            c => {
                if out.len() > 1 {
                    out.push('/');
                }
                out.push_str(c);
            }
        }
    }
    out
}

// ─── mkdir ────────────────────────────────────────────────────────────────────

/// `mkdir [-p] <path>...` — create a new directory via VFS IPC.
pub fn cmd_mkdir(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut parents = false;
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        if arg.starts_with('-') {
            parents |= arg.contains('p');
        } else {
            paths.push(arg);
        }
    }
    if paths.is_empty() {
        ostd::io::println("Usage: mkdir [-p] <path>...");
        return Ok(());
    }
    for path in paths {
        let resolved = resolve_shell_path(path);
        if parents {
            let mut prefix = String::new();
            for part in resolved.split('/').filter(|p| !p.is_empty()) {
                prefix.push('/');
                prefix.push_str(part);
                if matches!(stat_file_vfs(&prefix), Some((_, true))) {
                    continue;
                }
                let _ = vfs_req_ok(&api::ipc::VfsRequest::Mkdir(&prefix));
            }
        } else if !vfs_req_ok(&api::ipc::VfsRequest::Mkdir(&resolved)) {
            ostd::io::print("mkdir: cannot create directory '");
            ostd::io::print(path);
            ostd::io::println("'");
        }
    }
    Ok(())
}

// ─── rmdir ────────────────────────────────────────────────────────────────────

/// `rmdir <path>...` — remove an empty directory via VFS IPC.
pub fn cmd_rmdir(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut any = false;
    while let Some(path) = args.next() {
        if path.starts_with('-') {
            continue;
        }
        any = true;
        let resolved = resolve_shell_path(path);
        if !vfs_req_ok(&api::ipc::VfsRequest::Rmdir(&resolved)) {
            ostd::io::print("rmdir: failed to remove '");
            ostd::io::print(path);
            ostd::io::println("' (not empty or not found)");
        }
    }
    if !any {
        ostd::io::println("Usage: rmdir <path>...");
    }
    Ok(())
}

// ─── rm ───────────────────────────────────────────────────────────────────────

/// `rm [-r] [-f] <path>...` — remove a file, or (with -r on /data) a directory tree.
pub fn cmd_rm(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut recursive = false;
    let mut force = false;
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        if arg.starts_with('-') {
            recursive |= arg.contains('r') || arg.contains('R');
            force |= arg.contains('f');
        } else {
            paths.push(arg);
        }
    }
    if paths.is_empty() {
        if !force {
            ostd::io::println("Usage: rm [-r] [-f] <path>...");
        }
        return Ok(());
    }
    for path in paths {
        let resolved = resolve_shell_path(path);
        let ok = if recursive && resolved.starts_with("/data/") {
            rm_recursive(&resolved)
        } else {
            vfs_req_ok(&api::ipc::VfsRequest::Unlink(&resolved))
        };
        if !ok && !force {
            ostd::io::print("rm: cannot remove '");
            ostd::io::print(path);
            ostd::io::println("'");
        }
    }
    Ok(())
}

// ─── touch ────────────────────────────────────────────────────────────────────

/// `touch <path>...` — create an empty file or update timestamp via VFS IPC.
pub fn cmd_touch(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut any = false;
    while let Some(path) = args.next() {
        if path.starts_with('-') {
            continue;
        }
        any = true;
        let resolved = resolve_shell_path(path);
        if stat_file_vfs(&resolved).is_none() {
            if !write_file(&resolved, &[]) {
                ostd::io::print("touch: cannot touch '");
                ostd::io::print(path);
                ostd::io::println("'");
            }
        }
    }
    if !any {
        ostd::io::println("Usage: touch <path>...");
    }
    Ok(())
}

// ─── mv ───────────────────────────────────────────────────────────────────────

/// `mv <source> <target>` — rename/move a file or directory via VFS IPC (atomic rename).
pub fn cmd_mv(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let old = match args.next() {
        Some(o) => o,
        None => {
            ostd::io::println("Usage: mv <source> <target>");
            return Ok(());
        }
    };
    let new = match args.next() {
        Some(n) => n,
        None => {
            ostd::io::println("Usage: mv <source> <target>");
            return Ok(());
        }
    };
    let old_resolved = resolve_shell_path(old);
    let new_resolved = resolve_shell_path(new);
    if !vfs_req_ok(&api::ipc::VfsRequest::Rename {
        old: &old_resolved,
        new: &new_resolved,
    }) {
        ostd::io::print("mv: cannot move '");
        ostd::io::print(old);
        ostd::io::print("' to '");
        ostd::io::print(new);
        ostd::io::println("'");
    }
    Ok(())
}

// ─── cp ───────────────────────────────────────────────────────────────────────

/// `cp <source> <target>` — copy a file via VFS IPC.
pub fn cmd_cp(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let src = match args.next() {
        Some(s) => s,
        None => {
            ostd::io::println("Usage: cp <source> <target>");
            return Ok(());
        }
    };
    let dst = match args.next() {
        Some(d) => d,
        None => {
            ostd::io::println("Usage: cp <source> <target>");
            return Ok(());
        }
    };
    let src_resolved = resolve_shell_path(src);
    let dst_resolved = resolve_shell_path(dst);
    let data = match read_file_vfs_owned(&src_resolved, 1024 * 1024) {
        Ok(d) => d,
        Err(_) => match read_file_bytes(&src_resolved) {
            Ok(d) => d,
            Err(_) => {
                ostd::io::print("cp: cannot open '");
                ostd::io::print(src);
                ostd::io::println("'");
                return Ok(());
            }
        },
    };
    if !vfs_write_chunked(&dst_resolved, &data, false) {
        ostd::io::print("cp: cannot copy to '");
        ostd::io::print(dst);
        ostd::io::println("'");
    }
    Ok(())
}

/// Recursively delete a `/data/` directory tree via VFS IPC.
pub fn rm_recursive(path: &str) -> bool {
    vfs_req_ok(&api::ipc::VfsRequest::RmdirRecursive(path))
}

/// Write `content` to `path` via typed VFS IPC.
/// The VFS server enforces `/data/`/`/tmp/` path authorization.
pub fn write_file(path: &str, content: &[u8]) -> bool {
    vfs_req_ok(&api::ipc::VfsRequest::Write { path, content })
}

/// Append `content` to `path` via typed VFS IPC.
/// Caller must chunk if content exceeds the 512-byte IPC buffer capacity.
pub fn append_file(path: &str, content: &[u8]) -> bool {
    vfs_req_ok(&api::ipc::VfsRequest::Append { path, content })
}

/// Write `data` to `path`, splitting into ≤400-byte chunks to stay within the
/// 512-byte IPC frame limit.  First chunk uses `Write` (create/overwrite);
/// subsequent chunks use `Append` to extend.  When `append` is true, every
/// chunk uses `Append`.
pub fn vfs_write_chunked(path: &str, data: &[u8], append: bool) -> bool {
    const CHUNK: usize = 400;
    if data.is_empty() {
        return if append { true } else { write_file(path, &[]) };
    }
    let mut first = !append;
    let mut ok = true;
    for chunk in data.chunks(CHUNK) {
        ok &= if first {
            first = false;
            write_file(path, chunk)
        } else {
            append_file(path, chunk)
        };
    }
    ok
}

/// `vwrite <path> <text>` — write text to a VFS path via OP_WRITE (test helper).
pub fn cmd_vwrite(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = match args.next() {
        Some(p) => p,
        None => {
            ostd::io::println("Usage: vwrite <path> <text>");
            return Ok(());
        }
    };
    let rest = args.collect::<alloc::vec::Vec<_>>().join(" ");
    if !write_file(path, rest.as_bytes()) {
        ostd::io::print("vwrite: failed to write '");
        ostd::io::print(path);
        ostd::io::println("'");
    }
    Ok(())
}

/// `vappend <path> <text>` — append text to a VFS path via OP_APPEND (test helper).
pub fn cmd_vappend(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = match args.next() {
        Some(p) => p,
        None => {
            ostd::io::println("Usage: vappend <path> <text>");
            return Ok(());
        }
    };
    let rest = args.collect::<alloc::vec::Vec<_>>().join(" ");
    if !append_file(path, rest.as_bytes()) {
        ostd::io::print("vappend: failed to append '");
        ostd::io::print(path);
        ostd::io::println("'");
    }
    Ok(())
}

fn vfs_read_size(path: &str) -> ViResult<usize> {
    let mut vfs = ostd::clients::VfsClient::new();
    let (size, is_dir) = vfs.stat(path)?;
    if is_dir {
        return Err(ViError::IsADirectory);
    }
    usize::try_from(size).map_err(|_| ViError::InvalidInput)
}

fn read_file_vfs_exact(path: &str, file_len: usize, out: &mut [u8]) -> ViResult<usize> {
    if file_len == 0 {
        return Ok(0);
    }
    if file_len > out.len() {
        return Err(ViError::InvalidArgument);
    }

    let mut vfs = ostd::clients::VfsClient::new();
    let bytes = vfs.read_file_bounded(path, file_len)?;
    if bytes.len() != file_len {
        return Err(ViError::IO);
    }
    out[..file_len].copy_from_slice(&bytes);
    Ok(file_len)
}

/// Read a complete VFS file through caller-owned memory.
///
/// The observed Stat size is a hard bound: short copies, malformed replies,
/// and a destination that cannot hold that snapshot fail rather than truncate.
pub(crate) fn read_file_vfs_result(path: &str, out: &mut [u8]) -> ViResult<usize> {
    let file_len = vfs_read_size(path)?;
    read_file_vfs_exact(path, file_len, out)
}

/// Perform a complete read when the caller already obtained an authorized size.
pub(crate) fn read_file_vfs_known_size(path: &str, size: usize, out: &mut [u8]) -> ViResult<usize> {
    read_file_vfs_exact(path, size, out)
}

/// Allocate a bounded destination and perform a complete read.
pub(crate) fn read_file_vfs_owned(path: &str, max: usize) -> ViResult<Vec<u8>> {
    let file_len = vfs_read_size(path)?;
    if file_len > max {
        return Err(ViError::InvalidArgument);
    }
    let mut vfs = ostd::clients::VfsClient::new();
    let bytes = vfs.read_file_bounded(path, file_len)?;
    if bytes.len() != file_len {
        return Err(ViError::IO);
    }
    Ok(bytes)
}

/// `vcat <path>` — print file content via VFS OP_READ (reads RamFS including /tmp/).
///
/// Unlike `cat`, which reads the kernel-embedded FS, `vcat` reads from the
/// VFS cell's RamFS — the same store that OP_WRITE targets.
pub fn cmd_vcat(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = match args.next() {
        Some(p) => p,
        None => {
            ostd::io::println("Usage: vcat <path>");
            return Ok(());
        }
    };
    let mut buf = [0u8; 4096];
    let n = read_file_vfs_result(path, &mut buf).inspect_err(|_| {
        ostd::io::print("vcat: cannot read: ");
        ostd::io::println(path);
    })?;
    if let Ok(s) = core::str::from_utf8(&buf[..n]) {
        crate::executor::shell_print(s);
    }
    Ok(())
}

// ─── find ─────────────────────────────────────────────────────────────────────

/// `find <dir> [-name pattern]` — recursively list files under `dir`.
///
/// Uses VFS `ListDir` IPC.  Directories with more than ~30 entries are silently
/// truncated by the 512-byte `ListDir` reply limit — a known v1.0 limitation.
pub fn cmd_find(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let dir = args.next().unwrap_or(".");
    let pattern = if args.next() == Some("-name") {
        args.next()
    } else {
        None
    };
    // Resolve once; pass TID through recursion to avoid a syscall per directory level.
    let vfs_tid = vfs_endpoint();
    find_recursive(dir, pattern, 0, vfs_tid);
    Ok(())
}

/// Maximum directory recursion depth for `find`.  Prevents stack overflow on
/// pathological trees; each level holds ~1 KB of stack for IPC buffers.
const FIND_MAX_DEPTH: usize = 16;

fn find_recursive(dir: &str, pattern: Option<&str>, depth: usize, vfs_tid: usize) {
    if depth >= FIND_MAX_DEPTH {
        return;
    }
    use api::ipc::{VfsRequest, VfsResponse};
    let mut send = [0u8; 512];
    let n = match api::ipc::encode(&VfsRequest::ListDir(dir), &mut send) {
        Ok(s) => s.len(),
        Err(_) => return,
    };
    ostd::syscall::sys_send(vfs_tid, &send[..n]);
    let mut reply = [0u8; 512];
    // Masked recv — see vfs_req_ok.
    let raw = match ostd::syscall::sys_recv(vfs_tid, &mut reply) {
        ostd::syscall::SyscallResult::Ok(_) => &reply,
        _ => return,
    };
    if let Ok(VfsResponse::Data(entries)) = api::ipc::decode::<VfsResponse>(raw) {
        let text = core::str::from_utf8(entries).unwrap_or("");
        for entry in text.lines() {
            let (kind, name) = if let Some(rest) = entry.strip_prefix("d:") {
                ("d", rest)
            } else if let Some(rest) = entry.strip_prefix("f:") {
                ("f", rest)
            } else {
                continue;
            };
            // Build the full path without heap format for depth-zero dirs.
            let mut full = alloc::string::String::from(dir);
            if !full.ends_with('/') {
                full.push('/');
            }
            full.push_str(name);

            if kind == "f" {
                let matches = pattern.map(|p| name.contains(p)).unwrap_or(true);
                if matches {
                    crate::executor::shell_println(&full);
                }
            } else {
                find_recursive(&full, pattern, depth + 1, vfs_tid);
            }
        }
    }
}

/// List entries in a directory from the Userspace VFS Service via IPC.
pub(crate) fn vfs_list_dir(dir: &str) -> Option<alloc::vec::Vec<alloc::string::String>> {
    use api::ipc::{VfsRequest, VfsResponse};
    let vfs_tid = vfs_endpoint();
    let mut send = [0u8; 512];
    let n = api::ipc::encode(&VfsRequest::ListDir(dir), &mut send).ok()?.len();
    ostd::syscall::sys_send(vfs_tid, &send[..n]);
    let mut reply = [0u8; 512];
    if let ostd::syscall::SyscallResult::Ok(_) = ostd::syscall::sys_recv(vfs_tid, &mut reply) {
        if let Ok(VfsResponse::Data(entries)) = api::ipc::decode::<VfsResponse>(&reply) {
            let text = core::str::from_utf8(entries).ok()?;
            let mut list = alloc::vec::Vec::new();
            for entry in text.lines() {
                if let Some(name) = entry.strip_prefix("d:").or_else(|| entry.strip_prefix("f:")) {
                    list.push(alloc::string::String::from(name));
                }
            }
            return Some(list);
        }
    }
    None
}
// ─── uniq ─────────────────────────────────────────────────────────────────────

/// `uniq [file]` — filter adjacent duplicate lines.
///
/// When called without a file (in a pipeline), reads from `shell_stdin()`.
pub fn cmd_uniq(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = args.next().unwrap_or("");
    let owned;
    let data: &[u8] = if path.is_empty() {
        owned = crate::executor::shell_stdin();
        if owned.is_empty() {
            crate::executor::shell_println("Usage: uniq [file]");
            return Ok(());
        }
        &owned
    } else {
        owned = read_file_bytes(path).map_err(|_| {
            ostd::io::print("uniq: cannot open '");
            ostd::io::print(path);
            ostd::io::println("'");
            ViError::NotFound
        })?;
        &owned
    };
    let text = core::str::from_utf8(data).unwrap_or("");
    let mut prev = "";
    for line in text.lines() {
        if line != prev {
            crate::executor::shell_println(line);
            prev = line;
        }
    }
    Ok(())
}

// ─── sort (stdin-aware) ───────────────────────────────────────────────────────

/// `sort [file]` — sort lines lexicographically.
///
/// When called without a file (in a pipeline), reads from `shell_stdin()`.
pub fn cmd_sort(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = args.next().unwrap_or("");
    let owned;
    let data: &[u8] = if path.is_empty() {
        owned = crate::executor::shell_stdin();
        if owned.is_empty() {
            crate::executor::shell_println("Usage: sort [file]");
            return Ok(());
        }
        &owned
    } else {
        owned = read_file_bytes(path).map_err(|_| {
            ostd::io::print("sort: cannot open '");
            ostd::io::print(path);
            ostd::io::println("'");
            ViError::NotFound
        })?;
        &owned
    };
    let mut lines = collect_lines(data);
    lines.sort_unstable();
    for line in lines {
        crate::executor::shell_println(line);
    }
    Ok(())
}

// ─── tee ─────────────────────────────────────────────────────────────────────

/// `tee [-a] <path>` — read stdin, write to both stdout sink and a VFS file.
///
/// `-a` appends to the file instead of overwriting. Data flows through the
/// shell pipeline (via `shell_print`) AND is written to `path` via VFS IPC.
pub fn cmd_tee(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut append = false;
    let path = loop {
        match args.next() {
            Some("-a") => append = true,
            Some(p) => break p,
            None => {
                crate::executor::shell_println("Usage: tee [-a] <path>");
                return Ok(());
            }
        }
    };
    let data = crate::executor::shell_stdin();
    if data.is_empty() {
        // No stdin and no pipeline data: nothing to tee.
        return Ok(());
    }
    // Write to the current output sink (console or outer pipeline capture).
    if let Ok(s) = core::str::from_utf8(&data) {
        crate::executor::shell_print(s);
    }
    // Also write the same data to the VFS file.
    if !vfs_write_chunked(path, &data, append) {
        ostd::io::print("tee: cannot write '");
        ostd::io::print(path);
        ostd::io::println("'");
    }
    Ok(())
}

// ─── awk ─────────────────────────────────────────────────────────────────────

/// `awk [-F sep] [/pattern/] [col,...] [file]` — field extractor and line filter.
///
/// Because the shell tokenizer treats `{` and `}` as syntax operators, the
/// standard `awk '{print $1}'` form cannot be passed intact.  This implementation
/// uses a shell-friendly syntax instead:
///
/// - `-F sep`      — single-character field separator (default: whitespace).
/// - `/pattern/`   — print only lines containing the literal pattern.
/// - `col,...`     — comma-separated 1-based column indices to print (0 = full line;
///   omit to print the entire matching line).
/// - `file`        — path to read; reads `shell_stdin()` when absent.
///
/// Examples:
///   `awk -F: 1`           — first `:` -delimited field on each line
///   `awk /error/ 1 3`     — fields 1 and 3 from lines containing "error"
///   `awk 0`               — passthrough (entire lines)
///   `ps | awk /Running/ 2` — pipe-friendly
pub fn cmd_awk<'a>(mut args: core::str::SplitWhitespace<'a>) -> ViResult<()> {
    let mut sep: Option<char> = None;
    let mut pattern = "";
    let mut cols = [0usize; 8];
    let mut ncols: usize = 0;
    let mut path = "";

    loop {
        match args.next() {
            Some("-F") => match args.next() {
                Some(s) => sep = s.chars().next(),
                None => {
                    crate::executor::shell_println("awk: -F requires a separator character");
                    return Ok(());
                }
            },
            Some(a) if a.starts_with("-F") && a.len() > 2 => {
                sep = a[2..].chars().next();
            }
            // /pattern/ — starts and ends with '/' with no inner '/'
            Some(a)
                if a.len() >= 3
                    && a.starts_with('/')
                    && a.ends_with('/')
                    && !a[1..a.len() - 1].contains('/') =>
            {
                pattern = &a[1..a.len() - 1];
            }
            // col,col,... — non-empty, all digits or commas
            Some(a)
                if !a.is_empty()
                    && a.bytes().all(|b| b.is_ascii_digit() || b == b',')
                    && !a.starts_with(',')
                    && !a.ends_with(',') =>
            {
                for part in a.split(',') {
                    if let Ok(n) = part.parse::<usize>() {
                        if ncols < 8 {
                            cols[ncols] = n;
                            ncols += 1;
                        }
                    }
                }
            }
            Some(a) => {
                path = a;
                break;
            }
            None => break,
        }
    }

    let owned;
    let data: &[u8] = if path.is_empty() {
        owned = crate::executor::shell_stdin();
        if owned.is_empty() {
            crate::executor::shell_println("Usage: awk [-F sep] [/pattern/] [col,...] [file]");
            return Ok(());
        }
        &owned
    } else {
        owned = read_file_bytes(path).map_err(|_| {
            ostd::io::print("awk: cannot open '");
            ostd::io::print(path);
            ostd::io::println("'");
            ViError::NotFound
        })?;
        &owned
    };

    let text = core::str::from_utf8(data).unwrap_or("");

    for line in text.lines() {
        if !pattern.is_empty() && !line.contains(pattern) {
            continue;
        }

        if ncols == 0 {
            crate::executor::shell_println(line);
        } else {
            let fields: alloc::vec::Vec<&str> = if let Some(s) = sep {
                line.split(s).collect()
            } else {
                line.split_whitespace().collect()
            };
            let mut first_col = true;
            for &col in cols.iter().take(ncols) {
                let val: &str = if col == 0 {
                    line
                } else {
                    fields.get(col - 1).copied().unwrap_or("")
                };
                if !first_col {
                    crate::executor::shell_print(" ");
                }
                crate::executor::shell_print(val);
                first_col = false;
            }
            crate::executor::shell_print("\n");
        }
    }
    Ok(())
}
