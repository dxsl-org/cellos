use ostd::fs;
use ostd::grant::GrantHandle;
use ostd::prelude::*;
use ostd::syscall;

pub fn cmd_help() -> ViResult<()> {
    crate::executor::shell_println("Cellos Shell v0.2.1 — built-in commands:");
    crate::executor::shell_println(
        "  Files:   cd  ls  cat  cp  mv  rm  mkdir  rmdir  touch  wc  head  tail  grep  rg  sed  awk  find  uniq  sort  edit",
    );
    crate::executor::shell_println(
        "  System:  ps  top  pwd  uname  free  env  uptime  sleep  clear  exec  (power off: /bin/shutdown)",
    );
    crate::executor::shell_println(
        "  Shell:   help  history  echo  export  alias  unalias  jobs  source  .",
    );
    crate::executor::shell_println(
        "  Tier 3:  hv              start the hypervisor cell + guest on demand",
    );
    crate::executor::shell_println(
        "  Network: ssh             outbound SSH client (one connection, one session)",
    );
    crate::executor::shell_println(
        "           completion      completion record of the last ssh run",
    );
    crate::executor::shell_println("");
    crate::executor::shell_println("Syntax:  cmd | cmd2      (pipe)");
    crate::executor::shell_println("         cmd > file      (redirect stdout)");
    crate::executor::shell_println("         cmd < file      (redirect stdin)");
    crate::executor::shell_println("         cmd &           (background)");
    crate::executor::shell_println("         cmd ; cmd2      (sequence)");
    Ok(())
}

pub fn cmd_clear() -> ViResult<()> {
    // Screen control is not command data: always straight to the terminal, never captured.
    crate::executor::shell_console("\x1b[2J\x1b[1;1H");
    Ok(())
}

pub fn cmd_exec(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = args.next();
    if path.is_none() {
        crate::executor::shell_stderrln("Usage: exec <path> [args...]");
        return Ok(());
    }
    let path = path.unwrap();

    let cmd_argv: Vec<String> = args.map(String::from).collect();

    // 1. Open file using Kernel FS (same as ls/cat)
    // This ensures consistency with 'ls' and avoids relying on potentially out-of-sync Userspace VFS.
    // 1. Open file using Kernel FS
    match ostd::fs::File::open(path) {
        Ok(file) => {
            crate::executor::shell_print("exec: loading (KERNEL-FS) ");
            crate::executor::shell_println(path);
            exec_load_and_spawn(file, path, &cmd_argv)?;
        }
        Err(_) => {
            // Fallback: Try with '/' prefix
            let mut rooted = String::from("/");
            rooted.push_str(path);
            match ostd::fs::File::open(&rooted) {
                Ok(file) => {
                    crate::executor::shell_print("exec: loading (KERNEL-FS) ");
                    crate::executor::shell_println(&rooted);
                    exec_load_and_spawn(file, &rooted, &cmd_argv)?;
                }
                Err(_) => {
                    crate::executor::shell_stderr("exec: cannot open '");
                    crate::executor::shell_stderr(path);
                    crate::executor::shell_stderrln("' (File not found)");
                }
            }
        }
    }

    Ok(())
}

