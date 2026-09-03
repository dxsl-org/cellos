//! Shell AST executor — runs parsed commands, handles pipes and redirects.
//!
//! Pipes between built-in commands are implemented via an in-memory capture
//! stack (`shell_state`): each pipeline stage's output is captured into a
//! `Vec<u8>`, then passed as stdin to the next stage. `CaptureGuard` (RAII,
//! Law 8) pops the capture on every exit path, including early returns.
//!
//! All shell state lives in `shell_state` behind locks; this module holds no
//! mutable statics of its own.

extern crate alloc;

use crate::jobs::{JobState, Jobs};
use crate::parser::{Ast, Cmd, QuoteStyle, Redirect, Word};
use crate::shell_state::{self as state, CaptureGuard, LoopSignal};
use crate::text_engine::args::{with_legacy_parts, ArgCursor, UtilityStatus};
use crate::text_engine::records::{extend_input, InputBufferError, MAX_INPUT_BYTES};
use crate::text_tools::{awk, sed};
use alloc::string::String;
use alloc::vec::Vec;
use ostd::prelude::*;
use ostd::syscall;

/// Route command output through the current sink.
///
/// All built-in output calls this instead of `ostd::io::print` so pipeline
/// capture works.  The prompt and internal error diagnostics call
/// `ostd::io::print` directly to always reach the console regardless of sink.
pub fn shell_print(s: &str) {
    state::write_out(s);
}

/// `shell_print(s)` followed by a newline.
pub fn shell_println(s: &str) {
    shell_print(s);
    shell_print("\n");
}

/// Return the current pipe-fed stdin bytes, empty when no pipe is active.
///
/// Commands that accept either a file argument or stdin (e.g., `grep`, `wc`)
/// call this when no file path is given. Owned rather than borrowed — see
/// `shell_state::stdin_bytes`.
pub fn shell_stdin() -> Vec<u8> {
    state::stdin_bytes()
}

/// All recognized shell built-in names, used by tab completion.
#[cfg(not(feature = "shell_test"))] // reason: tab completion only exists in the interactive REPL
pub const BUILTINS: &[&str] = &[
    "alias", "awk", "bg", "blktest", "break", "cat", "cd", "clear", "continue", "echo", "env",
    "exec", "exit", "export", "fg", "find", "free", "grep", "head", "help", "jobs", "kill", "ls",
    "mkdir", "ps", "pwd", "read", "rm", "rmdir", "sed", "shutdown", "sleep", "snapshot", "sort",
    "source", "tail", "tee", "test", "top", "unalias", "uniq", "unset", "uname", "uptime",
    "vappend", "vcat", "vwrite", "wc",
];

// ── Shell-global state ────────────────────────────────────────────────────────
//
// Functions, variables, the exit flag and the loop signal all live in
// `shell_state` behind locks. These aliases keep the call sites in this module
// short and mark which of them the rest of the crate may use.

#[cfg(not(feature = "shell_test"))] // reason: only the REPL loop can honour an exit request
pub use crate::shell_state::take_exit_request;
pub use crate::shell_state::{define_function, request_exit};
use crate::shell_state::{
    get_function, get_var, set_loop_signal, set_var, take_loop_signal, unset_var,
};

/// Capture one supported command for `$(...)`.
///
/// A capture failure aborts the containing command so it cannot substitute
/// fabricated output.
fn run_capture(inner: &str) -> Result<String, ()> {
    let mut words = inner.split_whitespace();
    let cmd = match words.next() {
        Some(command) => command,
        None => return Ok(String::new()),
    };
    let args: alloc::vec::Vec<&str> = words.collect();
    let output = match cmd {
        "echo" => {
            let bytes = crate::commands::cmd_echo_to_vec(&args);
            String::from(core::str::from_utf8(&bytes).unwrap_or(""))
        }
        "vcat" | "cat" => {
            let Some(path) = args.first() else {
                return Ok(String::new());
            };
            match crate::cmd_fs::read_file_vfs_owned(path, 4096) {
                Ok(bytes) => String::from(core::str::from_utf8(&bytes).unwrap_or("")),
                Err(_) => {
                    ostd::io::println("shell: command substitution read failed");
                    String::new()
                }
            }
        }
        "pwd" => {
            let mut cwd = crate::cmd_cwd::get_shell_cwd().map_err(|_| ())?;
            cwd.push('\n');
            cwd
        }
        _ => String::new(),
    };
    Ok(output)
}

