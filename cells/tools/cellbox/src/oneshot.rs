//! `/bin/cellbox <command>`: one catalog command without an interactive shell (Phase 10
//! S10-0, CP1 2026-10-26, DS0-1..DS0-6).
//!
//! The launch context's argv (which never carries the program path) selects the entry
//! once, in `main`, through [`classify`]:
//!
//! - `ssh …` is the ssh client child (checked first, unchanged);
//! - a context with a terminal frontend runs `cellbox sh` for `[]`/`["sh"]` and refuses
//!   every other argv (2);
//! - without a frontend, `sh` still needs a terminal (125), an empty or option-like argv
//!   is a usage error (2, `-h`/`--help` print the usage with 0), a name in the closed
//!   [`TABLE`] runs or is refused by its class (2), and anything else, including an
//!   absolute path or a retained worker name, is an unknown command (127).
//!
//! The table covers every `executor::BUILTINS` name plus `[`, `.` and the six typed stage
//! words exactly once. Only [`Class::Run`] names execute, through the executor's closed
//! `dispatch_catalog`: a one-shot never launches a child (no `spawn_external`, no `exec`,
//! no `hv`, no ssh launch), so its authority is a strict subset of `cellbox sh`, whose
//! ELF, path and launch edges it shares.
//!
//! argv is used verbatim: no parsing, expansion, globbing, redirection or `|`. Output
//! goes to the context's Stdout/Stderr grants, captured before anything is written; a
//! context without both writes nothing anywhere and exits 1. Stdin is read only by the
//! stdin-consuming rows, on the command's own first demand (so only when it got no file
//! operand), to EOF and bounded by `typed::MAX_SINK_BYTES`.
//!
//! Inside `cellbox sh`, a simple command whose first word is `cellbox` or `/bin/cellbox`
//! goes through the same table in process ([`prefixed`]): a Run name (and `ssh`, the
//! shell's own builtin) drops the prefix; every other answer is the one-shot's refusal.

extern crate alloc;

use alloc::format;
use alloc::string::String;

/// How one table name behaves as `/bin/cellbox NAME`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Runs with the shell's own implementation and status.
    Run,
    /// Changes or reads shell state: refused (2), no effect.
    State,
    /// Needs an interactive terminal (`clear`): refused (2).
    Terminal,
    /// Launches a Cell (`exec`, `hv`): refused (2).
    Launch,
    /// `ssh`: the existing child entry, never dispatched as a one-shot.
    Reserved,
    /// A typed pipeline stage: refused (2).
    Stage,
}

/// The closed one-shot table: every `BUILTINS` name, `[`, `.` and the six stage words,
/// each exactly once (62 + 6).
pub const TABLE: &[(&str, Class)] = &[
    ("awk", Class::Run),
    ("blktest", Class::Run),
    ("cat", Class::Run),
    ("cp", Class::Run),
    ("date", Class::Run),
    ("echo", Class::Run),
    ("edit", Class::Run),
    ("env", Class::Run),
    ("find", Class::Run),
    ("free", Class::Run),
    ("grep", Class::Run),
    ("head", Class::Run),
    ("help", Class::Run),
    ("ifconfig", Class::Run),
    ("ip", Class::Run),
    ("kill", Class::Run),
    ("ls", Class::Run),
    ("mkdir", Class::Run),
    ("mv", Class::Run),
    ("ps", Class::Run),
    ("pwd", Class::Run),
    ("rg", Class::Run),
    ("rm", Class::Run),
    ("rmdir", Class::Run),
    ("sed", Class::Run),
    ("shutdown", Class::Run),
    ("sleep", Class::Run),
    ("snapshot", Class::Run),
    ("sort", Class::Run),
    ("tail", Class::Run),
    ("tee", Class::Run),
    ("test", Class::Run),
    ("[", Class::Run),
    ("top", Class::Run),
    ("touch", Class::Run),
    ("uname", Class::Run),
    ("uniq", Class::Run),
    ("uptime", Class::Run),
    ("vappend", Class::Run),
    ("vcat", Class::Run),
    ("vwrite", Class::Run),
    ("wc", Class::Run),
    ("alias", Class::State),
    ("unalias", Class::State),
    ("cd", Class::State),
    ("export", Class::State),
    ("unset", Class::State),
    ("exit", Class::State),
    ("source", Class::State),
    (".", Class::State),
    ("read", Class::State),
    ("history", Class::State),
    ("jobs", Class::State),
    ("fg", Class::State),
    ("bg", Class::State),
    ("break", Class::State),
    ("continue", Class::State),
    ("completion", Class::State),
    ("clear", Class::Terminal),
    ("exec", Class::Launch),
    ("hv", Class::Launch),
    ("ssh", Class::Reserved),
    ("select", Class::Stage),
    ("where", Class::Stage),
    ("sort-by", Class::Stage),
    ("take", Class::Stage),
    ("to", Class::Stage),
    ("from", Class::Stage),
];