fn exec_load_and_spawn(mut file: ostd::fs::File, path: &str, cmd_argv: &[String]) -> ViResult<()> {
    // Read file into memory
    let mut data = Vec::new();
    if file.read_to_end(&mut data).is_err() {
        crate::executor::shell_stderrln("exec: failed to read file.");
        return Ok(());
    }

    if data.len() >= 4 && (data[0] != 0x7F || data[1] != 0x45 || data[2] != 0x4C || data[3] != 0x46)
    {
        crate::executor::shell_stderrln("exec: Bad ELF magic.");
        return Ok(());
    }

    crate::executor::shell_println(&alloc::format!("exec: spawning ({} bytes)...", data.len()));

    if !ostd::set_spawn_argv(cmd_argv) {
        crate::executor::shell_stderrln("exec: argv exceeds 512-byte transport limit.");
        return Ok(());
    }

    let grant = match GrantHandle::<u8>::alloc_copy_from_slice(&data) {
        Some(grant) => grant,
        None => {
            crate::executor::shell_stderrln("exec: grant allocation failed.");
            return Ok(());
        }
    };

    // Spawn through the exact-path ELF route so the kernel can authorize the
    // reviewed `(shell, SpawnFromElf, /bin/<target>)` edge.
    match syscall::sys_spawn_from_elf(grant.id(), data.len(), path) {
        syscall::SyscallResult::Ok(tid) => {
            crate::executor::shell_println(&alloc::format!("exec: process spawned (pid {})", tid));

            // Wait for it
            match syscall::sys_wait(tid) {
                syscall::SyscallResult::Ok(_) => {
                    crate::executor::shell_println("exec: process exited.");
                }
                _ => {
                    crate::executor::shell_stderrln("exec: wait failed.");
                }
            }
        }
        syscall::SyscallResult::Err(_) => {
            crate::executor::shell_stderrln("exec: spawn failed.");
        }
    }
    Ok(())
}
// Removed IPC Logic
/*
let vfs_cell_id = 3;
*/

pub(crate) struct LsEntry {
    pub(crate) name: alloc::string::String,
    pub(crate) is_dir: bool,
    /// `None` when the size was not requested or the backend could not report it.
    pub(crate) size: Option<u64>,
}

/// Append an entry unless the directory already lists that name.
///
/// A directory can be served by more than one mount — `/bin` on the Pi is the
/// VIFS1 FAT volume *and* the CellosFS overlay — and the same cell then arrives
/// twice. `ls /` always deduplicated; `ls <dir>` did not, which is why the board
/// showed `config config`, `input input`, `shell shell`, `vfs vfs`.
fn push_unique(
    entries: &mut alloc::vec::Vec<LsEntry>,
    name: alloc::string::String,
    is_dir: bool,
    size: Option<u64>,
) {
    if entries.iter().any(|e| e.name.eq_ignore_ascii_case(&name)) {
        return;
    }
    entries.push(LsEntry { name, is_dir, size });
}

