//! Filesystem-oriented shell built-ins: wc, head, tail, mkdir, rm, rmdir, touch.
//!
//! VFS-write operations (mkdir, rm, rmdir) send IPC to the VFS service cell
//! resolved via `sys_lookup_service`.  Read operations use the kernel's `sys_open`/`sys_read` path,
//! gated by [`bootfs_fallback_allowed`] so a VFS refusal is never bypassed.

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

/// Whether the kernel BootFS (VIFS1) route may serve `path`. That route skips the VFS
/// caller/path policy, so it is used only when the VFS itself permits the read or answers
/// "no such entry" (`IO`); a VFS refusal, or no VFS answer at all, never falls through.
pub(crate) fn bootfs_fallback_allowed(path: &str) -> bool {
    let resolved = resolve_shell_path(path);
    let mut vfs = ostd::clients::VfsClient::new();
    matches!(vfs.stat(&resolved), Ok(_) | Err(ViError::IO))
}

/// Read the entire contents of `path` into a Vec<u8>.
fn read_file_bytes(path: &str) -> ViResult<Vec<u8>> {
    if !bootfs_fallback_allowed(path) {
        return Err(ViError::PermissionDenied);
    }
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
            crate::executor::shell_stderr("wc: cannot open '");
            crate::executor::shell_stderr(path);
            crate::executor::shell_stderrln("'");
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
        crate::executor::shell_stderr("head: cannot open '");
        crate::executor::shell_stderr(path);
        crate::executor::shell_stderrln("'");
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
        crate::executor::shell_stderr("tail: cannot open '");
        crate::executor::shell_stderr(path);
        crate::executor::shell_stderrln("'");
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

/// Resolve a user-supplied path against the current shell CWD. An unreadable CWD
/// resolves a relative path against `/` (and an empty path is `/`); callers that
/// must not guess a directory (`rg` search roots) use [`resolve_path_from`] with a
/// CWD they read themselves.
pub(crate) fn resolve_shell_path(path: &str) -> String {
    if path.is_empty() {
        return crate::cmd_cwd::get_shell_cwd().unwrap_or_else(|_| String::from("/"));
    }
    let cwd = if path.starts_with('/') {
        String::new()
    } else {
        crate::cmd_cwd::get_shell_cwd().unwrap_or_default()
    };
    resolve_path_from(&cwd, path)
}

/// The canonical absolute form of `path` resolved against `cwd`: empty and `.`
/// components dropped, `..` folded (never above `/`), no trailing `/` except for
/// `/` itself. An absolute `path` ignores `cwd`.
pub(crate) fn resolve_path_from(cwd: &str, path: &str) -> String {
    let mut out = String::from("/");
    if !path.starts_with('/') {
        push_components(&mut out, cwd);
    }
    push_components(&mut out, path);
    out
}

fn push_components(out: &mut String, path: &str) {
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
}

// ─── mkdir ────────────────────────────────────────────────────────────────────

/// `mkdir [-p] [-v] <path>...` — create a new directory via VFS IPC.
pub fn cmd_mkdir(args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut parents = false;
    let mut verbose = false;
    let mut paths = Vec::new();
    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg.chars().skip(1) {
                match c {
                    'p' => parents = true,
                    'v' => verbose = true,
                    _ => {}
                }
            }
        } else {
            paths.push(arg);
        }
    }
    if paths.is_empty() {
        crate::executor::shell_stderrln("Usage: mkdir [-p] [-v] <path>...");
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
                if vfs_req_ok(&api::ipc::VfsRequest::Mkdir(&prefix)) && verbose {
                    crate::executor::shell_println(&alloc::format!(
                        "mkdir: created directory '{prefix}'"
                    ));
                }
            }
        } else if vfs_req_ok(&api::ipc::VfsRequest::Mkdir(&resolved)) {
            if verbose {
                crate::executor::shell_println(&alloc::format!(
                    "mkdir: created directory '{path}'"
                ));
            }
        } else {
            crate::executor::shell_stderr("mkdir: cannot create directory '");
            crate::executor::shell_stderr(path);
            crate::executor::shell_stderrln("'");
        }
    }
    Ok(())
}

// ─── rmdir ────────────────────────────────────────────────────────────────────