/// The class of `name`, or `None` for a name outside the table.
pub fn class_of(name: &str) -> Option<Class> {
    TABLE.iter().find(|(entry, _)| *entry == name).map(|(_, class)| *class)
}

/// Run rows that read stdin when the command itself asks for it (it got no file operand).
#[cfg(any(test, not(feature = "shell_test")))]
pub const STDIN_CONSUMERS: &[&str] =
    &["cat", "grep", "wc", "head", "tail", "sort", "uniq", "tee", "sed", "awk", "rg"];

/// True when `name`'s row may read the Stdin grant.
#[cfg(any(test, not(feature = "shell_test")))]
pub fn consumes_stdin(name: &str) -> bool {
    STDIN_CONSUMERS.contains(&name)
}

/// Status of every S10-0.3/0.4 refusal (catalog N: unsupported operand, DS0-3).
pub const REFUSED_STATUS: i32 = 2;
/// Status of a name outside the table (catalog N).
pub const UNKNOWN_STATUS: i32 = 127;
/// Status of `sh` without a terminal frontend (the shell's launch refusal).
#[cfg(any(test, not(feature = "shell_test")))]
pub const NEEDS_TERMINAL_STATUS: i32 = 125;
/// Status after an output failure or a refused stdin (DS0-4).
#[cfg(any(test, not(feature = "shell_test")))]
pub const OUTPUT_FAILED_STATUS: i32 = 1;

/// Usage text: stdout for `-h`/`--help` (0), stderr otherwise (2).
pub const USAGE: &str = "usage: cellbox COMMAND [ARGUMENT...]\n       cellbox sh\n\
COMMAND runs once, without a shell: its arguments are taken literally (no\n\
expansion, globbing, redirection or pipelines). 'cellbox sh' is the interactive\n\
shell of a terminal frontend; 'help' lists the commands.\n";

/// A terminal frontend launched `/bin/cellbox` with an argv other than `sh`.
#[cfg(any(test, not(feature = "shell_test")))]
pub const FRONTEND_ONLY_SH: &str = "cellbox: a terminal frontend runs only 'sh'\n";

/// `sh` without a terminal frontend.
#[cfg(any(test, not(feature = "shell_test")))]
pub const NEEDS_TERMINAL: &str = "cellbox: sh needs a terminal frontend\n";

/// `cellbox sh` typed inside the shell.
pub const SHELL_DOES_NOT_NEST: &str = "cellbox: sh: already in 'cellbox sh'; the shell does not nest\n";

/// The launch decision for one context argv (S10-0.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// `ssh …`: the ssh client child entry (checked first, unchanged).
    Ssh,
    /// A terminal frontend with `[]` or `["sh"]`: the frontend shell (unchanged).
    Shell,
    /// A terminal frontend with any other argv: 2 on the holder's stderr writer.
    FrontendArgv,
    /// `["sh", …]` without a frontend: 125, the shell needs a terminal.
    NeedsTerminal,
    /// `-h`/`--help`: usage on stdout, 0.
    Help,
    /// `[]` or another option-like argv[0]: usage on stderr, 2.
    Usage,
    /// A Run name: execute it.
    Run,
    /// A State/Terminal/Launch/Stage name: one line on stderr, 2, no effect.
    Refused(Class),
    /// Any other argv[0]: 127, no external fallback.
    Unknown,
}