/// Expand variable and single-level command substitutions in one token.
///
/// A failed `pwd` capture propagates so the containing command returns nonzero.
fn expand_token(s: &str) -> Result<String, ()> {
    if !s.contains('$') {
        return Ok(String::from(s));
    }
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'(' {
                // Command substitution: $(...). Scan to matching ')'.
                // Single-level only — nested $( $() ) passes through as literal.
                let inner_start = i + 2;
                let mut depth = 1usize;
                let mut j = inner_start;
                while j < bytes.len() {
                    if bytes[j] == b'(' {
                        depth += 1;
                    } else if bytes[j] == b')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if depth == 0 {
                    // bytes[inner_start..j] is a run of shell token chars, which the
                    // parser guarantees are ASCII — the checked decode cannot fail.
                    let inner = core::str::from_utf8(&bytes[inner_start..j]).unwrap_or("");
                    // Reject nested $(...) — pass $(  literally so the user can see the issue.
                    if !inner.contains("$(") {
                        let captured = run_capture(inner.trim())?;
                        result.push_str(captured.trim_end_matches('\n'));
                        i = j + 1;
                        continue;
                    }
                }
                // Unmatched paren or nested: emit '$(' literally and continue.
                result.push('$');
                i += 1;
                continue;
            }
            if next == b'?' {
                // $? — exit code of the last command.
                if let Some(v) = get_var("?") {
                    result.push_str(&v);
                }
                i += 2;
                continue;
            }
            if next == b'#' {
                // $# — positional argument count.
                if let Some(v) = get_var("#") {
                    result.push_str(&v);
                }
                i += 2;
                continue;
            }
            if next == b'@' {
                // $@ — all positional arguments joined with spaces.
                if let Some(v) = get_var("@") {
                    result.push_str(&v);
                }
                i += 2;
                continue;
            }
            if next.is_ascii_digit() && next != b'0' {
                // $1..$9 — single-digit positional parameter.
                // Guarded by `next.is_ascii_digit()` above.
                let key = core::str::from_utf8(&bytes[i + 1..i + 2]).unwrap_or("");
                if let Some(v) = get_var(key) {
                    result.push_str(&v);
                }
                i += 2;
                continue;
            }
            if next.is_ascii_alphabetic() || next == b'_' {
                let start = i + 1;
                let end = bytes[start..]
                    .iter()
                    .take_while(|&&b| b.is_ascii_alphanumeric() || b == b'_')
                    .count()
                    + start;
                // `take_while` above accepted only ASCII alphanumeric / '_'.
                let name = core::str::from_utf8(&bytes[start..end]).unwrap_or("");
                if let Some(v) = get_var(name) {
                    result.push_str(&v);
                }
                // Unset variables expand to empty string (POSIX default).
                i = end;
                continue;
            }
        }
        result.push(bytes[i] as char); // shell tokens are ASCII
        i += 1;
    }
    Ok(result)
}

fn expand_word(word: &Word) -> Result<String, ()> {
    let mut expanded = String::new();
    for segment in &word.segments {
        match segment.quote {
            QuoteStyle::Single => expanded.push_str(&segment.text),
            QuoteStyle::None | QuoteStyle::Double => {
                expanded.push_str(&expand_token(&segment.text)?);
            }
        }
    }
    Ok(expanded)
}

/// Parse and execute `line`, capturing all `shell_print` output into a `Vec<u8>`.
///
/// Used by the `shell_test` feature harness to assert on command output without
/// requiring a real serial console.  The `CaptureGuard` pops the capture even if
/// the command panics or returns early.
#[cfg(feature = "shell_test")]
pub fn capture_line(line: &str, jobs: &mut Jobs) -> Vec<u8> {
    let guard = CaptureGuard::new();
    let ast = crate::parser::parse(line);
    execute(&ast, jobs);
    guard.finish()
}