/// `rmdir [-p] [-v] <path>...` — remove empty directories via VFS IPC.
pub fn cmd_rmdir(args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut parents = false;
    let mut verbose = false;
    let mut paths = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg.chars().skip(1) {
                match c {
                    'p' => parents = true,
                    'v' => verbose = true,
                    _ => {}
                }
            }
        } else {
            paths.push(arg);
        }
    }

    if paths.is_empty() {
        crate::executor::shell_stderrln("Usage: rmdir [-p] [-v] <path>...");
        return Ok(());
    }

    for path in paths {
        if parents {
            let mut cur = path;
            while !cur.is_empty() && cur != "/" && cur != "." {
                let resolved = resolve_shell_path(cur);
                if vfs_req_ok(&api::ipc::VfsRequest::Rmdir(&resolved)) {
                    if verbose {
                        crate::executor::shell_println(&alloc::format!(
                            "rmdir: removing directory, '{cur}'"
                        ));
                    }
                } else {
                    crate::executor::shell_stderr("rmdir: failed to remove '");
                    crate::executor::shell_stderr(cur);
                    crate::executor::shell_stderrln("' (not empty, protected, or not found)");
                    break;
                }
                let trimmed = cur.trim_end_matches('/');
                if let Some(pos) = trimmed.rfind('/') {
                    if pos == 0 {
                        cur = "/";
                    } else {
                        cur = &trimmed[..pos];
                    }
                } else {
                    break;
                }
            }
        } else {
            let resolved = resolve_shell_path(path);
            if vfs_req_ok(&api::ipc::VfsRequest::Rmdir(&resolved)) {
                if verbose {
                    crate::executor::shell_println(&alloc::format!(
                        "rmdir: removing directory, '{path}'"
                    ));
                }
            } else {
                crate::executor::shell_stderr("rmdir: failed to remove '");
                crate::executor::shell_stderr(path);
                crate::executor::shell_stderrln("' (not empty, protected, or not found)");
            }
        }
    }
    Ok(())
}

// ─── rm ───────────────────────────────────────────────────────────────────────

/// `rm [-r] [-f] [-i] <path>...` — remove a file, or (with -r on /data) a directory tree.
pub fn cmd_rm(args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut recursive = false;
    let mut force = false;
    let mut interactive = false;
    let mut paths = Vec::new();
    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg.chars().skip(1) {
                match c {
                    'r' | 'R' => recursive = true,
                    'f' => {
                        force = true;
                        interactive = false;
                    }
                    'i' => {
                        interactive = true;
                        force = false;
                    }
                    _ => {}
                }
            }
        } else {
            paths.push(arg);
        }
    }
    if paths.is_empty() {
        if !force {
            crate::executor::shell_stderrln("Usage: rm [-r] [-f] [-i] <path>...");
        }
        return Ok(());
    }

    let pipe = crate::executor::shell_stdin();
    let mut pipe_lines = if !pipe.is_empty() {
        let text = alloc::string::String::from_utf8_lossy(&pipe);
        let lines: Vec<String> = text.lines().map(String::from).collect();
        Some(lines.into_iter())
    } else {
        None
    };

    for path in paths {
        let resolved = resolve_shell_path(path);
        if interactive && !force {
            // The prompt is interaction, not command data: it must be visible before the read
            // blocks, even though the serial dispatcher only flushes its capture afterwards.
            crate::executor::shell_console("rm: remove '");
            crate::executor::shell_console(path);
            crate::executor::shell_console("'? (y/n) ");
            let line = if let Some(iter) = &mut pipe_lines {
                iter.next().unwrap_or_default()
            } else if crate::executor::serial_active() {
                // Serial session: answer arrives as session events (echoed by the shell,
                // Ctrl-C cancels the whole command); legacy fd 0 is never read.
                match crate::executor::terminal_prompt_line() {
                    Ok(crate::executor::PromptLine::Line(line)) => line,
                    Ok(crate::executor::PromptLine::Cancelled | crate::executor::PromptLine::Eof) => {
                        return Err(ViError::IO);
                    }
                    Err(_) => {
                        crate::executor::shell_stderrln("\nrm: input lost; remaining files not removed");
                        return Err(ViError::IO);
                    }
                }
            } else {
                let mut l = alloc::string::String::new();
                if ostd::io::stdin().read_line(&mut l).is_err() {
                    continue;
                }
                l
            };
            let trimmed = line.trim();
            if !trimmed.eq_ignore_ascii_case("y") && !trimmed.eq_ignore_ascii_case("yes") {
                continue;
            }
        }
        let ok = if recursive && resolved.starts_with("/data/") {
            rm_recursive(&resolved)
        } else {
            vfs_req_ok(&api::ipc::VfsRequest::Unlink(&resolved))
        };
        if !ok && !force {
            crate::executor::shell_stderr("rm: cannot remove '");
            crate::executor::shell_stderr(path);
            crate::executor::shell_stderrln("'");
        }
    }
    Ok(())
}