/// Classify a launch context's argv (`frontend`: the context carries an input frontend).
pub fn classify<S: AsRef<str>>(frontend: bool, argv: &[S]) -> Launch {
    let first = argv.first().map(AsRef::as_ref);
    if first == Some(crate::ssh::SSH_ENTRY) {
        return Launch::Ssh;
    }
    if frontend {
        return match (first, argv.len()) {
            (None, _) | (Some("sh"), 1) => Launch::Shell,
            _ => Launch::FrontendArgv,
        };
    }
    let Some(name) = first else {
        return Launch::Usage;
    };
    match name {
        "sh" => Launch::NeedsTerminal,
        "-h" | "--help" => Launch::Help,
        _ if name.starts_with('-') => Launch::Usage,
        _ => match class_of(name) {
            Some(Class::Run) => Launch::Run,
            // Unreachable here (`ssh` is the child entry above); kept total.
            Some(Class::Reserved) => Launch::Ssh,
            Some(class) => Launch::Refused(class),
            None => Launch::Unknown,
        },
    }
}

/// The one stderr line of a refused table name.
pub fn refusal_message(class: Class, name: &str) -> String {
    match class {
        Class::State => format!("cellbox: {name} changes or reads shell state; run it inside 'cellbox sh'\n"),
        Class::Terminal => format!("cellbox: {name} needs an interactive terminal\n"),
        Class::Launch => format!("cellbox: {name} launches a Cell; run it inside 'cellbox sh'\n"),
        Class::Stage => format!("cellbox: {name} is a pipeline stage; use 'cellbox sh'\n"),
        Class::Run | Class::Reserved => format!("cellbox: {name}: not refused\n"),
    }
}

/// The one stderr line of a name outside the table.
pub fn unknown_message(name: &str) -> String {
    format!("cellbox: {name}: unknown command\n")
}

/// True for the words that name this program inside the shell.
pub fn is_prefix(word: &str) -> bool {
    word == "cellbox" || word == "/bin/cellbox"
}

/// What the shell does with an expanded simple command (S10-0.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prefixed {
    /// The first word is not `cellbox`/`/bin/cellbox`: unchanged.
    No,
    /// Drop the first word: `cellbox ls /` is exactly `ls /`.
    Strip,
    /// Answer without running anything: `text` on stdout (`stdout`) or stderr, `status`.
    Answer { status: i32, stdout: bool, text: String },
}

/// The in-shell explicit form: the same table as the one-shot, in process, no spawn.
pub fn prefixed<S: AsRef<str>>(argv: &[S]) -> Prefixed {
    match argv.first() {
        Some(first) if is_prefix(first.as_ref()) => {}
        _ => return Prefixed::No,
    }
    let rest = &argv[1..];
    let name = rest.first().map(AsRef::as_ref).unwrap_or("");
    let refuse = |text: String| Prefixed::Answer { status: REFUSED_STATUS, stdout: false, text };
    match classify(false, rest) {
        // The shell's `ssh` builtin is the explicit form of the ssh client.
        Launch::Run | Launch::Ssh => Prefixed::Strip,
        Launch::Help => Prefixed::Answer { status: 0, stdout: true, text: String::from(USAGE) },
        Launch::Usage => refuse(String::from(USAGE)),
        Launch::NeedsTerminal => refuse(String::from(SHELL_DOES_NOT_NEST)),
        Launch::Refused(class) => refuse(refusal_message(class, name)),
        Launch::Unknown => Prefixed::Answer { status: UNKNOWN_STATUS, stdout: false, text: unknown_message(name) },
        // `classify(false, …)` never answers with a frontend decision.
        Launch::Shell | Launch::FrontendArgv => refuse(String::from(USAGE)),
    }
}

/// Why reading stdin stopped before end of stream.
#[cfg(any(test, not(feature = "shell_test")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdinStop {
    /// More than the bound arrived.
    TooLarge,
    /// The arena could not hold the next chunk.
    Allocation,
    /// The stream failed (not an end of stream).
    Read(api::cell_session::Status),
}

/// Read `read` to end of stream, keeping at most `bound` bytes. `read` returns the
/// number of bytes it placed in the buffer, or `Err(Status::Eof)` at the end.
#[cfg(any(test, not(feature = "shell_test")))]
pub fn read_bounded(
    mut read: impl FnMut(&mut [u8]) -> Result<usize, api::cell_session::Status>,
    bound: usize,
) -> Result<alloc::vec::Vec<u8>, StdinStop> {
    use api::cell_session::Status;
    let mut bytes = alloc::vec::Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        let count = match read(&mut chunk) {
            Ok(0) | Err(Status::Eof) => return Ok(bytes),
            Ok(count) => count.min(chunk.len()),
            Err(status) => return Err(StdinStop::Read(status)),
        };
        if bytes.len() + count > bound {
            return Err(StdinStop::TooLarge);
        }
        bytes.try_reserve(count).map_err(|_| StdinStop::Allocation)?;
        bytes.extend_from_slice(&chunk[..count]);
    }
}