/// Collect, de-duplicate, filter and sort the entries `ls` would show for `raw_path`.
///
/// `long` requests per-entry VFS sizes (BootFS sizes are always known); `all` keeps dot entries
/// and adds the synthetic `.`/`..`. Returns `None` when the path does not exist.
pub(crate) fn ls_collect(
    raw_path: &str,
    long: bool,
    all: bool,
) -> Option<alloc::vec::Vec<LsEntry>> {
    let resolved = crate::cmd_fs::resolve_shell_path(raw_path);
    let mut dir_found = false;
    let mut entries: alloc::vec::Vec<LsEntry> = alloc::vec::Vec::new();

    if resolved == "/" {
        dir_found = true;
        // 1. Collect from Userspace VFS service (mount points and root files)
        if let Some(vfs_list) = crate::cmd_fs::vfs_list_dir_details("/") {
            for (name, is_dir) in vfs_list {
                let full_path = alloc::format!("/{name}");
                let size = if long {
                    crate::cmd_fs::stat_file_vfs(&full_path).map(|(s, _)| s as u64)
                } else {
                    None
                };
                push_unique(&mut entries, name, is_dir, size);
            }
        }

        // 2. Collect from Kernel BootFS (VIFS1), deduplicating case-insensitively
        if let Ok(iter) = fs::read_dir("/") {
            for entry in iter {
                let name = core::str::from_utf8(&entry.name)
                    .unwrap_or("???")
                    .trim_matches('\0');
                if name.is_empty() {
                    continue;
                }
                let is_dir = matches!(entry.file_type, ostd::FileType::Directory);
                push_unique(
                    &mut entries,
                    alloc::string::String::from(name),
                    is_dir,
                    Some(entry.size),
                );
            }
        }
    } else {
        // Specific path: query Userspace VFS first
        if let Some(vfs_list) = crate::cmd_fs::vfs_list_dir_details(&resolved) {
            dir_found = true;
            for (name, is_dir) in vfs_list {
                let full_path = alloc::format!("{}/{}", resolved.trim_end_matches('/'), name);
                let size = if long {
                    crate::cmd_fs::stat_file_vfs(&full_path).map(|(s, _)| s as u64)
                } else {
                    None
                };
                push_unique(&mut entries, name, is_dir, size);
            }
        } else if crate::cmd_fs::bootfs_fallback_allowed(&resolved) {
            // Fallback to Kernel BootFS with resolved path
            let mut found = false;
            if let Ok(iter) = fs::read_dir(&resolved) {
                dir_found = true;
                for entry in iter {
                    let name = core::str::from_utf8(&entry.name)
                        .unwrap_or("???")
                        .trim_matches('\0');
                    if name.is_empty() {
                        continue;
                    }
                    let is_dir = matches!(entry.file_type, ostd::FileType::Directory);
                    entries.push(LsEntry {
                        name: alloc::string::String::from(name),
                        is_dir,
                        size: Some(entry.size),
                    });
                    found = true;
                }
            }
            if !found && !raw_path.is_empty() && raw_path != resolved {
                if let Ok(iter) = fs::read_dir(raw_path) {
                    dir_found = true;
                    for entry in iter {
                        let name = core::str::from_utf8(&entry.name)
                            .unwrap_or("???")
                            .trim_matches('\0');
                        if name.is_empty() {
                            continue;
                        }
                        let is_dir = matches!(entry.file_type, ostd::FileType::Directory);
                        entries.push(LsEntry {
                            name: alloc::string::String::from(name),
                            is_dir,
                            size: Some(entry.size),
                        });
                    }
                }
            }
        }
    }

    if !dir_found && entries.is_empty() {
        if let Some((size, is_dir)) = crate::cmd_fs::stat_file_vfs(&resolved) {
            if is_dir {
                dir_found = true;
            } else {
                let name = resolved.rsplit('/').next().unwrap_or(&resolved);
                entries.push(LsEntry {
                    name: alloc::string::String::from(name),
                    is_dir: false,
                    size: Some(size as u64),
                });
            }
        }
    }

    if !dir_found && entries.is_empty() {
        return None;
    }
    if all {
        let mut with_dots = alloc::vec::Vec::new();
        with_dots.push(LsEntry {
            name: alloc::string::String::from("."),
            is_dir: true,
            size: Some(0),
        });
        with_dots.push(LsEntry {
            name: alloc::string::String::from(".."),
            is_dir: true,
            size: Some(0),
        });
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        with_dots.extend(entries);
        entries = with_dots;
    } else {
        entries.retain(|e| !e.name.starts_with('.'));
        entries.sort_by(|a, b| a.name.cmp(&b.name));
    }

    Some(entries)
}

pub fn cmd_ls(args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut all = false;
    let mut long = false;
    let mut classify = false;
    let mut raw_path = "";

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg.chars().skip(1) {
                match c {
                    'a' | 'A' => all = true,
                    'l' => long = true,
                    'F' => classify = true,
                    _ => {}
                }
            }
        } else if raw_path.is_empty() {
            raw_path = arg;
        }
    }

    let Some(entries) = ls_collect(raw_path, long, all) else {
        let display_path = if raw_path.is_empty() { "/" } else { raw_path };
        crate::executor::shell_stderr("ls: cannot access '");
        crate::executor::shell_stderr(display_path);
        crate::executor::shell_stderrln("': No such file or directory");
        return Err(ViError::NotFound);
    };

    for entry in entries {
        let mut display_name = entry.name;
        if classify && entry.is_dir {
            display_name.push('/');
        }

        if long {
            let type_char = if entry.is_dir { 'd' } else { '-' };
            crate::executor::shell_println(&alloc::format!(
                "{type_char} {:>8} {}",
                entry.size.unwrap_or(0),
                display_name
            ));
        } else {
            crate::executor::shell_println(&display_name);
        }
    }

    Ok(())
}