// ─── touch ────────────────────────────────────────────────────────────────────

/// `touch <path>...` — create an empty file or update timestamp via VFS IPC.
pub fn cmd_touch(args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut any = false;
    for path in args {
        if path.starts_with('-') {
            continue;
        }
        any = true;
        let resolved = resolve_shell_path(path);
        if stat_file_vfs(&resolved).is_none() && !write_file(&resolved, &[]) {
            crate::executor::shell_stderr("touch: cannot touch '");
            crate::executor::shell_stderr(path);
            crate::executor::shell_stderrln("'");
        }
    }
    if !any {
        crate::executor::shell_stderrln("Usage: touch <path>...");
    }
    Ok(())
}

// ─── mv ───────────────────────────────────────────────────────────────────────

fn is_directory_target(resolved: &str) -> bool {
    resolved.ends_with('/')
        || resolved == "/"
        || resolved == "/tmp"
        || resolved == "/data"
        || resolved == "/srv"
        || resolved == "/mnt"
        || resolved == "/mnt/sd"
        || resolved == "/bin"
        || resolved == "/etc"
        || matches!(stat_file_vfs(resolved), Some((_, true)))
}

fn copy_single_file(src: &str, dst: &str) -> bool {
    let data = match read_file_vfs_owned(src, 1024 * 1024) {
        Ok(d) => d,
        Err(_) => match read_file_bytes(src) {
            Ok(d) => d,
            Err(_) => return false,
        },
    };
    vfs_write_chunked(dst, &data, false)
}

fn copy_dir_recursive(src: &str, dst: &str) -> bool {
    let _ = vfs_req_ok(&api::ipc::VfsRequest::Mkdir(dst));
    let entries = match vfs_list_dir_details(src) {
        Some(e) => e,
        None => return false,
    };
    let mut ok = true;
    for (name, is_dir) in entries {
        let child_src = if src == "/" {
            alloc::format!("/{}", name)
        } else {
            alloc::format!("{}/{}", src.trim_end_matches('/'), name)
        };
        let child_dst = if dst == "/" {
            alloc::format!("/{}", name)
        } else {
            alloc::format!("{}/{}", dst.trim_end_matches('/'), name)
        };
        if is_dir {
            ok &= copy_dir_recursive(&child_src, &child_dst);
        } else {
            ok &= copy_single_file(&child_src, &child_dst);
        }
    }
    ok
}

/// `mv <source>... <target>` — rename/move files or directories via VFS IPC (atomic rename).
pub fn cmd_mv(args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut paths = Vec::new();
    for arg in args {
        if !arg.starts_with('-') || arg.len() == 1 {
            paths.push(arg);
        }
    }
    if paths.len() < 2 {
        crate::executor::shell_stderrln("Usage: mv <source>... <target>");
        return Ok(());
    }

    let target = paths.pop().unwrap();
    let target_resolved = resolve_shell_path(target);
    let target_is_dir = target.ends_with('/') || is_directory_target(&target_resolved);

    if paths.len() > 1 && !target_is_dir {
        crate::executor::shell_stderr("mv: target '");
        crate::executor::shell_stderr(target);
        crate::executor::shell_stderrln("' is not a directory");
        return Ok(());
    }

    for src in paths {
        let src_resolved = resolve_shell_path(src);
        let final_dst = if target_is_dir {
            let file_name = src_resolved.rsplit('/').next().unwrap_or(&src_resolved);
            if target_resolved == "/" {
                alloc::format!("/{}", file_name)
            } else {
                alloc::format!("{}/{}", target_resolved.trim_end_matches('/'), file_name)
            }
        } else {
            target_resolved.clone()
        };

        let moved = if src_resolved.starts_with("/srv/") && final_dst.starts_with("/srv/") {
            vfs_req_ok(&api::ipc::VfsRequest::Rename {
                old: &src_resolved,
                new: &final_dst,
            })
        } else {
            false
        };

        let ok = if moved {
            true
        } else {
            let is_dir = stat_file_vfs(&src_resolved)
                .map(|(_, d)| d)
                .unwrap_or(false);
            if is_dir {
                if copy_dir_recursive(&src_resolved, &final_dst) {
                    if src_resolved.starts_with("/data/") {
                        rm_recursive(&src_resolved)
                    } else {
                        vfs_req_ok(&api::ipc::VfsRequest::Rmdir(&src_resolved))
                    }
                } else {
                    false
                }
            } else if copy_single_file(&src_resolved, &final_dst) {
                vfs_req_ok(&api::ipc::VfsRequest::Unlink(&src_resolved))
            } else {
                false
            }
        };

        if !ok {
            crate::executor::shell_stderr("mv: cannot move '");
            crate::executor::shell_stderr(src);
            crate::executor::shell_stderr("' to '");
            crate::executor::shell_stderr(&final_dst);
            crate::executor::shell_stderrln("'");
        }
    }
    Ok(())
}