/// The process status after a command ended with `status`, and whether the output
/// failure still needs its one stderr line. A failed stdout makes the one-shot end with
/// 1 (DS0-4); a command that already failed has reported its own diagnostic.
#[cfg(any(test, not(feature = "shell_test")))]
pub fn final_status(status: i32, stdout_failure: Option<api::cell_session::Status>) -> (i32, bool) {
    match stdout_failure {
        Some(_) => (OUTPUT_FAILED_STATUS, status == 0),
        None => (status, false),
    }
}

#[cfg(all(not(test), not(feature = "shell_test")))]
pub use runtime::run;

#[cfg(all(not(test), not(feature = "shell_test")))]
mod runtime {
    use super::*;
    use crate::shell_state as state;
    use alloc::vec::Vec;
    use terminal_sdk::context::{ChildContext, GrantSlot};
    use terminal_sdk::stream::{Endpoint, EndpointRef, OperationRef};

    /// The Stdin grant of this one-shot, read on the command's first demand.
    static STDIN: ostd::sync::Mutex<Option<Endpoint>> = ostd::sync::Mutex::new(None);

    fn stdout(text: &str) {
        let _ = state::oneshot_stdout(text.as_bytes());
    }

    fn stderr(text: &str) {
        state::oneshot_stderr(text.as_bytes());
    }

    /// Run a classified non-shell launch and return the process status. Nothing is
    /// written before both output grants are captured.
    pub fn run(context: &ChildContext, launch: Launch, argv: &[String]) -> i32 {
        let Ok(output) = ostd::io::ProcessOutput::read_own() else {
            return OUTPUT_FAILED_STATUS;
        };
        state::install_oneshot_output(output);
        let name = argv.first().map(String::as_str).unwrap_or("");
        let status = match launch {
            Launch::FrontendArgv => {
                stderr(FRONTEND_ONLY_SH);
                REFUSED_STATUS
            }
            Launch::NeedsTerminal => {
                stderr(NEEDS_TERMINAL);
                NEEDS_TERMINAL_STATUS
            }
            Launch::Help => {
                stdout(USAGE);
                0
            }
            Launch::Usage => {
                stderr(USAGE);
                REFUSED_STATUS
            }
            Launch::Refused(class) => {
                stderr(&refusal_message(class, name));
                REFUSED_STATUS
            }
            Launch::Unknown => {
                stderr(&unknown_message(name));
                UNKNOWN_STATUS
            }
            Launch::Run => run_command(context, argv),
            // `main` handles these before a one-shot exists.
            Launch::Ssh | Launch::Shell => REFUSED_STATUS,
        };
        let (status, report) = final_status(status, state::oneshot_stdout_failure());
        if report {
            if let Some(failure) = state::oneshot_stdout_failure() {
                stderr(&format!("cellbox: {name}: stdout failed: {failure:?}\n"));
            }
        }
        status
    }

    fn run_command(context: &ChildContext, argv: &[String]) -> i32 {
        // As for frontend shells: a cwd-less launch works from the root. A failure leaves
        // the command usable with absolute paths.
        let _ = crate::context_session::ensure_default_cwd();
        if consumes_stdin(&argv[0]) {
            if let Some(endpoint) = granted_stdin(context) {
                *STDIN.lock() = Some(endpoint);
                state::defer_stdin(load_stdin);
            }
        }
        crate::executor::run_catalog_command(argv)
    }

    fn granted_stdin(context: &ChildContext) -> Option<Endpoint> {
        let operation = OperationRef::from_raw(context.operation()).ok()?;
        let grant = context.grants().find(|grant| grant.slot == GrantSlot::Stdin && grant.kind == 1)?;
        Some(Endpoint::new(EndpointRef::from_raw(grant.object).ok()?, operation))
    }