/// Execute an `Ast` and return the last command's exit code.
///
/// `stdin_data` is the bytes available on stdin for the first command in a pipeline.
pub fn execute(ast: &Ast, jobs: &mut Jobs) -> i32 {
    match ast {
        Ast::Empty => 0,
        Ast::Simple(cmd) => exec_cmd(cmd, &[], jobs),
        Ast::Pipeline(cmds) => exec_pipeline(cmds, jobs),
        Ast::Background(cmd) => {
            // Cooperative background: the shell is a single-task executor with no
            // async spawn capability for built-ins. `cmd &` runs synchronously and
            // is marked Done before control returns. True async background would
            // require spawning the command as a separate Cell via SpawnCap — not
            // in scope for G1. `fg`/`bg` built-ins report this limitation.
            let name = cmd
                .argv
                .first()
                .map(|word| word.text.as_str())
                .unwrap_or("?");
            let jid = jobs.add(name);
            // Background job notification always goes to console, not the sink.
            ostd::io::print("[");
            ostd::io::print_usize(jid);
            ostd::io::println("] running");
            // Signal spawn_external to skip sys_wait so a long-running external
            // cell (httpd) does not park the shell forever. Built-ins ignore it.
            state::set_bg_spawn(true);
            exec_cmd(cmd, &[], jobs);
            state::set_bg_spawn(false);
            jobs.set_state(jid, JobState::Done);
            0
        }
        Ast::Sequence(sub) => {
            let mut last = 0;
            for s in sub {
                last = execute(s, jobs);
            }
            last
        }
        Ast::Case { expr, arms } => {
            let Ok(value) = expand_token(expr) else {
                return 1;
            };
            for (pattern, body) in arms {
                if case_matches(pattern, &value) {
                    execute(body, jobs);
                    break;
                }
            }
            0
        }
        Ast::FuncDef { name, body } => {
            define_function(name, body);
            0
        }
        Ast::And(left, right) => {
            let code = execute(left, jobs);
            if code == 0 {
                execute(right, jobs)
            } else {
                code
            }
        }
        Ast::Or(left, right) => {
            let code = execute(left, jobs);
            if code != 0 {
                execute(right, jobs)
            } else {
                code
            }
        }
        Ast::While { cond, body } => {
            loop {
                if execute(cond, jobs) != 0 {
                    break;
                }
                execute(body, jobs);
                match take_loop_signal() {
                    LoopSignal::Break => break,
                    LoopSignal::Continue => continue,
                    LoopSignal::None => {}
                }
            }
            0
        }
        Ast::For { var, words, body } => {
            'for_loop: for word in words {
                set_var(var, word);
                execute(body, jobs);
                match take_loop_signal() {
                    LoopSignal::Break => break 'for_loop,
                    LoopSignal::Continue => continue 'for_loop,
                    LoopSignal::None => {}
                }
            }
            0
        }
        Ast::If {
            cond,
            then_b,
            else_b,
        } => {
            let code = execute(cond, jobs);
            if code == 0 {
                execute(then_b, jobs)
            } else if let Some(eb) = else_b {
                execute(eb, jobs)
            } else {
                0
            }
        }
    }
}

/// Execute a pipeline: run each command in order, piping stdout→stdin.
///
/// Intermediate stages are captured into `Vec<u8>` buffers; the final stage
/// runs directly through the current sink so its exit code is preserved and
/// any outer capture (nested pipeline or `$(...)`) captures it correctly.
fn exec_pipeline(cmds: &[Cmd], jobs: &mut Jobs) -> i32 {
    if cmds.is_empty() {
        return 0;
    }
    let last_idx = cmds.len() - 1;
    let mut stdin_data: Vec<u8> = Vec::new();

    for (i, cmd) in cmds.iter().enumerate() {
        if i == last_idx {
            // Last stage: run directly (no intermediate capture).
            // Wire pipe stdin so built-ins without a file path read from it.
            state::set_stdin(&stdin_data);
            let code = exec_cmd(cmd, &stdin_data, jobs);
            state::clear_stdin();
            return code;
        }
        stdin_data = capture_cmd(cmd, &stdin_data, jobs);
    }
    0
}

/// Run a command and capture its output into a `Vec<u8>`.
///
/// Pushes a capture so that any built-in calling `shell_print` writes into it
/// instead of the serial console. The guard pops it on every exit path, which is
/// what makes an outer capture (a nested pipeline, or `$(...)`) resume correctly.
fn capture_cmd(cmd: &Cmd, stdin: &[u8], jobs: &mut Jobs) -> Vec<u8> {
    let guard = CaptureGuard::new();
    exec_cmd(cmd, stdin, jobs);
    guard.finish()
}