// ─── cp ───────────────────────────────────────────────────────────────────────

/// `cp [-r] [-R] <source>... <target>` — copy files or directories via VFS IPC.
pub fn cmd_cp(args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut recursive = false;
    let mut paths = Vec::new();
    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            recursive |= arg.contains('r') || arg.contains('R');
        } else {
            paths.push(arg);
        }
    }
    if paths.len() < 2 {
        crate::executor::shell_stderrln("Usage: cp [-r] <source>... <target>");
        return Ok(());
    }

    let target = paths.pop().unwrap();
    let target_resolved = resolve_shell_path(target);
    let target_is_dir = target.ends_with('/') || is_directory_target(&target_resolved);

    if paths.len() > 1 && !target_is_dir {
        crate::executor::shell_stderr("cp: target '");
        crate::executor::shell_stderr(target);
        crate::executor::shell_stderrln("' is not a directory");
        return Ok(());
    }

    let mut failed = false;
    for src in paths {
        let src_resolved = resolve_shell_path(src);
        let is_dir = stat_file_vfs(&src_resolved)
            .map(|(_, d)| d)
            .unwrap_or(false);

        let final_dst = if target_is_dir {
            let file_name = src_resolved.rsplit('/').next().unwrap_or(&src_resolved);
            if target_resolved == "/" {
                alloc::format!("/{}", file_name)
            } else {
                alloc::format!("{}/{}", target_resolved.trim_end_matches('/'), file_name)
            }
        } else {
            target_resolved.clone()
        };

        if is_dir {
            if !recursive {
                crate::executor::shell_stderr("cp: -r not specified; omitting directory '");
                crate::executor::shell_stderr(src);
                crate::executor::shell_stderrln("'");
                continue;
            }
            if !copy_dir_recursive(&src_resolved, &final_dst) {
                failed = true;
                crate::executor::shell_stderr("cp: failed to copy directory '");
                crate::executor::shell_stderr(src);
                crate::executor::shell_stderrln("'");
            }
        } else {
            if !copy_single_file(&src_resolved, &final_dst) {
                failed = true;
                crate::executor::shell_stderr("cp: cannot copy '");
                crate::executor::shell_stderr(src);
                crate::executor::shell_stderr("' to '");
                crate::executor::shell_stderr(&final_dst);
                crate::executor::shell_stderrln("'");
            }
        }
    }
    if failed { Err(ViError::IO) } else { Ok(()) }
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
            crate::executor::shell_stderrln("Usage: vwrite <path> <text>");
            return Ok(());
        }
    };
    let rest = args.collect::<alloc::vec::Vec<_>>().join(" ");
    if !write_file(path, rest.as_bytes()) {
        crate::executor::shell_stderr("vwrite: failed to write '");
        crate::executor::shell_stderr(path);
        crate::executor::shell_stderrln("'");
    }
    Ok(())
}