    /// The command asked for stdin: read the grant to end of stream, bounded. A refusal
    /// ends the one-shot with 1 before the command sees any of it.
    fn load_stdin() -> Vec<u8> {
        let Some(endpoint) = STDIN.lock().take() else {
            return Vec::new();
        };
        let read = read_bounded(|chunk| endpoint.read(chunk, u64::MAX), crate::typed::MAX_SINK_BYTES);
        let _ = endpoint.close();
        match read {
            Ok(bytes) => bytes,
            Err(stop) => {
                match stop {
                    StdinStop::TooLarge => {
                        stderr(&format!("cellbox: stdin exceeds {} bytes\n", crate::typed::MAX_SINK_BYTES))
                    }
                    StdinStop::Allocation => stderr("cellbox: stdin: allocation failed\n"),
                    StdinStop::Read(status) => stderr(&format!("cellbox: stdin: {status:?}\n")),
                }
                ostd::syscall::sys_exit(OUTPUT_FAILED_STATUS as usize)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{Surface, LEDGER};
    use alloc::collections::BTreeSet;
    use alloc::vec::Vec;
    use api::cell_session::Status;

    const STAGES: [&str; 6] = ["select", "where", "sort-by", "take", "to", "from"];

    #[test]
    fn the_table_covers_builtins_aliases_and_stages_exactly_once() {
        let names: Vec<&str> = TABLE.iter().map(|(name, _)| *name).collect();
        let set: BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), set.len(), "a table name appears twice");
        let mut expected: BTreeSet<&str> = crate::executor::BUILTINS.iter().copied().collect();
        expected.extend(["[", "."]);
        expected.extend(STAGES);
        assert_eq!(set, expected);
        assert_eq!(TABLE.len(), 68);
        let count = |class: Class| TABLE.iter().filter(|(_, c)| *c == class).count();
        assert_eq!(
            [count(Class::Run), count(Class::State), count(Class::Terminal), count(Class::Launch), count(Class::Reserved), count(Class::Stage)],
            [42, 16, 1, 2, 1, 6]
        );
        for stage in STAGES {
            assert_eq!(class_of(stage), Some(Class::Stage));
        }
    }

    #[test]
    fn the_table_builtins_equal_the_ledger_builtin_rows() {
        let ledger: BTreeSet<&str> =
            LEDGER.iter().filter(|entry| entry.surface == Surface::Builtin).map(|entry| entry.name).collect();
        let table: BTreeSet<&str> = TABLE
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| !["[", "."].contains(name) && !STAGES.contains(name))
            .collect();
        assert_eq!(table, ledger);
    }

    #[test]
    fn retained_worker_paths_and_unknown_names_are_127_without_fallback() {
        for entry in LEDGER.iter().filter(|entry| matches!(entry.surface, Surface::SysTool | Surface::NetTool)) {
            let path = format!("/bin/{}", entry.name);
            assert_eq!(classify(false, &[path.as_str()]), Launch::Unknown, "{path}");
            assert_eq!(classify(false, &[path.as_str(), "x"]), Launch::Unknown, "{path}");
        }
        for name in ["nope", "hotswap", "ping", "curl", "nc", "wget", "httpd", "mqtt", "/bin/cellbox", "cellbox", "KEY=VALUE", "", " ls", "LS"] {
            assert_eq!(classify(false, &[name]), Launch::Unknown, "{name:?}");
        }
        assert_eq!(unknown_message("/bin/ls"), "cellbox: /bin/ls: unknown command\n");
    }

    #[test]
    fn the_launch_classification_table_row_by_row() {
        let empty: [&str; 0] = [];
        // ssh first, with or without a frontend.
        assert_eq!(classify(true, &["ssh", "host"]), Launch::Ssh);
        assert_eq!(classify(false, &["ssh"]), Launch::Ssh);
        // Frontend: only [] or ["sh"].
        assert_eq!(classify(true, &empty), Launch::Shell);
        assert_eq!(classify(true, &["sh"]), Launch::Shell);
        assert_eq!(classify(true, &["sh", "-c"]), Launch::FrontendArgv);
        assert_eq!(classify(true, &["ls"]), Launch::FrontendArgv);
        assert_eq!(classify(true, &["--help"]), Launch::FrontendArgv);
        // No frontend.
        assert_eq!(classify(false, &["sh"]), Launch::NeedsTerminal);
        assert_eq!(classify(false, &["sh", "x"]), Launch::NeedsTerminal);
        assert_eq!(classify(false, &empty), Launch::Usage);
        assert_eq!(classify(false, &["-x"]), Launch::Usage);
        assert_eq!(classify(false, &["-"]), Launch::Usage);
        assert_eq!(classify(false, &["--records"]), Launch::Usage);
        assert_eq!(classify(false, &["-h"]), Launch::Help);
        assert_eq!(classify(false, &["--help"]), Launch::Help);
        for (name, class) in TABLE {
            let expected = match class {
                Class::Run => Launch::Run,
                Class::Reserved => Launch::Ssh,
                other => Launch::Refused(*other),
            };
            assert_eq!(classify(false, &[*name]), expected, "{name}");
        }
        assert_eq!(classify(false, &["nope"]), Launch::Unknown);
    }