/// Execute one simple command.
///
/// Handles redirection, built-in dispatch, and external binary spawn.
fn exec_cmd(cmd: &Cmd, _stdin: &[u8], jobs: &mut Jobs) -> i32 {
    if cmd.is_empty() {
        return 0;
    }

    // Failed command substitution aborts before dispatch with a nonzero status.
    let expanded = match cmd
        .argv
        .iter()
        .map(expand_word)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(expanded) => expanded,
        Err(()) => {
            set_var("?", "1");
            return 1;
        }
    };
    let prog: &str = &expanded[0];
    let args: Vec<String> = expanded[1..].to_vec();

    // Detect `KEY=VALUE` assignment (key is non-empty alphanumeric+underscore).
    if args.is_empty() {
        if let Some(eq) = prog.find('=') {
            let key = &prog[..eq];
            if !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                set_var(key, &prog[eq + 1..]);
                return 0;
            }
        }
    }

    // echo with stdout redirect: fast path using cmd_echo_to_vec (no OutputSink needed).
    if prog == "echo" {
        if let Some(Redirect::StdoutTo(path)) = cmd
            .redirects
            .iter()
            .find(|r| matches!(r, Redirect::StdoutTo(_)))
        {
            let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            let bytes = crate::commands::cmd_echo_to_vec(&arg_refs);
            let Ok(path) = expand_word(path) else {
                set_var("?", "1");
                return 1;
            };
            if !crate::cmd_fs::write_file(&path, &bytes) {
                ostd::io::print("echo: cannot write '");
                ostd::io::print(&path);
                ostd::io::println("'");
                return 1;
            }
            return 0;
        }
        if let Some(Redirect::StdoutAppend(path)) = cmd
            .redirects
            .iter()
            .find(|r| matches!(r, Redirect::StdoutAppend(_)))
        {
            let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            let bytes = crate::commands::cmd_echo_to_vec(&arg_refs);
            let Ok(path) = expand_word(path) else {
                set_var("?", "1");
                return 1;
            };
            if !crate::cmd_fs::append_file(&path, &bytes) {
                ostd::io::print("echo: cannot append '");
                ostd::io::print(&path);
                ostd::io::println("'");
                return 1;
            }
            return 0;
        }
    }

    // StdinFrom redirect: preload the file into a buffer and expose it via
    // shell_stdin() so built-ins (grep, wc, …) can read from it.
    let stdin_file_buf: Vec<u8>;
    let effective_stdin: &[u8] = if let Some(Redirect::StdinFrom(path)) = cmd
        .redirects
        .iter()
        .find(|r| matches!(r, Redirect::StdinFrom(_)))
    {
        let Ok(path) = expand_word(path) else {
            set_var("?", "1");
            return 1;
        };
        stdin_file_buf = match crate::cmd_fs::read_file_vfs_owned(&path, 4096) {
            Ok(bytes) => bytes,
            Err(_) => {
                ostd::io::print("shell: cannot open '");
                ostd::io::print(&path);
                ostd::io::println("'");
                return 1;
            }
        };
        &stdin_file_buf
    } else {
        _stdin
    };

    // Detect stdout/stderr redirect for non-echo commands.
    //
    // ViCell has one output channel (serial console). `2>file` is therefore
    // semantically equivalent to `>file` — both capture `shell_print` output.
    // When both `>` and `2>` are present, `>` takes precedence; `2>` is a
    // documented no-op in that case (single-channel limitation).
    let stdout_redir = cmd
        .redirects
        .iter()
        .find_map(|redirect| match redirect {
            Redirect::StdoutTo(path) => Some((path, false)),
            Redirect::StdoutAppend(path) => Some((path, true)),
            _ => None,
        })
        .or_else(|| {
            cmd.redirects.iter().find_map(|redirect| match redirect {
                // Fallback: StderrTo reuses the stdout-capture path (one-channel shell).
                Redirect::StderrTo(path) => Some((path, false)),
                _ => None,
            })
        });
    let stdout_redir = match stdout_redir {
        Some((path, append)) => match expand_word(path) {
            Ok(path) => Some((path, append)),
            Err(()) => {
                set_var("?", "1");
                return 1;
            }
        },
        None => None,
    };

    // Wire the pipe-fed stdin so pipe-aware built-ins can read it.
    state::set_stdin(effective_stdin);

    let code = if let Some((path, append)) = stdout_redir {
        // Capture this command's output into a buffer, then write to VFS.
        let captured: Vec<u8>;
        let status;
        {
            let guard = CaptureGuard::new();
            status = dispatch_builtin(prog, &args, jobs);
            captured = guard.finish();
        } // capture popped here, before the VFS write
        let write_ok = crate::cmd_fs::vfs_write_chunked(&path, &captured, append);
        if status == 0 && !write_ok {
            1
        } else {
            status
        }
    } else {
        dispatch_builtin(prog, &args, jobs)
    };

    // Clear pipe stdin; leave the capture stack alone (exec_cmd does not own it).
    state::clear_stdin();

    set_var("?", i32_to_str(code));
    code
}

/// Match a case pattern against a value.
///
/// `*` is a catch-all; everything else is exact string equality.
fn case_matches(pattern: &str, value: &str) -> bool {
    pattern == "*" || pattern == value
}

/// Convert a small positional-arg index (1-9) to an owned `String` key.
///
/// Avoids `i32_to_str` which writes to a single shared static buffer —
/// calling it twice invalidates the first result while the second is alive.
fn usize_key(n: usize) -> String {
    let digit = b'0' + (n as u8).min(9);
    // SAFETY: `digit` is always a valid ASCII byte.
    String::from(digit as char) // `digit` is b'0'..=b'9', so this is ASCII
}