pub fn cmd_cat(args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut paths = Vec::new();
    for path in args {
        paths.push(path);
    }

    if paths.is_empty() {
        let stdin_bytes = crate::executor::shell_stdin();
        if !stdin_bytes.is_empty() {
            if let Ok(s) = core::str::from_utf8(&stdin_bytes) {
                crate::executor::shell_print(s);
            }
        }
        return Ok(());
    }

    // POSIX: a failed operand is diagnosed, later operands are still written, and the
    // command exits nonzero. The first failure is the one reported.
    let mut failure: Option<ViError> = None;
    for path in paths {
        if path == "-" {
            let stdin_bytes = crate::executor::shell_stdin();
            if !stdin_bytes.is_empty() {
                if let Ok(s) = core::str::from_utf8(&stdin_bytes) {
                    crate::executor::shell_print(s);
                }
            }
            continue;
        }

        let resolved = crate::cmd_fs::resolve_shell_path(path);
        // 1. A path the VFS has (/tmp, /data, /srv, a disk mount) is read there and no
        //    longer falls through to a false "No such file or directory" (S10-3b): its text
        //    is printed exactly as the BootFS branch prints, one U+FFFD per invalid UTF-8
        //    sequence; a directory is refused.
        let mut too_large = false;
        if let Some((size, is_dir)) = crate::cmd_fs::stat_file_vfs(&resolved) {
            match cat_vfs_plan(size, is_dir) {
                CatVfs::Read => {
                    let read = if size == 0 {
                        Ok(Vec::new())
                    } else {
                        crate::cmd_fs::read_file_vfs_owned(&resolved, crate::cmd_fs::CAT_READ_MAX)
                    };
                    match read {
                        Ok(mut bytes) => {
                            let len = bytes.len();
                            emit_utf8_lossy(&mut bytes, len, true, &mut |s| crate::executor::shell_print(s));
                        }
                        Err(_) => {
                            cat_failed(path, "Input/output error");
                            failure.get_or_insert(ViError::IO);
                        }
                    }
                    continue;
                }
                CatVfs::Directory => {
                    cat_failed(path, "Is a directory");
                    failure.get_or_insert(ViError::IsADirectory);
                    continue;
                }
                // A boot-image file (`/bin` is the VIFS1 ramdisk seen through the VFS)
                // still streams from BootFS below at any size; any other file over the
                // bound is refused there instead of "No such file or directory".
                CatVfs::TooLarge => too_large = true,
            }
        }

        // 2. Fallback to kernel sys_open (covers kernel BootFS /bin, /etc) unless VFS refused.
        if !crate::cmd_fs::bootfs_fallback_allowed(&resolved) {
            crate::executor::shell_stderr("cat: ");
            crate::executor::shell_stderr(path);
            crate::executor::shell_stderrln(": Permission denied");
            failure.get_or_insert(ViError::PermissionDenied);
            continue;
        }
        match syscall::sys_open(&resolved) {
            Ok(fd) => {
                let mut buffer = [0u8; 256];
                let mut pending = 0;
                loop {
                    match syscall::sys_read(fd, &mut buffer[pending..]) {
                        Ok(n) if n > 0 => {
                            pending = emit_utf8_lossy(&mut buffer, pending + n, false, &mut |s| {
                                crate::executor::shell_print(s)
                            });
                        }
                        Ok(0) => {
                            emit_utf8_lossy(&mut buffer, pending, true, &mut |s| crate::executor::shell_print(s));
                            break;
                        }
                        Err(_) => {
                            crate::executor::shell_stderrln("cat: read error");
                            break;
                        }
                        _ => break,
                    }
                }
                syscall::sys_close(fd);
            }
            Err(_) if too_large => {
                cat_failed(path, "File too large");
                failure.get_or_insert(ViError::InvalidArgument);
            }
            Err(_) => {
                crate::executor::shell_stderr("cat: ");
                crate::executor::shell_stderr(path);
                crate::executor::shell_stderrln(": No such file or directory");
                failure.get_or_insert(ViError::NotFound);
            }
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// How `cat` treats a path the VFS has (S10-3b, D3b-2 A).
#[derive(Debug, PartialEq, Eq)]
enum CatVfs {
    /// Read it whole and print it as lossy text.
    Read,
    Directory,
    /// Over [`crate::cmd_fs::CAT_READ_MAX`]: streamed only if BootFS has it.
    TooLarge,
}

fn cat_vfs_plan(size: usize, is_dir: bool) -> CatVfs {
    if is_dir {
        CatVfs::Directory
    } else if size > crate::cmd_fs::CAT_READ_MAX {
        CatVfs::TooLarge
    } else {
        CatVfs::Read
    }
}

/// `cat: PATH: REASON` on stderr.
fn cat_failed(path: &str, reason: &str) {
    crate::executor::shell_stderr("cat: ");
    crate::executor::shell_stderr(path);
    crate::executor::shell_stderr(": ");
    crate::executor::shell_stderrln(reason);
}

/// Emit `buffer[..len]` as text: valid UTF-8 unchanged, each invalid sequence as one U+FFFD.
/// An incomplete sequence at the end is moved to the front of `buffer` and its length (at most
/// 3) returned, to be completed by the next read; when `last`, it is replaced by U+FFFD too.
fn emit_utf8_lossy(buffer: &mut [u8], len: usize, last: bool, emit: &mut dyn FnMut(&str)) -> usize {
    let mut start = 0;
    loop {
        match core::str::from_utf8(&buffer[start..len]) {
            Ok(text) => {
                if !text.is_empty() {
                    emit(text);
                }
                return 0;
            }
            Err(error) => {
                let valid = start + error.valid_up_to();
                if valid > start {
                    emit(core::str::from_utf8(&buffer[start..valid]).unwrap_or(""));
                }
                match error.error_len() {
                    Some(bad) => {
                        emit("\u{FFFD}");
                        start = valid + bad;
                    }
                    None if last => {
                        emit("\u{FFFD}");
                        return 0;
                    }
                    None => {
                        buffer.copy_within(valid..len, 0);
                        return len - valid;
                    }
                }
            }
        }
    }
}

const PS_USAGE: &str = "Usage: ps [-p TID] [-s]";

/// `ps` flags: `(-p TID, -s)`. Unknown or repeated flags are usage errors.
fn ps_options(args: &[String]) -> Result<(Option<u64>, bool), &'static str> {
    let mut filter_tid: Option<u64> = None;
    let mut sort_by_state = false;
    let mut words = args.iter().map(String::as_str);
    while let Some(arg) = words.next() {
        match arg {
            "-p" if filter_tid.is_none() => {
                filter_tid = Some(words.next().and_then(|tid| tid.parse().ok()).ok_or(PS_USAGE)?);
            }
            "-s" if !sort_by_state => sort_by_state = true,
            _ => return Err(PS_USAGE),
        }
    }
    Ok((filter_tid, sort_by_state))
}

/// `ps [-p TID] [-s]` — every task from one telemetry capture, rendered by the shared
/// `top` text renderer in task view (D64-3). `-p` keeps one task, `-s` orders by state.
/// Bad options exit 2; denied, failed or incomplete telemetry exits 1 with no table.
pub fn cmd_ps(args: &[String]) -> i32 {
    let (filter_tid, sort_by_state) = match ps_options(args) {
        Ok(options) => options,
        Err(usage) => {
            crate::executor::shell_stderrln(usage);
            return text_engine::top::USAGE_STATUS;
        }
    };
    let mut frame = match crate::top::capture_tasks(filter_tid) {
        Ok(frame) => frame,
        Err(error) => {
            crate::executor::shell_stderrln(&error.message("ps"));
            return error.status();
        }
    };
    if sort_by_state {
        frame.sort(&text_engine::top::SortSpec::new(text_engine::top::Field::State));
    }
    let mut text = String::new();
    text_engine::top::text::render_frame(&frame, None, &mut text);
    crate::executor::shell_print(&text);
    0
}

/// Build `echo` output bytes (`"a b c\n"`) without printing.
///
/// Used by the shell redirect path to capture echo output for OP_WRITE.
pub fn cmd_echo_to_vec(args: &[&str]) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        out.extend_from_slice(a.as_bytes());
    }
    out.push(b'\n');
    out
}