    #[test]
    fn refusals_name_the_command_and_their_class() {
        assert_eq!(
            refusal_message(Class::State, "cd"),
            "cellbox: cd changes or reads shell state; run it inside 'cellbox sh'\n"
        );
        assert_eq!(refusal_message(Class::Terminal, "clear"), "cellbox: clear needs an interactive terminal\n");
        assert_eq!(refusal_message(Class::Launch, "exec"), "cellbox: exec launches a Cell; run it inside 'cellbox sh'\n");
        assert_eq!(refusal_message(Class::Stage, "select"), "cellbox: select is a pipeline stage; use 'cellbox sh'\n");
        for text in [USAGE, FRONTEND_ONLY_SH, NEEDS_TERMINAL, SHELL_DOES_NOT_NEST] {
            assert!(!text.as_bytes().contains(&0x1b), "no ESC in a one-shot message");
            assert!(text.ends_with('\n'));
        }
    }

    #[test]
    fn argv_is_classified_by_its_first_word_only_and_kept_literal() {
        let argv = ["echo", "", "a b", "|", ";", "$HOME", "*", ">", "&&"];
        assert_eq!(classify(false, &argv), Launch::Run);
        // Words that would be grammar in the shell never select another command.
        for word in ["|", ";", "$HOME", "*", ">", "&&", "a b", ""] {
            assert_eq!(classify(false, &[word]), Launch::Unknown, "{word:?}");
        }
    }

    #[test]
    fn stdin_is_read_only_by_consumer_rows() {
        let consumers: BTreeSet<&str> = TABLE
            .iter()
            .filter(|(name, _)| consumes_stdin(name))
            .map(|(name, _)| *name)
            .collect();
        let expected: BTreeSet<&str> =
            ["cat", "grep", "wc", "head", "tail", "sort", "uniq", "tee", "sed", "awk", "rg"].into_iter().collect();
        assert_eq!(consumers, expected);
        for name in &expected {
            assert_eq!(class_of(name), Some(Class::Run), "{name}");
        }
        for name in ["ls", "echo", "ps", "rm", "top", "sleep", "read", "exec", "/bin/cat"] {
            assert!(!consumes_stdin(name), "{name}");
        }
    }

    /// A reader over `data` in `step`-byte reads, then end of stream.
    fn reader(data: Vec<u8>, step: usize) -> impl FnMut(&mut [u8]) -> Result<usize, Status> {
        let mut at = 0usize;
        move |out: &mut [u8]| {
            if at == data.len() {
                return Err(Status::Eof);
            }
            let n = step.min(out.len()).min(data.len() - at);
            out[..n].copy_from_slice(&data[at..at + n]);
            at += n;
            Ok(n)
        }
    }

    #[test]
    fn stdin_is_read_to_end_of_stream_byte_exact() {
        let data: Vec<u8> = [0u8, 0xff, 0x80, b'a', 0, 0xfe].iter().copied().cycle().take(5000).collect();
        assert_eq!(read_bounded(reader(data.clone(), 137), 1 << 20), Ok(data));
        assert_eq!(read_bounded(reader(Vec::new(), 512), 16), Ok(Vec::new()));
    }

    #[test]
    fn stdin_over_the_bound_is_refused_and_errors_are_not_end_of_stream() {
        let bound = 1000;
        assert_eq!(read_bounded(reader(alloc::vec![7u8; bound], 300), bound).map(|b| b.len()), Ok(bound));
        assert_eq!(read_bounded(reader(alloc::vec![7u8; bound + 1], 300), bound), Err(StdinStop::TooLarge));
        let mut calls = 0;
        let failing = |_: &mut [u8]| {
            calls += 1;
            if calls == 1 { Ok(10) } else { Err(Status::Revoked) }
        };
        assert_eq!(read_bounded(failing, bound), Err(StdinStop::Read(Status::Revoked)));
        assert_eq!(crate::typed::MAX_SINK_BYTES, 4 * 1024 * 1024);
    }