/// Convert a small non-negative integer to a &str backed by a fixed buffer.
///
/// Returns "0" for 0, the decimal string for 1-127, and "1" for anything else.
/// This avoids heap allocation for the `$?` variable.
fn i32_to_str(n: i32) -> &'static str {
    // Use a 'static lookup table for the most common exit codes (0–9).
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        6 => "6",
        7 => "7",
        8 => "8",
        9 => "9",
        127 => "127",
        _ => "1",
    }
}

/// Dispatch to the matching shell built-in.
///
/// Returns the exit code (0 = success, non-zero = error).
/// Falls through to `spawn_external` if no built-in matches.
fn dispatch_builtin(prog: &str, args: &[String], jobs: &mut Jobs) -> i32 {
    let legacy_result = match prog {
        "ls" => Some(with_legacy_parts(args, crate::commands::cmd_ls)),
        "cat" => Some(with_legacy_parts(args, crate::commands::cmd_cat)),
        "wc" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_wc)),
        "head" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_head)),
        "tail" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_tail)),
        "find" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_find)),
        "uniq" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_uniq)),
        "sort" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_sort)),
        "tee" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_tee)),
        "mkdir" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_mkdir)),
        "rmdir" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_rmdir)),
        "rm" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_rm)),
        "vcat" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_vcat)),
        "vwrite" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_vwrite)),
        "vappend" => Some(with_legacy_parts(args, crate::cmd_fs::cmd_vappend)),
        "kill" => Some(with_legacy_parts(args, crate::commands::cmd_kill)),
        "ps" => Some(with_legacy_parts(args, crate::commands::cmd_ps)),
        "cd" => Some(with_legacy_parts(args, crate::cmd_cwd::cmd_cd)),
        "pwd" => Some(with_legacy_parts(args, crate::cmd_sys::cmd_pwd)),
        "uname" => Some(with_legacy_parts(args, crate::cmd_sys::cmd_uname)),
        "free" => Some(with_legacy_parts(args, crate::cmd_sys::cmd_free)),
        "env" => Some(with_legacy_parts(args, crate::cmd_sys::cmd_env)),
        "uptime" => Some(with_legacy_parts(args, crate::cmd_sys::cmd_uptime)),
        "sleep" => Some(with_legacy_parts(args, crate::cmd_sys::cmd_sleep)),
        "blktest" => Some(with_legacy_parts(args, crate::cmd_sys::cmd_blkio_test)),
        "echo" => Some(with_legacy_parts(args, crate::commands::cmd_echo)),
        "exec" => Some(with_legacy_parts(args, crate::commands::cmd_exec)),
        _ => None,
    };
    if let Some(result) = legacy_result {
        return match result {
            Ok(()) => 0,
            Err(_) => 1,
        };
    }

    match prog {
        "top" => {
            let joined = args.join(" ");
            crate::commands::cmd_top(joined.split_whitespace())
                .map(|_| 0)
                .unwrap_or(1)
        }
        "grep" => cmd_grep_args(args),
        "sed" => cmd_sed_args(args),
        "awk" => cmd_awk_args(args),
        "snapshot" => crate::snapshot_client::run(),
        "shutdown" => crate::cmd_sys::cmd_shutdown().map(|_| 0).unwrap_or(1),
        "clear" => crate::commands::cmd_clear().map(|_| 0).unwrap_or(1),
        "help" => crate::commands::cmd_help().map(|_| 0).unwrap_or(1),
        "jobs" => {
            print_jobs(jobs);
            0
        }
        "fg" | "bg" => {
            shell_println(
                "fg/bg: no job control — background jobs run synchronously in this shell",
            );
            0
        }
        "source" | "." => cmd_source(args, jobs).map(|_| 0).unwrap_or(1),
        "test" => {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            cmd_test(&refs).map(|_| 0).unwrap_or(1)
        }
        "[" => {
            let refs: Vec<&str> = args
                .iter()
                .map(String::as_str)
                .filter(|arg| *arg != "]")
                .collect();
            cmd_test(&refs).map(|_| 0).unwrap_or(1)
        }
        "break" => {
            set_loop_signal(LoopSignal::Break);
            0
        }
        "continue" => {
            set_loop_signal(LoopSignal::Continue);
            0
        }
        "read" => {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            cmd_read(&refs).map(|_| 0).unwrap_or(1)
        }
        "exit" => {
            let code = args
                .first()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            request_exit(code);
            0
        }
        "unset" => {
            for name in args {
                unset_var(name);
            }
            0
        }
        _ => {
            if let Some(body) = get_function(prog) {
                let nargs = args.len().min(9);
                let mut saved: Vec<(String, Option<String>)> = Vec::with_capacity(nargs + 2);
                for i in 1..=nargs {
                    let key = usize_key(i);
                    saved.push((key.clone(), get_var(&key)));
                }
                saved.push((String::from("#"), get_var("#")));
                saved.push((String::from("@"), get_var("@")));
                for i in 1..=nargs {
                    set_var(&usize_key(i), &args[i - 1]);
                }
                set_var("#", i32_to_str(nargs as i32));
                set_var("@", &args.join(" "));
                let mut buf = [0u8; 480];
                let bb = body.as_bytes();
                let blen = bb.len().min(479);
                buf[..blen].copy_from_slice(&bb[..blen]);
                let result = if let Ok(s) = core::str::from_utf8(&buf[..blen]) {
                    let ast = crate::parser::parse(s);
                    execute(&ast, jobs)
                } else {
                    1
                };
                for (k, v) in &saved {
                    match v {
                        Some(old) => set_var(k, old),
                        None => unset_var(k),
                    }
                }
                result
            } else {
                spawn_external(prog, args)
            }
        }
    }
}

