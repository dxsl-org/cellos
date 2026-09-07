use ostd::fs;
use ostd::grant::GrantHandle;
use ostd::prelude::*;
use ostd::syscall;

pub fn cmd_help() -> ViResult<()> {
    crate::executor::shell_println("Cellos Shell v0.2.1 — built-in commands:");
    crate::executor::shell_println(
        "  Files:   cd  ls  cat  cp  mv  rm  mkdir  rmdir  touch  wc  head  tail  grep  sed  awk  find  uniq  sort",
    );
    crate::executor::shell_println(
        "  System:  ps  top  kill  pwd  uname  free  env  uptime  sleep  clear  exec",
    );
    crate::executor::shell_println(
        "  Shell:   help  history  echo  export  alias  unalias  jobs  source  .",
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
    ostd::io::print("\x1b[2J\x1b[1;1H"); // bypass sink — clear screen always goes to console
    Ok(())
}

pub fn cmd_exec(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let path = args.next();
    if path.is_none() {
        ostd::io::println("Usage: exec <path> [args...]");
        return Ok(());
    }
    let path = path.unwrap();

    let cmd_argv: Vec<String> = args.map(String::from).collect();

    // 1. Open file using Kernel FS (same as ls/cat)
    // This ensures consistency with 'ls' and avoids relying on potentially out-of-sync Userspace VFS.
    // 1. Open file using Kernel FS
    match ostd::fs::File::open(path) {
        Ok(file) => {
            ostd::io::print("exec: loading (KERNEL-FS) ");
            ostd::io::println(path);
            exec_load_and_spawn(file, path, &cmd_argv)?;
        }
        Err(_) => {
            // Fallback: Try with '/' prefix
            let mut rooted = String::from("/");
            rooted.push_str(path);
            match ostd::fs::File::open(&rooted) {
                Ok(file) => {
                    ostd::io::print("exec: loading (KERNEL-FS) ");
                    ostd::io::println(&rooted);
                    exec_load_and_spawn(file, &rooted, &cmd_argv)?;
                }
                Err(_) => {
                    ostd::io::print("exec: cannot open '");
                    ostd::io::print(path);
                    ostd::io::println("' (File not found)");
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
        ostd::io::println("exec: failed to read file.");
        return Ok(());
    }

    if data.len() >= 4 && (data[0] != 0x7F || data[1] != 0x45 || data[2] != 0x4C || data[3] != 0x46)
    {
        ostd::io::println("exec: Bad ELF magic.");
        return Ok(());
    }

    ostd::io::print("exec: spawning (");
    ostd::io::print_usize(data.len());
    ostd::io::println(" bytes)...");

    if !ostd::set_spawn_argv(cmd_argv) {
        ostd::io::println("exec: argv exceeds 512-byte transport limit.");
        return Ok(());
    }

    let grant = match GrantHandle::<u8>::alloc_copy_from_slice(&data) {
        Some(grant) => grant,
        None => {
            ostd::io::println("exec: grant allocation failed.");
            return Ok(());
        }
    };

    // Spawn through the exact-path ELF route so the kernel can authorize the
    // reviewed `(shell, SpawnFromElf, /bin/<target>)` edge.
    match syscall::sys_spawn_from_elf(grant.id(), data.len(), path) {
        syscall::SyscallResult::Ok(tid) => {
            ostd::io::print("exec: process spawned (pid ");
            ostd::io::print_usize(tid);
            ostd::io::println(")");

            // Wait for it
            match syscall::sys_wait(tid) {
                syscall::SyscallResult::Ok(_) => {
                    ostd::io::println("exec: process exited.");
                }
                _ => {
                    ostd::io::println("exec: wait failed.");
                }
            }
        }
        syscall::SyscallResult::Err(_) => {
            ostd::io::println("exec: spawn failed.");
        }
    }
    Ok(())
}
// Removed IPC Logic
/*
let vfs_cell_id = 3;
*/

struct LsEntry {
    name: alloc::string::String,
    is_dir: bool,
    size: u64,
}

pub fn cmd_ls(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut all = false;
    let mut long = false;
    let mut classify = false;
    let mut raw_path = "";

    while let Some(arg) = args.next() {
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

    let resolved = crate::cmd_fs::resolve_shell_path(raw_path);
    let mut dir_found = false;
    let mut entries: alloc::vec::Vec<LsEntry> = alloc::vec::Vec::new();

    if resolved == "/" {
        dir_found = true;
        // 1. Collect from Userspace VFS service (mount points and root files)
        if let Some(vfs_list) = crate::cmd_fs::vfs_list_dir_details("/") {
            for (name, is_dir) in vfs_list {
                if !entries.iter().any(|e| e.name == name) {
                    let full_path = alloc::format!("/{name}");
                    let size = if long {
                        crate::cmd_fs::stat_file_vfs(&full_path)
                            .map(|(s, _)| s as u64)
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    entries.push(LsEntry { name, is_dir, size });
                }
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
                let already = entries.iter().any(|e| e.name.eq_ignore_ascii_case(name));
                if !already {
                    let is_dir = matches!(entry.file_type, ostd::FileType::Directory);
                    entries.push(LsEntry {
                        name: alloc::string::String::from(name),
                        is_dir,
                        size: entry.size,
                    });
                }
            }
        }
    } else {
        // Specific path: query Userspace VFS first
        if let Some(vfs_list) = crate::cmd_fs::vfs_list_dir_details(&resolved) {
            dir_found = true;
            for (name, is_dir) in vfs_list {
                let full_path = alloc::format!("{}/{}", resolved.trim_end_matches('/'), name);
                let size = if long {
                    crate::cmd_fs::stat_file_vfs(&full_path)
                        .map(|(s, _)| s as u64)
                        .unwrap_or(0)
                } else {
                    0
                };
                entries.push(LsEntry { name, is_dir, size });
            }
        } else {
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
                        size: entry.size,
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
                            size: entry.size,
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
                    size: size as u64,
                });
            }
        }
    }

    if !dir_found && entries.is_empty() {
        let display_path = if raw_path.is_empty() { "/" } else { raw_path };
        ostd::io::print("ls: cannot access '");
        ostd::io::print(display_path);
        ostd::io::println("': No such file or directory");
        return Ok(());
    }
    if all {
        let mut with_dots = alloc::vec::Vec::new();
        with_dots.push(LsEntry {
            name: alloc::string::String::from("."),
            is_dir: true,
            size: 0,
        });
        with_dots.push(LsEntry {
            name: alloc::string::String::from(".."),
            is_dir: true,
            size: 0,
        });
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        with_dots.extend(entries);
        entries = with_dots;
    } else {
        entries.retain(|e| !e.name.starts_with('.'));
        entries.sort_by(|a, b| a.name.cmp(&b.name));
    }

    for entry in entries {
        let mut display_name = entry.name;
        if classify && entry.is_dir {
            display_name.push('/');
        }

        if long {
            let type_char = if entry.is_dir { 'd' } else { '-' };
            crate::executor::shell_println(&alloc::format!(
                "{type_char} {:>8} {}",
                entry.size,
                display_name
            ));
        } else {
            crate::executor::shell_println(&display_name);
        }
    }

    Ok(())
}

pub fn cmd_cat(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut paths = Vec::new();
    while let Some(path) = args.next() {
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
        // 1. Try VFS read first (covers /tmp, /data, /srv, etc.)
        if let Ok(bytes) = crate::cmd_fs::read_file_vfs_owned(&resolved, 1024 * 1024) {
            if let Ok(s) = core::str::from_utf8(&bytes) {
                crate::executor::shell_print(s);
                continue;
            }
        }

        // 2. Fallback to kernel sys_open (covers kernel BootFS /bin, /etc)
        match syscall::sys_open(&resolved) {
            Ok(fd) => {
                let mut buffer = [0u8; 256];
                let mut pending = 0;
                loop {
                    match syscall::sys_read(fd, &mut buffer[pending..]) {
                        Ok(n) if n > 0 => {
                            let total = pending + n;
                            match core::str::from_utf8(&buffer[..total]) {
                                Ok(s) => {
                                    crate::executor::shell_print(s);
                                    pending = 0;
                                }
                                Err(e) => {
                                    let valid_len = e.valid_up_to();
                                    if valid_len > 0 {
                                        let s =
                                            core::str::from_utf8(&buffer[..valid_len]).unwrap_or("");
                                        crate::executor::shell_print(s);
                                    }
                                    if let Some(error_len) = e.error_len() {
                                        crate::executor::shell_print("\u{FFFD}");
                                        let start = valid_len + error_len;
                                        let remaining = total - start;
                                        for i in 0..remaining {
                                            buffer[i] = buffer[start + i];
                                        }
                                        pending = remaining;
                                    } else {
                                        let remaining = total - valid_len;
                                        for i in 0..remaining {
                                            buffer[i] = buffer[valid_len + i];
                                        }
                                        pending = remaining;
                                    }
                                }
                            }
                        }
                        Ok(0) => {
                            if pending > 0 {
                                crate::executor::shell_print("\u{FFFD}");
                            }
                            break;
                        }
                        Err(_) => {
                            ostd::io::println("cat: read error");
                            break;
                        }
                        _ => break,
                    }
                }
                syscall::sys_close(fd);
            }
            Err(_) => {
                ostd::io::print("cat: ");
                ostd::io::print(path);
                ostd::io::println(": No such file or directory");
            }
        }
    }
    Ok(())
}

fn state_order(state: usize) -> usize {
    match state {
        1 => 0,
        0 => 1,
        2 => 2,
        3 => 3,
        _ => 4,
    }
}

pub fn cmd_ps(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut filter_tid: Option<usize> = None;
    let mut sort_by_state = false;
    loop {
        match args.next() {
            Some("-p") => {
                filter_tid = args.next().and_then(|s| s.parse().ok());
            }
            Some("-s") => sort_by_state = true,
            Some(_) => {}
            None => break,
        }
    }
    let mut buffer = [api::syscall::ProcessInfo::default(); 64];
    match syscall::sys_get_procs(&mut buffer) {
        Ok(count) => {
            let entries = &mut buffer[..count];
            if sort_by_state {
                entries.sort_unstable_by_key(|p| state_order(p.state));
            }
            crate::executor::shell_println("  PID  STATE      NAME");
            crate::executor::shell_println("  ---  ---------  ----------------");
            for info in entries.iter() {
                if filter_tid.map(|t| t != info.id).unwrap_or(false) {
                    continue;
                }
                let name = core::str::from_utf8(&info.name)
                    .unwrap_or("???")
                    .trim_matches('\0');
                let state_str = match info.state {
                    0 => "Ready",
                    1 => "Running",
                    2 => "Waiting",
                    3 => "Dead",
                    _ => "???",
                };
                crate::executor::shell_print(&alloc::format!(
                    "  {:<4} {:<10} {}\n",
                    info.id,
                    state_str,
                    name
                ));
            }
            Ok(())
        }
        Err(_) => {
            ostd::io::println("ps: failed to get process list");
            Ok(())
        }
    }
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

/// `top` — process telemetry observer backed by `GetProcs2`.
pub fn cmd_top<'a>(args: core::str::SplitWhitespace<'a>) -> ViResult<()> {
    crate::top::cmd_top(args)
}

// ─── kill ─────────────────────────────────────────────────────────────────────

/// `kill <tid>` — send a cooperative shutdown request to the target task.
///
/// Checks task state via `sys_get_procs` before sending to avoid blocking the
/// shell.  `sys_send` to a non-Recv task would put the shell in
/// `TaskState::Sending` indefinitely — so we only send when the target is
/// confirmed to be in Waiting (Recv) state.
///
/// Limitation: cannot terminate tasks blocked inside VFS/net IPC.  A kernel-level
/// `ForceExit` syscall is planned (see roadmap Phase X-6) to handle those cases.
pub fn cmd_kill(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let tid_str = match args.next() {
        Some(s) => s,
        None => {
            crate::executor::shell_println("Usage: kill <tid>");
            return Ok(());
        }
    };
    let mut tid: usize = 0;
    for ch in tid_str.bytes() {
        if !ch.is_ascii_digit() {
            crate::executor::shell_println("kill: invalid tid");
            return Ok(());
        }
        tid = tid.saturating_mul(10).saturating_add((ch - b'0') as usize);
    }
    if tid == 0 {
        crate::executor::shell_println("kill: invalid tid (0)");
        return Ok(());
    }

    // Safety check: only send to a task in Waiting (Recv-any) state.
    // Sending to a task in any other state blocks the shell indefinitely because
    // ipc_send puts the caller into TaskState::Sending until the target enters Recv.
    let mut procs = [api::syscall::ProcessInfo::default(); 16];
    let target_state = syscall::sys_get_procs(&mut procs)
        .ok()
        .and_then(|n| procs[..n].iter().find(|p| p.id == tid).map(|p| p.state));

    match target_state {
        None => {
            crate::executor::shell_print(&alloc::format!("kill: no task with tid {}\n", tid));
        }
        Some(2) => {
            // Waiting state = task is in sys_recv — safe to send the signal.
            let msg = [0xFFu8];
            syscall::sys_send(tid, &msg);
            crate::executor::shell_print(&alloc::format!(
                "kill: signal sent to task {} — run 'ps' to verify termination\n",
                tid
            ));
        }
        Some(3) => {
            crate::executor::shell_print(&alloc::format!("kill: task {} is already Dead\n", tid));
        }
        Some(_) => {
            // Task is Ready/Running/Sleeping — cooperative signal won't reach it.
            // Use ForceExit to terminate regardless of state.
            // Kernel rejects system cells (VFS=block_io_cap, net=network_cap); use hotswap for those.
            match syscall::sys_force_exit(tid) {
                syscall::SyscallResult::Ok(_) => {
                    crate::executor::shell_print(&alloc::format!(
                        "kill: task {} force-terminated\n",
                        tid
                    ));
                }
                syscall::SyscallResult::Err(_) => {
                    crate::executor::shell_print(&alloc::format!(
                        "kill: task {} not terminated (system cell — use hotswap; or no SpawnCap)\n",
                        tid
                    ));
                }
            }
        }
    }
    Ok(())
}