/// Expand `\n`, `\t`, `\\`, `\r` escape sequences in `s`.
fn expand_echo_escapes(s: &str) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('0') => out.push('\0'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `echo [-e] [-n] a b c` — print args joined by a single space.
///
/// `-e` interprets escape sequences (`\n`, `\t`, `\\`, `\r`).
/// `-n` suppresses the trailing newline.
pub fn cmd_echo(args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let parts: alloc::vec::Vec<&str> = args.collect();
    let mut escape = false;
    let mut no_newline = false;
    let mut word_start = 0;
    // Consume leading flags.
    for (i, &a) in parts.iter().enumerate() {
        if a == "-e" {
            escape = true;
            word_start = i + 1;
        } else if a == "-n" {
            no_newline = true;
            word_start = i + 1;
        } else if a == "-en" || a == "-ne" {
            escape = true;
            no_newline = true;
            word_start = i + 1;
        } else {
            break;
        }
    }
    let text = parts[word_start..].join(" ");
    if escape {
        crate::executor::shell_print(&expand_echo_escapes(&text));
    } else {
        crate::executor::shell_print(&text);
    }
    if !no_newline {
        crate::executor::shell_print("\n");
    }
    Ok(())
}

// ─── top ──────────────────────────────────────────────────────────────────────

/// `top` — Cell-aware telemetry observer over `TelemetrySnapshot`; returns the exit status.
pub fn cmd_top(args: &[String]) -> i32 {
    crate::top::cmd_top(args)
}

// ─── refused lifecycle builtins (S2c-min) ─────────────────────────────────────

/// Exit status of a refused lifecycle builtin.
pub const REFUSED_STATUS: i32 = 1;

/// `kill` — refused. The suite shell holds no lifecycle authority: it neither sends the
/// former cooperative IPC byte nor attempts `ForceExit`; no syscall is issued.
pub fn cmd_kill() -> i32 {
    crate::executor::shell_stderrln("kill: not available from the suite shell, requires a lifecycle owner");
    REFUSED_STATUS
}

/// `snapshot` — refused. The Supervisor no longer accepts the shell as a snapshot caller;
/// no request is sent.
pub fn cmd_snapshot() -> i32 {
    crate::executor::shell_stderrln("snapshot: not available from the suite shell, requires a lifecycle owner");
    REFUSED_STATUS
}

#[cfg(test)]
mod cat_tests {
    use super::{cat_vfs_plan, emit_utf8_lossy, CatVfs};
    use alloc::string::String;
    use alloc::vec::Vec;

    /// Feed `bytes` in `chunk`-sized reads through the cat decoder, as `cmd_cat` does.
    fn decode(bytes: &[u8], chunk: usize) -> String {
        let mut out = String::new();
        let mut buffer = [0u8; 256];
        let mut pending = 0;
        for piece in bytes.chunks(chunk) {
            buffer[pending..pending + piece.len()].copy_from_slice(piece);
            pending = emit_utf8_lossy(&mut buffer, pending + piece.len(), false, &mut |s| out.push_str(s));
        }
        emit_utf8_lossy(&mut buffer, pending, true, &mut |s| out.push_str(s));
        out
    }

    #[test]
    fn every_byte_after_an_invalid_one_is_kept_at_any_read_size() {
        let mut bytes: Vec<u8> = b"head \x9b31m mid \x9d0;osc\x9c tail \xe2\x82\xac end\n".to_vec();
        bytes.extend_from_slice(b"\xff\xfe last");
        let want = "head \u{FFFD}31m mid \u{FFFD}0;osc\u{FFFD} tail \u{20ac} end\n\u{FFFD}\u{FFFD} last";
        for chunk in [1, 2, 3, 7, 64, 253] {
            assert_eq!(decode(&bytes, chunk), want, "chunk {chunk}");
        }
    }

    #[test]
    fn a_truncated_sequence_at_the_end_is_one_replacement() {
        for chunk in [1, 2, 256 - 3] {
            assert_eq!(decode(b"abc\xe2\x82", chunk), "abc\u{FFFD}");
        }
    }

    /// S10-3b: a VFS file is emitted from one whole buffer (`last`), with the same text as
    /// the chunked BootFS reads; ESC sequences pass through to the terminal's own policy.
    #[test]
    fn a_whole_vfs_buffer_decodes_like_the_chunked_bootfs_reads() {
        let mut bytes: Vec<u8> = b"\x1b[31mred\x1b[0m \x9b2J \xe2\x82\xac \xf0\x9f".to_vec();
        let want = "\u{1b}[31mred\u{1b}[0m \u{FFFD}2J \u{20ac} \u{FFFD}";
        assert_eq!(decode(&bytes, 5), want);
        let mut out = String::new();
        let len = bytes.len();
        assert_eq!(emit_utf8_lossy(&mut bytes, len, true, &mut |s| out.push_str(s)), 0);
        assert_eq!(out, want);
    }

    #[test]
    fn a_vfs_directory_or_oversized_file_is_never_read() {
        let max = crate::cmd_fs::CAT_READ_MAX;
        assert_eq!(cat_vfs_plan(0, false), CatVfs::Read);
        assert_eq!(cat_vfs_plan(max, false), CatVfs::Read);
        assert_eq!(cat_vfs_plan(max + 1, false), CatVfs::TooLarge);
        assert_eq!(cat_vfs_plan(7 * 1024 * 1024, false), CatVfs::TooLarge);
        assert_eq!(cat_vfs_plan(0, true), CatVfs::Directory);
    }
}

#[cfg(test)]
mod ls_tests {
    use super::{push_unique, LsEntry};

    /// A directory served by two mounts must list each name once, whatever the
    /// case the two views report it with — the board saw `/bin` from the VIFS1
    /// FAT volume and from the CellosFS overlay at the same time.
    #[test]
    fn a_name_from_a_second_mount_is_not_listed_twice() {
        let mut entries: alloc::vec::Vec<LsEntry> = alloc::vec::Vec::new();
        push_unique(
            &mut entries,
            alloc::string::String::from("config"),
            false,
            Some(0),
        );
        push_unique(&mut entries, alloc::string::String::from("input"), false, Some(0));
        push_unique(
            &mut entries,
            alloc::string::String::from("config"),
            false,
            Some(0),
        );
        push_unique(
            &mut entries,
            alloc::string::String::from("CONFIG"),
            false,
            Some(0),
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "config");
        assert_eq!(entries[1].name, "input");
    }
}

#[cfg(test)]
mod ps_top_tests {
    use super::ps_options;
    use crate::typed::{classify_command, CmdClass};
    use alloc::string::String;
    use alloc::vec::Vec;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| String::from(*word)).collect()
    }

    #[test]
    fn ps_accepts_its_flags_once_and_refuses_the_rest() {
        assert_eq!(ps_options(&argv(&[])), Ok((None, false)));
        assert_eq!(ps_options(&argv(&["-p", "7", "-s"])), Ok((Some(7), true)));
        for bad in [&["-x"][..], &["-p"], &["-p", "x"], &["-p", "1", "-p", "2"], &["-s", "-s"], &["--records"]] {
            assert!(ps_options(&argv(bad)).is_err(), "{:?}", bad);
        }
    }

    #[test]
    fn top_options_classify_sessions_and_report_usage_as_status_2() {
        assert_eq!(classify_command(&argv(&["top"])), CmdClass::Session);
        assert_eq!(classify_command(&argv(&["top", "--tasks", "-d", "0.5"])), CmdClass::Session);
        assert_eq!(classify_command(&argv(&["top", "-b", "-n", "2"])), CmdClass::Builtin);
        // Bad options are not a session: the built-in runs and exits with the usage status.
        for bad in [&["top", "--bogus"][..], &["top", "--tasks", "-o", "mem"], &["top", "--tasks", "--cell", "1:2"]] {
            assert_eq!(classify_command(&argv(bad)), CmdClass::Builtin, "{:?}", bad);
            let words: Vec<&str> = bad[1..].to_vec();
            assert!(text_engine::top::parse_options(&words).is_err());
        }
        assert_eq!(text_engine::top::USAGE_STATUS, 2);
    }
}