/// Print all active jobs.
fn print_jobs(jobs: &Jobs) {
    for (id, state, name) in jobs.list() {
        shell_print(&alloc::format!(
            "[{}] {}  {}\n",
            id,
            match state {
                JobState::Running => "Running",
                JobState::Done => "Done   ",
            },
            name
        ));
    }
}

/// `test` / `[` — evaluate a condition and return 0 (true) or 1 (false).
///
/// Supported forms:
/// - `-f path`   : path exists and is a regular file
/// - `-z str`    : string is empty
/// - `-n str`    : string is non-empty
/// - `a = b`     : string equality
/// - `a != b`    : string inequality
fn cmd_test(args: &[&str]) -> ViResult<()> {
    let ok = Ok(());
    let fail = Err(ViError::NotFound); // any non-Ok maps to exit code 1
    match args {
        ["-f", path] => {
            if matches!(crate::cmd_fs::stat_file_vfs(path), Some((_, false))) {
                ok
            } else {
                fail
            }
        }
        [s1, "-z"] | ["-z", s1] => {
            if s1.is_empty() {
                ok
            } else {
                fail
            }
        }
        [s1, "-n"] | ["-n", s1] => {
            if !s1.is_empty() {
                ok
            } else {
                fail
            }
        }
        _ => {
            // String comparison: `a = b` or `a != b`.
            // args may be ["a", "=", "b"] or ["a", "!=", "b"].
            if args.len() == 3 {
                let (a, op, b) = (args[0], args[1], args[2]);
                match op {
                    "=" | "==" => {
                        if a == b {
                            ok
                        } else {
                            fail
                        }
                    }
                    "!=" => {
                        if a != b {
                            ok
                        } else {
                            fail
                        }
                    }
                    _ => fail,
                }
            } else {
                fail
            }
        }
    }
}

/// `read [VAR]` — read one line from stdin (fd 0) into `$VAR` (default: `$REPLY`).
///
/// Blocks until a newline is received through the same focus-aware input-service
/// path as the interactive REPL.
fn cmd_read(args: &[&str]) -> ViResult<()> {
    let var = args.first().copied().unwrap_or("REPLY");
    let mut line = String::new();
    ostd::io::stdin().read_line(&mut line)?;
    let value = line.trim_end_matches(&['\r', '\n'][..]);
    set_var(var, value);
    Ok(())
}

/// `source <path>` — read a shell script from VFS and execute each line.
///
/// Lines starting with `#` and blank lines are skipped. The script runs in the
/// current shell's Jobs context, so spawns from the script are tracked normally.
/// Maximum script size is 4096 bytes (same limit as VFS OP_READ reply).
fn cmd_source(args: &[String], jobs: &mut Jobs) -> ViResult<()> {
    let path = match args.first() {
        Some(p) => p.as_str(),
        None => {
            ostd::io::println("Usage: source <path>");
            return Ok(());
        }
    };
    let bytes = crate::cmd_fs::read_file_vfs_owned(path, 4096).inspect_err(|_| {
        ostd::io::print("source: cannot open '");
        ostd::io::print(path);
        ostd::io::println("'");
    })?;
    let content = core::str::from_utf8(&bytes).unwrap_or("");
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let ast = crate::parser::parse(line);
        execute(&ast, jobs);
    }
    Ok(())
}