/// `vappend <path> <text>` — append text to a VFS path via OP_APPEND (test helper).
pub fn cmd_vappend(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = match args.next() {
        Some(p) => p,
        None => {
            crate::executor::shell_stderrln("Usage: vappend <path> <text>");
            return Ok(());
        }
    };
    let rest = args.collect::<alloc::vec::Vec<_>>().join(" ");
    if !append_file(path, rest.as_bytes()) {
        crate::executor::shell_stderr("vappend: failed to append '");
        crate::executor::shell_stderr(path);
        crate::executor::shell_stderrln("'");
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

/// Largest file `cat` reads, as text or as exact bytes.
pub(crate) const CAT_READ_MAX: usize = 1024 * 1024;

/// Read a whole file as exact bytes — no UTF-8 validation or replacement — bounded by
/// [`CAT_READ_MAX`]. VFS files are tried first (stat decides existence, kind and size), then the
/// kernel BootFS. Errors are the reason text shown after `cat: <path>: `; an oversized file
/// fails rather than truncating.
pub(crate) fn read_path_bytes_exact(resolved: &str) -> Result<Vec<u8>, &'static str> {
    if let Some((size, is_dir)) = stat_file_vfs(resolved) {
        if is_dir {
            return Err("Is a directory");
        }
        if size > CAT_READ_MAX {
            return Err("File too large");
        }
        if size == 0 {
            return Ok(Vec::new());
        }
        return read_file_vfs_owned(resolved, CAT_READ_MAX).map_err(|_| "Input/output error");
    }
    if !bootfs_fallback_allowed(resolved) {
        return Err("Permission denied");
    }
    let fd = syscall::sys_open(resolved).map_err(|_| "No such file or directory")?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    let result = loop {
        match syscall::sys_read(fd, &mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => {
                if bytes.len() + n > CAT_READ_MAX {
                    break Err("File too large");
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            Err(_) => break Err("Input/output error"),
        }
    };
    syscall::sys_close(fd);
    result.map(|()| bytes)
}

/// `vcat <path>` — print file content via VFS OP_READ (reads RamFS including /tmp/).
///
/// Unlike `cat`, which reads the kernel-embedded FS, `vcat` reads from the
/// VFS cell's RamFS — the same store that OP_WRITE targets.
pub fn cmd_vcat(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = match args.next() {
        Some(p) => p,
        None => {
            crate::executor::shell_stderrln("Usage: vcat <path>");
            return Ok(());
        }
    };
    let mut buf = [0u8; 4096];
    let n = read_file_vfs_result(path, &mut buf).inspect_err(|_| {
        crate::executor::shell_stderr("vcat: cannot read: ");
        crate::executor::shell_stderrln(path);
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
    let raw = match ostd::ipc::recv_from(vfs_tid, &mut reply) {
        Ok(raw) => raw,
        Err(_) => return,
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

/// List entries with directory metadata from the Userspace VFS Service via IPC.
pub(crate) fn vfs_list_dir_details(
    dir: &str,
) -> Option<alloc::vec::Vec<(alloc::string::String, bool)>> {
    use api::ipc::{VfsRequest, VfsResponse};
    let vfs_tid = vfs_endpoint();
    let mut send = [0u8; 512];
    let n = api::ipc::encode(&VfsRequest::ListDir(dir), &mut send)
        .ok()?
        .len();
    ostd::syscall::sys_send(vfs_tid, &send[..n]);
    let mut reply = [0u8; 512];
    if ostd::ipc::recv_from(vfs_tid, &mut reply).is_ok() {
        if let Ok(VfsResponse::Data(entries)) = api::ipc::decode::<VfsResponse>(&reply) {
            let text = core::str::from_utf8(entries).ok()?;
            let mut list = alloc::vec::Vec::new();
            for entry in text.lines() {
                if let Some(name) = entry.strip_prefix("d:") {
                    list.push((alloc::string::String::from(name), true));
                } else if let Some(name) = entry.strip_prefix("f:") {
                    list.push((alloc::string::String::from(name), false));
                }
            }
            return Some(list);
        }
    }
    None
}

/// List entries in a directory from the Userspace VFS Service via IPC.
#[allow(dead_code)]
pub(crate) fn vfs_list_dir(dir: &str) -> Option<alloc::vec::Vec<alloc::string::String>> {
    vfs_list_dir_details(dir).map(|entries| entries.into_iter().map(|(name, _)| name).collect())
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
            crate::executor::shell_stderr("uniq: cannot open '");
            crate::executor::shell_stderr(path);
            crate::executor::shell_stderrln("'");
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
            crate::executor::shell_stderr("sort: cannot open '");
            crate::executor::shell_stderr(path);
            crate::executor::shell_stderrln("'");
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
        crate::executor::shell_stderr("tee: cannot write '");
        crate::executor::shell_stderr(path);
        crate::executor::shell_stderrln("'");
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
            crate::executor::shell_stderr("awk: cannot open '");
            crate::executor::shell_stderr(path);
            crate::executor::shell_stderrln("'");
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