    #[test]
    fn status_mapping_output_failure_refusals_and_unknown() {
        assert_eq!(final_status(0, None), (0, false));
        assert_eq!(final_status(1, None), (1, false));
        assert_eq!(final_status(2, None), (2, false));
        // A failed stdout ends the one-shot with 1; only an otherwise successful
        // command still needs the one stderr line.
        assert_eq!(final_status(0, Some(Status::BrokenPipe)), (OUTPUT_FAILED_STATUS, true));
        assert_eq!(final_status(0, Some(Status::TimedOut)), (1, true));
        assert_eq!(final_status(1, Some(Status::Revoked)), (1, false));
        assert_eq!((REFUSED_STATUS, UNKNOWN_STATUS, NEEDS_TERMINAL_STATUS), (2, 127, 125));
    }

    fn words(text: &[&str]) -> Vec<String> {
        text.iter().map(|word| String::from(*word)).collect()
    }

    #[test]
    fn the_in_shell_prefix_strips_run_names_and_refuses_the_rest() {
        assert_eq!(prefixed(&words(&["ls", "/"])), Prefixed::No);
        assert_eq!(prefixed(&words(&["/bin/ls", "/"])), Prefixed::No);
        assert_eq!(prefixed(&words(&["cellbox", "ls", "/"])), Prefixed::Strip);
        assert_eq!(prefixed(&words(&["/bin/cellbox", "ls", "/p04"])), Prefixed::Strip);
        assert_eq!(prefixed(&words(&["cellbox", "ps", "--records"])), Prefixed::Strip);
        assert_eq!(prefixed(&words(&["cellbox", "ssh", "host"])), Prefixed::Strip);
        let refused = |argv: &[&str]| match prefixed(&words(argv)) {
            Prefixed::Answer { status, stdout: false, text } => (status, text),
            other => panic!("{argv:?}: {other:?}"),
        };
        assert_eq!(refused(&["/bin/cellbox", "cd", "/"]), (2, refusal_message(Class::State, "cd")));
        assert_eq!(refused(&["cellbox"]), (2, String::from(USAGE)));
        assert_eq!(refused(&["cellbox", "sh"]), (2, String::from(SHELL_DOES_NOT_NEST)));
        assert_eq!(refused(&["cellbox", "exec", "/bin/echo", "x"]), (2, refusal_message(Class::Launch, "exec")));
        assert_eq!(refused(&["cellbox", "select", "tid"]), (2, refusal_message(Class::Stage, "select")));
        assert_eq!(refused(&["cellbox", "clear"]), (2, refusal_message(Class::Terminal, "clear")));
        assert_eq!(refused(&["cellbox", "/bin/ls"]), (127, unknown_message("/bin/ls")));
        assert_eq!(refused(&["cellbox", "-x"]), (2, String::from(USAGE)));
        assert_eq!(
            prefixed(&words(&["cellbox", "--help"])),
            Prefixed::Answer { status: 0, stdout: true, text: String::from(USAGE) }
        );
    }

    /// The catalog's one-shot table, exactly as `command-catalog.md` must carry it.
    fn catalog_block() -> String {
        let mut out = String::from("<!-- oneshot:begin -->\n| Name | `/bin/cellbox NAME` |\n|---|---|\n");
        for (name, class) in TABLE {
            let label = match class {
                Class::Run if consumes_stdin(name) => "Run (reads stdin)",
                Class::Run => "Run",
                Class::State => "Refused 2: shell state",
                Class::Terminal => "Refused 2: needs a terminal",
                Class::Launch => "Refused 2: launches a Cell",
                Class::Reserved => "ssh client child entry",
                Class::Stage => "Refused 2: pipeline stage",
            };
            out.push_str(&format!("| `{name}` | {label} |\n"));
        }
        out.push_str("<!-- oneshot:end -->");
        out
    }

    const CATALOG: &str = include_str!("../../../../.agents/261020-1750-cellbox-cli/command-catalog.md");

    #[test]
    fn catalog_one_shot_table_is_generated_from_the_table() {
        let begin = CATALOG.find("<!-- oneshot:begin -->").expect("catalog one-shot begin marker");
        let end = CATALOG.find("<!-- oneshot:end -->").expect("catalog one-shot end marker") + "<!-- oneshot:end -->".len();
        let expected = catalog_block();
        assert!(&CATALOG[begin..end] == expected.as_str(), "catalog one-shot table is stale; replace the marked block with:\n{expected}");
    }
}