/// Attempt to spawn an external binary from `/bin/<prog>`.
///
/// Arguments are published via `sys_set_spawn_args` (a reserved state-stash
/// slot) for the spawned cell to read on startup — `sys_spawn_from_path` does
/// not yet carry argv on the new cell's stack. We always set the slot (empty
/// when there are no args) so the cell never reads a previous command's args.
fn spawn_external(prog: &str, args: &[String]) -> i32 {
    if !ostd::set_spawn_argv(args) {
        shell_println("shell: external argv exceeds 512-byte transport limit");
        return 1;
    }

    let mut path = alloc::string::String::from("/bin/");
    path.push_str(prog);
    match syscall::sys_spawn_from_path(&path) {
        syscall::SyscallResult::Ok(tid) => {
            // Backgrounded (`cmd &`): do NOT sys_wait. A long-running external
            // cell (httpd) would otherwise park the shell forever, so no later
            // command runs. The child keeps the shell's focus grant; that is
            // acceptable for a server that never reads the keyboard. Foreground
            // spawns fall through to the focus-handoff + sys_wait below.
            if state::bg_spawn() {
                return 0;
            }
            // Drain any pending input-service IPC events before ClearFocus.
            // Shell reads UART via sys_read(0), so Enter arrives via BOTH the ring
            // buffer (consumed in read_line) AND input-service EV_ASCII IPC. The IPC
            // path blocks input service in sys_send(shell, Enter_event) while shell
            // is not in sys_recv. drain_pending_input_events() uses sys_try_recv with
            // mask=input_tid (the correct G18 wildcard) to drain the queued message,
            // unblocking input service so release_focus does not deadlock.
            ostd::input::drain_pending_input_events();
            // Release keyboard focus before blocking in sys_wait. While the shell
            // is in sys_wait it is NOT in sys_recv, so the input service would
            // block indefinitely trying to dispatch key events to this cell and
            // eventually be killed by its own heartbeat watchdog (G18 deadlock).
            ostd::input::release_focus();
            // Foreground: block until the child exits so it owns the console
            // (stdin/UART). Without this the shell loops back to read the next
            // line and races interactive children (e.g. `hypha`) for keystrokes.
            // Fast commands return immediately (kernel Wait short-circuits when
            // the child is already Terminated). Background (`&`) already runs
            // synchronously in G1, so this does not regress it.
            let code = match syscall::sys_wait(tid) {
                syscall::SyscallResult::Ok(code) => code as i32,
                syscall::SyscallResult::Err(_) => 0,
            };
            // Re-acquire focus for the next interactive prompt.
            for _ in 0..10 {
                if ostd::input::request_focus() {
                    break;
                }
            }
            code
        }
        syscall::SyscallResult::Err(_) => {
            ostd::io::print("shell: command not found: ");
            ostd::io::println(prog);
            127
        }
    }
}

fn cmd_grep_args(args: &[String]) -> i32 {
    crate::text_tools::grep::run(args)
}

fn cmd_sed_args(args: &[String]) -> i32 {
    let mut cursor = ArgCursor::new(args);
    let mut suppress = false;
    let expr = loop {
        match cursor.next() {
            Some("-n") => suppress = true,
            Some(arg) => break String::from(arg),
            None => return UtilityStatus::Error.exit_code(),
        }
    };
    let path = cursor.next_owned();
    if cursor.next().is_some() {
        shell_println("sed: only one input file is supported");
        return UtilityStatus::Error.exit_code();
    }
    let text = match read_utility_text("sed", path.as_deref()) {
        Ok(text) => text,
        Err(code) => return code,
    };
    let output = match sed::execute(&expr, suppress, &text) {
        Ok(output) => output,
        Err(err) => {
            shell_print("sed: ");
            shell_println(err.message());
            return UtilityStatus::Error.exit_code();
        }
    };
    for line in output {
        shell_println(&line);
    }
    UtilityStatus::Selected.exit_code()
}

fn cmd_awk_args(args: &[String]) -> i32 {
    let mut cursor = ArgCursor::new(args);
    let mut separator = None;
    let program = loop {
        match cursor.next() {
            Some("-F") => separator = cursor.next_owned(),
            Some(arg) if arg.starts_with("-F") && arg.len() > 2 => {
                separator = Some(String::from(&arg[2..]));
            }
            Some(arg) => break String::from(arg),
            None => return UtilityStatus::Error.exit_code(),
        }
    };
    if !awk::looks_like_program(&program) {
        let joined = args.join(" ");
        return match crate::cmd_fs::cmd_awk(joined.split_whitespace()) {
            Ok(()) => UtilityStatus::Selected.exit_code(),
            Err(_) => UtilityStatus::Error.exit_code(),
        };
    }
    let path = cursor.next_owned();
    if cursor.next().is_some() {
        shell_println("awk: only one input file is supported");
        return UtilityStatus::Error.exit_code();
    }
    let text = match read_utility_text("awk", path.as_deref()) {
        Ok(text) => text,
        Err(code) => return code,
    };
    let separator = match awk::Separator::from_flag(separator.as_deref()) {
        Ok(separator) => separator,
        Err(err) => {
            shell_print("awk: ");
            shell_println(err.message());
            return UtilityStatus::Error.exit_code();
        }
    };
    let output = match awk::execute(&program, separator, &text) {
        Ok(output) => output,
        Err(err) => {
            shell_print("awk: ");
            shell_println(err.message());
            return UtilityStatus::Error.exit_code();
        }
    };
    for line in output {
        shell_println(&line);
    }
    UtilityStatus::Selected.exit_code()
}

fn read_utility_text(name: &str, path: Option<&str>) -> Result<String, i32> {
    let bytes = if let Some(path) = path {
        match read_path_bytes(path) {
            Ok(bytes) => bytes,
            Err(UtilityReadError::Io) => {
                ostd::io::print(name);
                ostd::io::print(": cannot open '");
                ostd::io::print(path);
                ostd::io::println("'");
                return Err(UtilityStatus::Error.exit_code());
            }
            Err(UtilityReadError::InputTooLarge) => {
                shell_print(name);
                shell_println(": input exceeds 65536-byte limit");
                return Err(UtilityStatus::Error.exit_code());
            }
            Err(UtilityReadError::AllocationFailed) => {
                shell_print(name);
                shell_println(": input allocation failed");
                return Err(UtilityStatus::Error.exit_code());
            }
        }
    } else {
        let stdin = shell_stdin();
        let mut bytes = Vec::new();
        if let Err(err) = extend_input(&mut bytes, &stdin) {
            shell_print(name);
            shell_println(match err {
                InputBufferError::TooLarge => ": input exceeds 65536-byte limit",
                InputBufferError::AllocationFailed => ": input allocation failed",
            });
            return Err(UtilityStatus::Error.exit_code());
        }
        bytes
    };
    core::str::from_utf8(&bytes).map(String::from).map_err(|_| {
        shell_print(name);
        shell_println(": input is not valid UTF-8");
        UtilityStatus::Error.exit_code()
    })
}

enum UtilityReadError {
    Io,
    InputTooLarge,
    AllocationFailed,
}

fn read_path_bytes(path: &str) -> Result<Vec<u8>, UtilityReadError> {
    if let Some((size, is_dir)) = crate::cmd_fs::stat_file_vfs(path) {
        if is_dir {
            return Err(UtilityReadError::Io);
        }
        if size > MAX_INPUT_BYTES {
            return Err(UtilityReadError::InputTooLarge);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| UtilityReadError::AllocationFailed)?;
        bytes.resize(size, 0);
        if size == 0 {
            return Ok(bytes);
        }
        if crate::cmd_fs::read_file_vfs_known_size(path, size, &mut bytes) == Ok(size) {
            return Ok(bytes);
        }
    }

    let fd = syscall::sys_open(path).map_err(|_| UtilityReadError::Io)?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        match syscall::sys_read(fd, &mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(err) = extend_input(&mut bytes, &chunk[..n]) {
                    syscall::sys_close(fd);
                    return Err(match err {
                        InputBufferError::TooLarge => UtilityReadError::InputTooLarge,
                        InputBufferError::AllocationFailed => UtilityReadError::AllocationFailed,
                    });
                }
            }
            Err(_) => {
                syscall::sys_close(fd);
                return Err(UtilityReadError::Io);
            }
        }
    }
    syscall::sys_close(fd);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{expand_word, set_var};
    use crate::parser::{parse, Ast};

    fn expanded_arg(line: &str) -> alloc::string::String {
        match parse(line) {
            Ast::Simple(cmd) => expand_word(&cmd.argv[1]).expect("expansion succeeds"),
            _ => panic!("expected Simple"),
        }
    }

    #[test]
    fn mixed_quote_expansion_is_segment_local() {
        set_var("HOME", "/home/test");
        assert_eq!(expanded_arg("echo pre'$HOME'"), "pre$HOME");
        assert_eq!(expanded_arg("echo '$HOME'suffix"), "$HOMEsuffix");
        assert_eq!(expanded_arg("echo $HOME\"/bin\""), "/home/test/bin");
        assert_eq!(expanded_arg("echo \"\""), "");
    }
}
