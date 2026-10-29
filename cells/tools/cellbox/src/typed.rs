//! Typed (record) pipeline admission, bounded execution and display.
//!
//! This module owns no grammar: the parser still produces `Cmd` stages, and the
//! stage vocabulary (`select`, `where`, `sort-by`, `take`, `to`, `from`) is parsed
//! by `text_engine::pipeline::parse_stage`. What lives here is
//!
//! * **admission** — [`admit`] classifies every expanded stage and either accepts
//!   the pipeline as legacy byte composition, as a [`TypedPlan`], or refuses it
//!   with a one-line reason. It is pure: no syscall, no VFS access, no redirect
//!   open. The executor calls it before ANY stage runs or file is touched;
//! * **execution** — [`run_rows`] drives a pull chain of `RowSource`s, moving each
//!   row once (no intermediate `String`, no global stdin/sink), and closes the
//!   whole chain when the consumer is done (`take`, error, end of input). A sink
//!   that needs a whole bounded buffer (a redirect, a byte consumer, an enclosing
//!   capture) collects with [`run_rows`]; the live terminal streams with
//!   [`stream_rows`] (one row buffered, Ctrl-C polled between rows);
//! * **display** — the terminal table renderer. Rows that are not explicitly
//!   serialized reach a terminal as a table and a redirect/non-terminal sink as
//!   typed JSONL; the choice never changes the data type or authority.
//!
//! * **external stages** — while a terminal session owns the shell, spawned cells
//!   may join a pipeline through shell-owned byte streams: `rows → to → external`,
//!   `external → from → rows`, `external | external`, `cat F | external` and an
//!   external last stage. [`admit`] decides which shapes are legal; the executor
//!   owns the processes and endpoints. [`StreamRows`] turns the bytes of an
//!   external stage into rows incrementally so `take` can stop an endless producer.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;
use text_engine::pipeline::{
    self, Category, Format, FrameEnd, Input, Item, PipelineError, RowSource, Stage, Value, MAX_STAGES,
};

use crate::rg::RgArgs;

/// Upper bound on bytes a typed pipeline may collect for a sink, a redirect or a
/// byte consumer (equals the global sort payload bound).
pub const MAX_SINK_BYTES: usize = 4 * 1024 * 1024;
/// Upper bound on rows buffered to align a terminal table.
pub const MAX_TABLE_ROWS: usize = 4096;
/// Upper bound on the estimated heap a buffered terminal table may occupy. The
/// cellbox cell runs on a fixed arena (`crate::HEAP_ARENA_BYTES`) with no fallible allocation, so a whole-set
/// table must refuse well before the arena is exhausted.
pub const MAX_TABLE_HEAP_BYTES: usize = 512 * 1024;
/// Largest slice a streaming sink receives in one `write`.
pub const SINK_WRITE_MAX: usize = 4096;
/// Exit status of a pipeline ended by Ctrl-C (128 + SIGINT), as for external stages.
pub const INTERRUPTED_STATUS: i32 = 130;

// ─── Admission input ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirKind {
    /// `>`
    Out,
    /// `>>`
    Append,
    /// `<`
    In,
    /// `2>`
    Err,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redir {
    pub kind: RedirKind,
    pub path: String,
}

/// One pipeline stage after variable/command-substitution expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageIn {
    pub argv: Vec<String>,
    pub redirects: Vec<Redir>,
}

// ─── Command classification ──────────────────────────────────────────────────

/// How the shell would run a command that is not part of the typed vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdClass {
    /// Mutates the owning shell context (cd, assignments, aliases, source, exit,
    /// job control, loop control, `exec`). Standalone only.
    Stateful,
    /// Full-screen / interactive session (`top` without `--batch`). Sole stage,
    /// no data redirects.
    Session,
    /// User-defined shell function (runs in the parent context).
    Function,
    /// Built-in running inside the shell task over captured bytes.
    Builtin,
    /// Spawned cell.
    External,
}

const STATEFUL: &[&str] = &[
    "cd", "alias", "unalias", "export", "unset", "read", "source", ".", "exit", "break",
    "continue", "fg", "bg", "exec",
];

const BUILTINS: &[&str] = &[
    "ls", "cat", "wc", "head", "tail", "find", "uniq", "sort", "tee", "mkdir", "rmdir", "rm",
    "touch", "mv", "cp", "vcat", "vwrite", "vappend", "kill", "ps", "pwd", "uname", "free", "env",
    "uptime", "sleep", "blktest", "echo", "history", "ifconfig", "ip", "date", "top", "grep",
    "sed", "awk", "snapshot", "hv", "shutdown", "clear", "help", "jobs", "test", "[",
];

fn is_assignment(argv: &[String]) -> bool {
    argv.len() == 1
        && argv[0].find('=').is_some_and(|eq| {
            eq > 0 && argv[0].as_bytes()[..eq].iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
        })
}

/// Classify an expanded command that is not a typed-vocabulary stage.
pub fn classify_command(argv: &[String]) -> CmdClass {
    let Some(prog) = argv.first().map(String::as_str) else {
        return CmdClass::Builtin;
    };
    if is_assignment(argv) || STATEFUL.contains(&prog) {
        return CmdClass::Stateful;
    }
    if prog == "top" {
        let words: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
        return match text_engine::top::parse_options(&words) {
            Ok(options) if !options.batch => CmdClass::Session,
            _ => CmdClass::Builtin,
        };
    }
    if prog == "edit" {
        // Bad argv is not a session: the built-in prints usage and exits 2.
        return if crate::edit::valid_args(&argv[1..]) { CmdClass::Session } else { CmdClass::Builtin };
    }
    if prog == "ssh" {
        // Bad argv is not a session: the built-in prints usage and exits 2.
        return if vissh::args::parse_owned(&argv[1..]).is_ok() { CmdClass::Session } else { CmdClass::Builtin };
    }
    if BUILTINS.contains(&prog) {
        return CmdClass::Builtin;
    }
    if crate::shell_state::get_function(prog).is_some() {
        return CmdClass::Function;
    }
    CmdClass::External
}

enum Class {
    /// `ps|ls|top --records`: native row producer (head only).
    Producer,
    /// `rg PATTERN [PATH…]`: the search row producer (head only); argv parsed.
    Search(RgArgs),
    /// `rg -a PATTERN -`: search rows from the bytes of the head before it.
    SearchStdin(RgArgs),
    Row(Stage),
    To(Stage),
    From(Stage),
    /// `cat PATH`: exact-byte producer (head only).
    Cat(String),
    Other(CmdClass),
    /// A typed stage whose arguments `parse_stage` refused.
    Bad(String),
}

fn cat_operand(argv: &[String]) -> Option<&str> {
    match argv {
        [_, path] if !path.starts_with('-') => Some(path.as_str()),
        _ => None,
    }
}

fn classify_stage(argv: &[String]) -> Class {
    let name = argv[0].as_str();
    match name {
        "select" | "where" | "sort-by" | "take" | "to" | "from" => match pipeline::parse_stage(argv) {
            Ok(stage) => match name {
                "to" => Class::To(stage),
                "from" => Class::From(stage),
                _ => Class::Row(stage),
            },
            Err(error) => Class::Bad(format!("{}: {}", name, error)),
        },
        "ps" | "ls" | "top" if argv[1..].iter().any(|arg| arg == "--records") => Class::Producer,
        // `rg` is always typed; its whole argv is checked here, before any effect.
        "rg" => match crate::rg::args::parse(argv) {
            Ok(args) if args.stdin => Class::SearchStdin(args),
            Ok(args) => Class::Search(args),
            Err(usage) => Class::Bad(format!("rg: {}", usage.0)),
        },
        "cat" if cat_operand(argv).is_some() => {
            Class::Cat(String::from(cat_operand(argv).unwrap_or("")))
        }
        _ => Class::Other(classify_command(argv)),
    }
}

/// True when the first word of an UNEXPANDED simple command can only be, or may
/// become, a typed stage or an interactive session. Plain commands skip admission.
/// The explicit form `cellbox CMD` / `/bin/cellbox CMD` (S10-0.6) is looked through, so
/// `cellbox ps --records | select tid` admits exactly as `ps --records | select tid`.
pub fn might_be_typed(first: &str, args: &[&str]) -> bool {
    if crate::oneshot::is_prefix(first) {
        return match args.split_first() {
            Some((command, rest)) => might_be_typed(command, rest),
            None => false,
        };
    }
    matches!(first, "select" | "where" | "sort-by" | "take" | "to" | "from" | "top" | "rg")
        || first.contains('$')
        || (matches!(first, "ps" | "ls") && args.contains(&"--records"))
}

// ─── Admission result ────────────────────────────────────────────────────────

/// Where the bytes or rows of a typed pipeline come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// Native row producer, constructed from `TypedPlan::producer_argv`.
    Producer,
    /// `cat PATH`: exact file bytes, converted by the following `from`.
    Cat(String),
    /// Any other built-in; its printed bytes (captured) feed the following `from`.
    Capture,
    /// `from … < PATH`: exact file bytes feed the leading `from`.
    FromFile(String),
    /// A one-shot's stdin (`/bin/cellbox rg … -`, S10-0.5): the Stdin grant's bytes feed
    /// the leading `rg -`. Never produced by [`admit`].
    Stdin,
    /// The first `count` stages are external commands joined by kernel streams;
    /// the last one's stdout feeds the `from` stage that follows.
    External { count: usize },
}

#[derive(Debug)]
pub struct TypedPlan {
    pub head: Head,
    /// Producer argv; for `ps|ls|top` the display selector `--text` is removed
    /// (`rg` parses its own argv, where `--text` may be a pattern or a path).
    pub producer_argv: Vec<String>,
    /// `--text` was given: nonterminal/redirect rows use the plain text table (the
    /// search text renderer for `rg`) instead of typed JSONL.
    pub text_display: bool,
    /// What the head yields into the typed segment: Rows, or Frames (`top --records`).
    pub category: Category,
    /// Typed segment. For a byte head the first element is the `from` stage, except
    /// when `rg -` consumes the bytes (`search` with `stdin`).
    pub stages: Vec<Stage>,
    /// Number of leading `cmds` covered by the head + typed segment; the rest are
    /// byte-consuming built-ins fed by a terminal `to` (or, after `rg`, by its
    /// default sink).
    pub typed_cmds: usize,
    /// `(path, append)` of the final stage's stdout redirect when the typed
    /// segment is the end of the pipeline.
    pub output: Option<(String, bool)>,
    /// Number of external stages that close the pipeline (they follow a terminal
    /// `to`, or `rg`'s rows, and start at `typed_cmds`).
    pub tail_externals: usize,
    /// `(path, append)` of the final stage's stdout redirect when that stage is
    /// one of the `tail_externals`.
    pub tail_output: Option<(String, bool)>,
    /// Parsed argv of the `rg` stage that produces the rows (the head, or `rg -`
    /// after a byte head).
    pub search: Option<RgArgs>,
    /// Pipeline index of that `rg` stage.
    pub search_index: usize,
    /// Rows reach their default sink as `rg` produced them (only `where`, `take`
    /// and `sort-by` follow it, no explicit `to`): the search text renderer
    /// replaces the table (terminal, `--text`) (D76-9).
    pub search_text: bool,
    /// The pipeline status is `rg`'s completion status: nothing but an optional
    /// explicit `to` follows it (D76-8).
    pub search_status: bool,
}

/// Where the bytes entering the first external stage of a byte pipeline come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ByteHead {
    /// The first stage is external; its stdin is empty.
    None,
    /// `cat PATH`: the exact file bytes become the first external stdin.
    Cat(String),
    /// The built-in stages before the first external one run over captured bytes.
    Capture,
}

/// A byte pipeline (no typed stage) whose tail is made of external commands.
#[derive(Debug)]
pub struct BytePlan {
    pub head: ByteHead,
    /// Number of leading built-in stages; the external stages are `prefix..`.
    pub prefix: usize,
    /// `(path, append)` of the final stage's stdout redirect; `None` shows the output.
    pub output: Option<(String, bool)>,
}

#[derive(Debug)]
pub enum Admission {
    /// Not a typed pipeline: run through the existing byte composition.
    Legacy,
    Typed(TypedPlan),
    /// Byte pipeline containing external commands (terminal session only).
    Bytes(BytePlan),
    Reject(String),
}

fn reject<T>(message: impl Into<String>) -> Result<T, String> {
    Err(message.into())
}

fn stateful_message(name: &str, context: &str) -> String {
    format!("`{}` changes shell state and cannot run {}", name, context)
}

fn session_message(name: &str) -> String {
    format!("`{}` is an interactive session; it must be the only stage with no redirects", name)
}

/// Whole-pipeline static admission. `stages` is the complete, expanded pipeline
/// (one element for a simple command). Nothing is executed or opened.
///
/// `externals` is true only while a terminal session owns the shell: spawned
/// cells can then join a pipeline through shell-owned byte streams. Without a
/// session every external stage of a typed pipeline is refused and plain byte
/// pipelines keep their legacy composition.
pub fn admit(stages: &[StageIn], externals: bool) -> Admission {
    match admit_inner(stages, externals, false) {
        Ok(admission) => admission,
        Err(message) => Admission::Reject(message),
    }
}

/// Admission of a one-shot command (`/bin/cellbox <command>`, S10-0): one stage, no
/// redirects, no external stage, and a leading `rg -` searches the one-shot's stdin
/// ([`Head::Stdin`]) instead of needing `< FILE`.
#[cfg(any(test, not(feature = "shell_test")))]
pub fn admit_oneshot(stage: &StageIn) -> Admission {
    match admit_inner(core::slice::from_ref(stage), false, true) {
        Ok(admission) => admission,
        Err(message) => Admission::Reject(message),
    }
}

fn is_external(class: &Class) -> bool {
    matches!(class, Class::Other(CmdClass::External))
}

fn has_output_redirect(stage: &StageIn) -> bool {
    stage.redirects.iter().any(|r| matches!(r.kind, RedirKind::Out | RedirKind::Append))
}

/// Child-launch bound on the argument list (the immutable child context carries
/// 7 bytes of framing plus every argument and its terminator).
const EXTERNAL_ARGV_LIMIT: usize = 512;

/// True when `args` (the arguments after the program name) fit one child launch.
pub fn argv_fits(args: &[String]) -> bool {
    args.iter()
        .try_fold(7usize, |n, arg| n.checked_add(arg.len() + 1))
        .is_some_and(|n| n <= EXTERNAL_ARGV_LIMIT)
}

/// Launch-limit refusal for one external stage, raised before any stage starts.
fn check_external(stage: &StageIn) -> Result<(), String> {
    let name = stage.argv[0].as_str();
    if stage.argv.iter().any(|arg| arg.contains('\0')) {
        return reject(format!("`{}`: arguments cannot contain NUL", name));
    }
    if !argv_fits(&stage.argv[1..]) {
        return reject(format!(
            "`{}`: argument list exceeds the {}-byte child launch limit",
            name, EXTERNAL_ARGV_LIMIT
        ));
    }
    Ok(())
}

fn admit_inner(stages: &[StageIn], externals: bool, stdin_head: bool) -> Result<Admission, String> {
    let n = stages.len();
    if n > MAX_STAGES {
        return reject(format!("pipeline has {} stages; at most {} are allowed", n, MAX_STAGES));
    }
    let classes: Vec<Class> = stages.iter().map(|s| classify_stage(&s.argv)).collect();
    let typed_any = classes.iter().any(|c| {
        matches!(
            c,
            Class::Producer
                | Class::Search(_)
                | Class::SearchStdin(_)
                | Class::Row(_)
                | Class::To(_)
                | Class::From(_)
                | Class::Bad(_)
        )
    });

    if !typed_any {
        if n == 1 {
            if matches!(classes[0], Class::Other(CmdClass::Session)) && !stages[0].redirects.is_empty() {
                return reject(session_message(&stages[0].argv[0]));
            }
            // An external whose stdout is redirected runs as a one-stage byte pipeline:
            // its stdout is a stream into the file, never the terminal session.
            if externals && is_external(&classes[0]) && has_output_redirect(&stages[0]) {
                return admit_bytes(stages, &classes);
            }
            return Ok(Admission::Legacy);
        }
        for (stage, class) in stages.iter().zip(&classes) {
            match class {
                Class::Other(CmdClass::Stateful) => {
                    return reject(stateful_message(&stage.argv[0], "inside a pipeline"))
                }
                Class::Other(CmdClass::Function) => {
                    return reject(stateful_message(&stage.argv[0], "inside a pipeline (functions run in the shell context)"))
                }
                Class::Other(CmdClass::Session) => return reject(session_message(&stage.argv[0])),
                _ => {}
            }
        }
        if externals && classes.iter().any(is_external) {
            return admit_bytes(stages, &classes);
        }
        return Ok(Admission::Legacy);
    }

    // Typed pipeline: every stage must be a typed stage, a native byte stage, a
    // pure built-in byte consumer or (terminal session only) an external command.
    for (stage, class) in stages.iter().zip(&classes) {
        let name = stage.argv[0].as_str();
        match class {
            Class::Bad(message) => return reject(message.clone()),
            Class::Other(CmdClass::Stateful) => {
                return reject(stateful_message(name, "inside a pipeline"))
            }
            Class::Other(CmdClass::Function) => {
                return reject(stateful_message(name, "inside a pipeline (functions run in the shell context)"))
            }
            Class::Other(CmdClass::Session) => return reject(session_message(name)),
            Class::Other(CmdClass::External) => {
                if !externals {
                    return reject(format!(
                        "external command `{}` cannot join a typed pipeline without a terminal session",
                        name
                    ));
                }
                check_external(stage)?;
            }
            _ => {}
        }
    }

    // Head. Leading external commands feed the `from` (or `rg -`) stage that must
    // follow them.
    let external_head = classes.iter().take_while(|class| is_external(class)).count();
    let head = if external_head > 0 {
        if !matches!(classes.get(external_head), Some(Class::From(_) | Class::SearchStdin(_))) {
            return reject(format!(
                "`{}` output is bytes: `from json|jsonl` must directly follow it",
                stages[external_head - 1].argv[0]
            ));
        }
        Head::External { count: external_head }
    } else {
        match &classes[0] {
            Class::Producer | Class::Search(_) => Head::Producer,
            Class::Cat(path) => Head::Cat(path.clone()),
            Class::Other(CmdClass::Builtin) => Head::Capture,
            Class::From(_) | Class::SearchStdin(_) => {
                match stages[0].redirects.iter().find(|r| r.kind == RedirKind::In).map(|r| r.path.clone()) {
                    Some(path) => Head::FromFile(path),
                    None if stdin_head && matches!(classes[0], Class::SearchStdin(_)) => Head::Stdin,
                    None => {
                        return reject(format!(
                            "`{}` needs byte input: pipe bytes into it or redirect `< FILE`",
                            stages[0].argv[0]
                        ))
                    }
                }
            }
            Class::Row(_) | Class::To(_) => {
                return reject(format!("`{}` needs record input; start the pipeline with a record producer or `from`", stages[0].argv[0]))
            }
            Class::Bad(_) | Class::Other(_) => unreachable_class()?,
        }
    };

    // Typed run.
    let mut typed: Vec<Stage> = Vec::new();
    let mut end = match head {
        Head::FromFile(_) | Head::Stdin => 0,
        Head::External { count } => count,
        _ => 1,
    };
    // The search stage: the head itself, or `rg -` directly consuming the byte head.
    let mut search: Option<(RgArgs, usize)> = match &classes[0] {
        Class::Search(args) => Some((args.clone(), 0)),
        _ => None,
    };
    if !matches!(head, Head::Producer) {
        if let Some(Class::SearchStdin(args)) = classes.get(end) {
            search = Some((args.clone(), end));
            end += 1;
        }
    }
    let mut ended_with_to = false;
    while end < n {
        match &classes[end] {
            Class::Row(stage) | Class::From(stage) => typed.push(stage.clone()),
            Class::To(stage) => {
                typed.push(stage.clone());
                end += 1;
                ended_with_to = true;
                break;
            }
            _ => break,
        }
        end += 1;
    }
    // `rg` rows may feed byte consumers and external commands through its default
    // sink (typed JSONL, or the search text with `--text`); other producers need `to`.
    let feeds_bytes = ended_with_to || search.is_some();

    if matches!(head, Head::Cat(_) | Head::Capture) && typed.is_empty() && search.is_none() {
        return reject(format!(
            "`{}` output is bytes: `from json|jsonl` must directly follow it before any typed stage",
            stages[0].argv[0]
        ));
    }
    // Anything after the typed run must be a byte consumer fed by an explicit `to`:
    // either built-ins over captured bytes or external commands, never both.
    let tail_externals = classes[end..].iter().filter(|class| is_external(class)).count();
    for (i, class) in classes.iter().enumerate().skip(end) {
        match class {
            Class::Other(CmdClass::External) => {
                if matches!(head, Head::External { .. }) {
                    return reject(format!(
                        "`{}` cannot follow typed stages after an external head: an external command may start or end a typed pipeline, not both",
                        stages[i].argv[0]
                    ));
                }
                if !feeds_bytes {
                    return reject(format!(
                        "`{}` cannot consume records or bytes here: end the typed stages with `to json|jsonl|text`",
                        stages[i].argv[0]
                    ));
                }
            }
            Class::Other(CmdClass::Builtin) | Class::Cat(_) => {
                if !feeds_bytes {
                    return reject(format!(
                        "`{}` cannot consume records or bytes here: end the typed stages with `to json|jsonl|text`",
                        stages[i].argv[0]
                    ));
                }
                if tail_externals > 0 {
                    return reject(format!(
                        "built-in `{}` cannot be mixed with external commands after `to`",
                        stages[i].argv[0]
                    ));
                }
            }
            Class::Producer | Class::Search(_) => {
                return reject(format!("`{}` is a record producer and must start the pipeline", stages[i].argv[0]))
            }
            Class::SearchStdin(_) => {
                return reject(format!(
                    "`{}` searches bytes: it must directly follow `cat`, a built-in, external commands or take `< FILE`",
                    stages[i].argv[0]
                ))
            }
            _ => {
                return reject(format!(
                    "typed stage `{}` cannot follow a byte stage; `to` is terminal for typed stages",
                    stages[i].argv[0]
                ))
            }
        }
    }
    let category = match head {
        Head::Producer => crate::native_rows::producer_category(&stages[0].argv),
        _ => Category::Rows,
    };
    let input = match (&head, category) {
        _ if search.is_some() => Input::Rows,
        (Head::Producer, Category::Frames) => Input::Frames,
        (Head::Producer, Category::Rows) => Input::Rows,
        _ => Input::Bytes,
    };
    pipeline::validate_stage_sequence(input, &typed).map_err(|error| error.to_string())?;

    // Redirects.
    let mut output: Option<(String, bool)> = None;
    let mut tail_output: Option<(String, bool)> = None;
    for (i, stage) in stages.iter().enumerate() {
        for redirect in &stage.redirects {
            match redirect.kind {
                RedirKind::Err => {
                    return reject(
                        "`2>` is not supported on typed pipelines: diagnostics never enter data streams",
                    )
                }
                RedirKind::In => {
                    let allowed = i == 0 && matches!(head, Head::FromFile(_) | Head::Capture);
                    if !allowed {
                        return reject(format!("`<` is not valid on `{}` in a typed pipeline", stage.argv[0]));
                    }
                }
                RedirKind::Out | RedirKind::Append => {
                    if i + 1 != n {
                        return reject(format!("output redirect on `{}` is only allowed on the final stage", stage.argv[0]));
                    }
                    let target = (redirect.path.clone(), redirect.kind == RedirKind::Append);
                    if end == n {
                        if output.is_some() {
                            return reject("more than one output redirect on the final stage");
                        }
                        output = Some(target);
                    } else if tail_externals > 0 {
                        if tail_output.is_some() {
                            return reject("more than one output redirect on the final stage");
                        }
                        tail_output = Some(target);
                    }
                }
            }
        }
    }

    let mut producer_argv = stages[0].argv.clone();
    let mut text_display = false;
    if let Some((args, _)) = &search {
        text_display = args.text_display;
    } else if matches!(head, Head::Producer) {
        let before = producer_argv.len();
        producer_argv.retain(|arg| arg != "--text");
        text_display = producer_argv.len() != before;
    }
    let search_text = search.is_some()
        && typed.iter().all(|stage| matches!(stage, Stage::Where { .. } | Stage::Take { .. } | Stage::SortBy { .. }));
    let search_status = search.is_some() && end == n && typed.iter().all(|stage| matches!(stage, Stage::To(_)));
    let (search, search_index) = match search {
        Some((args, index)) => (Some(args), index),
        None => (None, 0),
    };

    Ok(Admission::Typed(TypedPlan {
        head,
        producer_argv,
        text_display,
        category,
        stages: typed,
        typed_cmds: end,
        output,
        tail_externals,
        tail_output,
        search,
        search_index,
        search_text,
        search_status,
    }))
}

/// Byte pipeline (no typed stage) with external commands: optional built-in
/// producers, then only external commands. `classes` holds no stateful, function
/// or session stage (the caller refused those).
fn admit_bytes(stages: &[StageIn], classes: &[Class]) -> Result<Admission, String> {
    let n = stages.len();
    let prefix = classes.iter().position(is_external).unwrap_or(n);
    for i in prefix..n {
        if !is_external(&classes[i]) {
            return reject(format!(
                "`{}` cannot consume the output of an external command; only external commands may follow one",
                stages[i].argv[0]
            ));
        }
        check_external(&stages[i])?;
    }
    let head = match (prefix, classes.first()) {
        (0, _) => ByteHead::None,
        (1, Some(Class::Cat(path))) => ByteHead::Cat(path.clone()),
        _ => ByteHead::Capture,
    };

    let mut output: Option<(String, bool)> = None;
    for (i, stage) in stages.iter().enumerate() {
        for redirect in &stage.redirects {
            match redirect.kind {
                RedirKind::Err => {
                    return reject(
                        "`2>` is not supported on pipelines with external commands: diagnostics never enter data streams",
                    )
                }
                RedirKind::In => {
                    if !(i == 0 && head == ByteHead::Capture) {
                        return reject(format!(
                            "`<` is not valid on `{}` in a pipeline with external commands",
                            stage.argv[0]
                        ));
                    }
                }
                RedirKind::Out | RedirKind::Append => {
                    if i + 1 != n {
                        return reject(format!("output redirect on `{}` is only allowed on the final stage", stage.argv[0]));
                    }
                    if output.is_some() {
                        return reject("more than one output redirect on the final stage");
                    }
                    output = Some((redirect.path.clone(), redirect.kind == RedirKind::Append));
                }
            }
        }
    }
    Ok(Admission::Bytes(BytePlan { head, prefix, output }))
}

/// `classes[0]` was already narrowed away from these variants by the earlier loop;
/// reaching this is a classifier bug, reported as a refusal rather than a panic.
fn unreachable_class<T>() -> Result<T, String> {
    Err(String::from("internal: unclassifiable first stage"))
}

/// Refusal reason when `stage` may not run as a background (`&`) single command;
/// `None` when the existing single-command background behavior applies.
pub fn background_refusal(stage: &StageIn) -> Option<String> {
    let name = stage.argv[0].as_str();
    match classify_stage(&stage.argv) {
        Class::Bad(message) => Some(message),
        Class::Producer
        | Class::Search(_)
        | Class::SearchStdin(_)
        | Class::Row(_)
        | Class::To(_)
        | Class::From(_) => Some(format!(
            "`{}` is a typed stage and cannot run in the background",
            name
        )),
        Class::Other(CmdClass::Stateful) => Some(stateful_message(name, "in the background")),
        Class::Other(CmdClass::Function) => {
            Some(stateful_message(name, "in the background (functions run in the shell context)"))
        }
        Class::Other(CmdClass::Session) => Some(session_message(name)),
        Class::Cat(_) | Class::Other(CmdClass::Builtin | CmdClass::External) => None,
    }
}

// ─── Execution ───────────────────────────────────────────────────────────────

/// First failure observed while pulling, attributed to the stage that raised it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFailure {
    pub stage: usize,
    pub message: String,
    /// Pipeline status of the failure: 1, or 2 for an `rg` Collected run over its
    /// bound ([`SEARCH_COLLECT_BOUND`]).
    pub status: i32,
}

impl RunFailure {
    fn new(stage: usize, message: String) -> Self {
        RunFailure { stage, message, status: 1 }
    }
}

type Slot = Rc<RefCell<Option<RunFailure>>>;

/// Transparent adapter that remembers which stage produced the first error, so
/// the job can record member statuses without parsing messages.
struct Tap {
    inner: Box<dyn RowSource>,
    stage: usize,
    slot: Slot,
}

impl RowSource for Tap {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        let result = self.inner.next_item();
        if let Err(error) = &result {
            let mut slot = self.slot.borrow_mut();
            if slot.is_none() {
                *slot = Some(RunFailure::new(self.stage, error.to_string()));
            }
        }
        result
    }

    fn close(&mut self) {
        self.inner.close();
    }

    fn completion(&self) -> Option<i32> {
        self.inner.completion()
    }
}

/// Shared view of a producer's completion status, filled in when the chain
/// releases the producer (`run_rows`/`stream_rows` consume the chain).
pub type CompletionSlot = Rc<core::cell::Cell<Option<i32>>>;

/// Transparent adapter that publishes `inner`'s completion status when it is
/// closed (end of input, an error, a satisfied `take`, Ctrl-C: every chain exit
/// closes its producer).
struct Observed {
    inner: Box<dyn RowSource>,
    slot: CompletionSlot,
}

impl RowSource for Observed {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        self.inner.next_item()
    }

    fn close(&mut self) {
        self.inner.close();
        self.slot.set(self.inner.completion());
    }

    fn completion(&self) -> Option<i32> {
        self.inner.completion()
    }
}

/// `source` plus the slot its completion status lands in once it is closed.
pub fn observe_completion(source: Box<dyn RowSource>) -> (Box<dyn RowSource>, CompletionSlot) {
    let slot: CompletionSlot = Rc::new(core::cell::Cell::new(None));
    (Box::new(Observed { inner: source, slot: slot.clone() }), slot)
}

/// What items become when the typed segment does not end in an explicit `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultSink {
    /// Terminal (or `--text`): aligned plain table (one per frame), display only.
    Table,
    /// Redirect/nonterminal: typed JSONL (lossless, one row or frame envelope per line).
    TypedJsonl,
    /// `rg` rows (terminal, or `--text`): one escaped `PATH:LINE:TEXT` line per row,
    /// streamed (`crate::rg::text::render_row`), never buffered as a table.
    SearchText,
}

/// The default sink of a typed segment that ends the pipeline without a redirect.
/// `display` is true only when that end is the live terminal display; a one-shot's
/// stdout is a non-terminal stream (S10-0.5) and never displays. `--text` selects the
/// display renderer for a non-terminal too.
pub fn default_sink(display: bool, plan: &TypedPlan) -> DefaultSink {
    match (display || plan.text_display, plan.search_text) {
        (true, true) => DefaultSink::SearchText,
        (true, false) => DefaultSink::Table,
        (false, _) => DefaultSink::TypedJsonl,
    }
}

/// Turn a byte payload into a row source with the leading `from` stage.
pub fn rows_from_bytes(format: &Format, bytes: &[u8]) -> Result<Box<dyn RowSource>, PipelineError> {
    Ok(Box::new(pipeline::VecSource::new(pipeline::decode_rows(format, bytes)?)))
}

/// The item chain for `stages` over `source`: the producer wrapped once with the
/// order guard for its `category`, every row stage applied and tapped (so the first
/// error names its stage), plus the explicit trailing `to` format.
/// Returns `(chain, explicit_format, failure_slot, sink_stage)`; `sink_stage` is the
/// pipeline index of the serializer (or of the last row stage when there is none).
fn build_chain(
    source: Box<dyn RowSource>,
    category: Category,
    first_stage: usize,
    stages: &[Stage],
) -> Result<(Box<dyn RowSource>, Option<Format>, Slot, usize), RunFailure> {
    let slot: Slot = Rc::new(RefCell::new(None));
    let guarded = pipeline::check_items(source, category);
    let mut chain: Box<dyn RowSource> = Box::new(Tap { inner: guarded, stage: first_stage, slot: slot.clone() });
    let (row_stages, explicit) = match stages.split_last() {
        Some((Stage::To(format), rest)) => (rest, Some(format.clone())),
        _ => (stages, None),
    };
    for (k, stage) in row_stages.iter().enumerate() {
        let index = first_stage + 1 + k;
        chain = match pipeline::apply_row_stage(chain, stage) {
            Ok(next) => next,
            Err(error) => return Err(RunFailure::new(index, error.to_string())),
        };
        chain = Box::new(Tap { inner: chain, stage: index, slot: slot.clone() });
    }
    let sink_stage = first_stage + row_stages.len() + usize::from(explicit.is_some());
    Ok((chain, explicit, slot, sink_stage))
}

/// What a Collected run (a redirect, a byte consumer, an enclosing capture) may
/// gather; over it the run fails, nothing is written, and the pipeline ends with
/// `status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectBound {
    pub bytes: usize,
    pub status: i32,
}

/// Every typed producer except `rg`.
pub const COLLECT_BOUND: CollectBound = CollectBound { bytes: MAX_SINK_BYTES, status: 1 };
/// `rg`-headed Collected runs (D76-6): the general 4 MiB bound cannot fit the
/// cellbox arena next to the search's regex/glob construction peak.
pub const SEARCH_COLLECT_BOUND: CollectBound = CollectBound { bytes: 256 * 1024, status: 2 };

/// Why collecting stopped.
enum Collect {
    Failed(String),
    Overflow,
    Interrupted,
}

/// The chain as a Collected sink sees it (S10-3b): `interrupted` is polled before every
/// pull, and the first `true` ends the input, so an endless producer is not drained to
/// the bound with Ctrl-C pending.
struct Interruptible<'a> {
    chain: &'a mut dyn RowSource,
    interrupted: &'a mut dyn FnMut() -> bool,
    hit: bool,
}

impl RowSource for Interruptible<'_> {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        if self.hit || (self.interrupted)() {
            self.hit = true;
            return Ok(None);
        }
        self.chain.next_item()
    }

    fn close(&mut self) {
        self.chain.close();
    }

    fn completion(&self) -> Option<i32> {
        self.chain.completion()
    }
}

/// Pull `source` (a producer of `category` items) through the row stages in `stages`
/// (an optional trailing `to` selects an explicit serializer) and collect the sink bytes.
///
/// `first_stage` is the pipeline index of `source`; row stage `k` is `first_stage
/// + 1 + k`. The whole chain is closed on every exit path, so a satisfied `take`,
/// an error or end of input always releases the producer. On failure NO bytes are
/// returned and the caller opens no redirect.
pub fn run_rows(
    source: Box<dyn RowSource>,
    category: Category,
    first_stage: usize,
    stages: &[Stage],
    default_sink: DefaultSink,
) -> Result<Vec<u8>, RunFailure> {
    run_rows_within(source, category, first_stage, stages, default_sink, COLLECT_BOUND, &mut || false)
}

/// [`run_rows`] with an explicit collect bound and an interrupt poll, made before every
/// pull. On interrupt the chain is closed, no bytes are returned, and the failure carries
/// [`INTERRUPTED_STATUS`].
pub fn run_rows_within(
    source: Box<dyn RowSource>,
    category: Category,
    first_stage: usize,
    stages: &[Stage],
    default_sink: DefaultSink,
    bound: CollectBound,
    interrupted: &mut dyn FnMut() -> bool,
) -> Result<Vec<u8>, RunFailure> {
    let (mut chain, explicit, slot, sink_stage) = build_chain(source, category, first_stage, stages)?;
    let mut gate = Interruptible { chain: &mut *chain, interrupted, hit: false };
    let result = match &explicit {
        Some(format) => encode_all(&mut gate, category, format, bound.bytes),
        None => match default_sink {
            DefaultSink::Table => match render_table(&mut gate, category) {
                Ok(bytes) if bytes.len() > bound.bytes => Err(Collect::Overflow),
                Ok(bytes) => Ok(bytes),
                Err(message) => Err(Collect::Failed(message)),
            },
            DefaultSink::TypedJsonl => encode_all(&mut gate, category, &Format::JsonlTyped, bound.bytes),
            DefaultSink::SearchText => render_search_text(&mut gate, bound.bytes),
        },
    };
    let result = if gate.hit { Err(Collect::Interrupted) } else { result };
    chain.close();
    result.map_err(|stop| {
        slot.borrow_mut().take().unwrap_or(match stop {
            Collect::Failed(message) => RunFailure::new(sink_stage, message),
            Collect::Overflow => RunFailure {
                stage: sink_stage,
                message: format!("output exceeds the {} byte bound", bound.bytes),
                status: bound.status,
            },
            Collect::Interrupted => RunFailure {
                stage: sink_stage,
                message: String::from("interrupted"),
                status: INTERRUPTED_STATUS,
            },
        })
    })
}

fn encode_all(chain: &mut dyn RowSource, category: Category, format: &Format, bound: usize) -> Result<Vec<u8>, Collect> {
    let failed = |error: PipelineError| Collect::Failed(error.to_string());
    let mut out = Vec::new();
    let mut first = true;
    while let Some(item) = chain.next_item().map_err(failed)? {
        pipeline::encode_item(format, category, item, first, &mut out).map_err(failed)?;
        first = false;
        if out.len() > bound {
            return Err(Collect::Overflow);
        }
    }
    pipeline::finish_encoding(format, !first, &mut out);
    Ok(out)
}

/// One rendered search line for an item; anything but a search row is a failure.
fn search_line(item: Item, out: &mut Vec<u8>) -> Result<(), PipelineError> {
    match item {
        Item::Row(row) => crate::rg::text::render_row(&row, out),
        _ => Err(PipelineError::Sequence(String::from("the search text renderer takes search rows only"))),
    }
}

fn render_search_text(chain: &mut dyn RowSource, bound: usize) -> Result<Vec<u8>, Collect> {
    let failed = |error: PipelineError| Collect::Failed(error.to_string());
    let mut out = Vec::new();
    while let Some(item) = chain.next_item().map_err(failed)? {
        search_line(item, &mut out).map_err(failed)?;
        if out.len() > bound {
            return Err(Collect::Overflow);
        }
    }
    Ok(out)
}

// ─── Streaming terminal sink ─────────────────────────────────────────────────

/// Why a [`ChunkSink`] refused bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkStop {
    /// The user interrupted the pipeline (Ctrl-C).
    Interrupted,
    /// The destination failed, one diagnostic line.
    Failed(String),
}

/// Destination of a streamed sink (the terminal). Bytes are delivered as the rows
/// that produce them are pulled; nothing is collected.
pub trait ChunkSink {
    /// Deliver `bytes` (at most [`SINK_WRITE_MAX`]), retrying backpressure itself.
    fn write(&mut self, bytes: &[u8]) -> Result<(), SinkStop>;
    /// Non-blocking look for a user interrupt; polled before every row is pulled.
    fn interrupted(&mut self) -> bool;
}

/// How a streamed typed segment ended without a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEnd {
    Complete,
    Interrupted,
}

impl StreamEnd {
    /// Pipeline status: 0 when complete, 130 after Ctrl-C.
    pub fn status(self) -> i32 {
        match self {
            StreamEnd::Complete => 0,
            StreamEnd::Interrupted => INTERRUPTED_STATUS,
        }
    }
}

/// Length of the prefix of `bytes` that is final under lossy UTF-8 conversion: a
/// trailing incomplete (or invalid) sequence waits for the next bytes, so the
/// delivered text equals `String::from_utf8_lossy` of the whole output.
fn complete_prefix(bytes: &[u8]) -> usize {
    match bytes.utf8_chunks().last() {
        Some(chunk) => bytes.len() - chunk.invalid().len(),
        None => 0,
    }
}

/// Encoded bytes on their way to a sink: one row's worth is held, never the stream.
struct Delivery<'a> {
    sink: &'a mut dyn ChunkSink,
    pending: Vec<u8>,
    last: Option<u8>,
}

impl<'a> Delivery<'a> {
    fn new(sink: &'a mut dyn ChunkSink) -> Self {
        Self { sink, pending: Vec::new(), last: None }
    }

    fn poll(&mut self) -> Result<(), Halt> {
        if self.sink.interrupted() {
            Err(Halt::Interrupted)
        } else {
            Ok(())
        }
    }

    /// Hand the settled part of `pending` to the sink in bounded slices.
    fn flush(&mut self, end_of_output: bool) -> Result<(), Halt> {
        let split = if end_of_output { self.pending.len() } else { complete_prefix(&self.pending) };
        if split == 0 {
            return Ok(());
        }
        {
            let text = String::from_utf8_lossy(&self.pending[..split]);
            for piece in text.as_bytes().chunks(SINK_WRITE_MAX) {
                self.sink.write(piece).map_err(|stop| match stop {
                    SinkStop::Interrupted => Halt::Interrupted,
                    SinkStop::Failed(message) => Halt::Failed(message),
                })?;
            }
            self.last = text.as_bytes().last().copied();
        }
        self.pending.drain(..split);
        Ok(())
    }

    /// Deliver the rest and end the output with a newline when it has none.
    fn finish(&mut self) -> Result<(), Halt> {
        self.flush(true)?;
        if self.last.is_some_and(|byte| byte != b'\n') {
            self.pending.push(b'\n');
            self.flush(true)?;
        }
        Ok(())
    }
}

fn stream_encoded(
    chain: &mut dyn RowSource,
    category: Category,
    format: &Format,
    out: &mut Delivery,
) -> Result<(), Halt> {
    let mut first = true;
    loop {
        out.poll()?;
        let Some(item) = chain.next_item().map_err(halt)? else {
            break;
        };
        pipeline::encode_item(format, category, item, first, &mut out.pending).map_err(halt)?;
        first = false;
        out.flush(false)?;
    }
    pipeline::finish_encoding(format, !first, &mut out.pending);
    out.finish()
}

/// Search rows rendered and delivered one by one (no buffering, unlike the table).
fn stream_search_text(chain: &mut dyn RowSource, out: &mut Delivery) -> Result<(), Halt> {
    loop {
        out.poll()?;
        let Some(item) = chain.next_item().map_err(halt)? else {
            break;
        };
        search_line(item, &mut out.pending).map_err(halt)?;
        out.flush(false)?;
    }
    out.finish()
}

impl TableOut for Delivery<'_> {
    fn interrupted(&mut self) -> bool {
        self.sink.interrupted()
    }

    fn emit(&mut self, text: &str) -> Result<(), Halt> {
        self.pending.extend_from_slice(text.as_bytes());
        self.flush(false)
    }
}

/// Like [`run_rows`] for a sink that is the terminal: each encoded item is written
/// to `sink` as soon as it is pulled, so memory stays bounded by one item however
/// long the source runs, and the sink is polled for an interrupt before every pull.
/// The chain is closed on every exit path (a satisfied `take`, Ctrl-C, an error).
///
/// Explicit serializers (`to jsonl|json|text|...`), the typed JSONL default and the
/// search text default stream item by item. The default table is the one
/// exception: it buffers rows to align the columns — the whole stream for Rows,
/// one frame for Frames — bounded by [`MAX_TABLE_ROWS`] and
/// [`MAX_TABLE_HEAP_BYTES`] (refusing with the usual one-line error), then streams
/// the rendered lines; a frame is written at its End.
///
/// Unlike [`run_rows`] a failure can arrive after bytes were delivered: the rows
/// before it have been shown, then the failure is reported.
pub fn stream_rows(
    source: Box<dyn RowSource>,
    category: Category,
    first_stage: usize,
    stages: &[Stage],
    default_sink: DefaultSink,
    sink: &mut dyn ChunkSink,
) -> Result<StreamEnd, RunFailure> {
    let (mut chain, explicit, slot, sink_stage) = build_chain(source, category, first_stage, stages)?;
    let mut out = Delivery::new(sink);
    let outcome = match (&explicit, default_sink) {
        (Some(format), _) => stream_encoded(&mut *chain, category, format, &mut out),
        (None, DefaultSink::Table) => write_tables(&mut *chain, category, &mut out).and_then(|()| out.finish()),
        (None, DefaultSink::TypedJsonl) => stream_encoded(&mut *chain, category, &Format::JsonlTyped, &mut out),
        (None, DefaultSink::SearchText) => stream_search_text(&mut *chain, &mut out),
    };
    chain.close();
    match outcome {
        Ok(()) => Ok(StreamEnd::Complete),
        Err(Halt::Interrupted) => Ok(StreamEnd::Interrupted),
        Err(Halt::Failed(message)) => {
            Err(slot.borrow_mut().take().unwrap_or(RunFailure::new(sink_stage, message)))
        }
    }
}

/// A typed segment serialized incrementally for an external consumer: items are
/// pulled only when the consumer's stdin has room, so memory stays bounded by one
/// fill (not by the stream) and an endless producer (`top --records`) never runs
/// ahead of the stage reading it.
pub struct RowFeed {
    chain: Box<dyn RowSource>,
    category: Category,
    /// The serializer; `None` is the search text renderer.
    format: Option<Format>,
    /// No row encoded yet (a `json` array needs its opening bracket first).
    first: bool,
    slot: Slot,
    sink_stage: usize,
    closed: bool,
}

impl RowFeed {
    /// Build the chain over `source` (row stages applied, nothing pulled yet). The
    /// serializer is the explicit trailing `to`; without one, the search text for
    /// [`DefaultSink::SearchText`] and typed JSONL otherwise (a pipe is never a table).
    pub fn new(
        source: Box<dyn RowSource>,
        category: Category,
        first_stage: usize,
        stages: &[Stage],
        default_sink: DefaultSink,
    ) -> Result<RowFeed, RunFailure> {
        let (chain, explicit, slot, sink_stage) = build_chain(source, category, first_stage, stages)?;
        let format = match (explicit, default_sink) {
            (Some(format), _) => Some(format),
            (None, DefaultSink::SearchText) => None,
            (None, DefaultSink::Table | DefaultSink::TypedJsonl) => Some(Format::JsonlTyped),
        };
        Ok(RowFeed { chain, category, format, first: true, slot, sink_stage, closed: false })
    }

    /// Append encoded items to `out` until it holds at least `want` bytes or the items
    /// end. `Ok(false)` means the end of the items was reached and the closing bytes of
    /// the serializer are in `out`; the chain is then already closed. An error closes
    /// the chain and names the stage that failed.
    pub fn fill(&mut self, out: &mut Vec<u8>, want: usize) -> Result<bool, RunFailure> {
        if self.closed {
            return Ok(false);
        }
        while out.len() < want {
            match self.chain.next_item() {
                Ok(Some(item)) => {
                    let encoded = match &self.format {
                        Some(format) => pipeline::encode_item(format, self.category, item, self.first, out),
                        None => search_line(item, out),
                    };
                    if let Err(error) = encoded {
                        return Err(self.fail(error.to_string()));
                    }
                    self.first = false;
                }
                Ok(None) => {
                    if let Some(format) = &self.format {
                        pipeline::finish_encoding(format, !self.first, out);
                    }
                    self.close();
                    return Ok(false);
                }
                Err(error) => return Err(self.fail(error.to_string())),
            }
        }
        Ok(true)
    }

    fn fail(&mut self, message: String) -> RunFailure {
        self.close();
        self.slot.borrow_mut().take().unwrap_or(RunFailure::new(self.sink_stage, message))
    }

    /// Release the producer chain (a closed `top` never samples again). Idempotent.
    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            self.chain.close();
        }
    }
}

impl Drop for RowFeed {
    fn drop(&mut self) {
        self.close();
    }
}

// ─── External byte stages: incremental rows ──────────────────────────────────

/// Largest chunk one stream read can return (the endpoint transfer unit).
pub const STREAM_CHUNK: usize = 512;

/// Why an external byte stage stopped feeding its consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamStop {
    /// The user interrupted the pipeline (Ctrl-C).
    Interrupted,
    /// A stream or process failure, one diagnostic line.
    Failed(String),
}

/// Incremental byte producer behind an external stage.
pub trait ByteSource {
    /// Next chunk into `out` (at most `STREAM_CHUNK` bytes); `Ok(None)` is end of stream.
    fn read_chunk(&mut self, out: &mut [u8]) -> Result<Option<usize>, StreamStop>;
    /// The consumer is done with the source. `drained` is true when end of stream
    /// was already reached; false means the producer must be stopped.
    fn close(&mut self, drained: bool);
}

enum StreamState {
    Reading,
    /// End of stream seen, buffered rows may remain.
    Ended,
    Closed,
}

/// Rows decoded from a byte stream. JSONL is decoded line by line as bytes
/// arrive, so a satisfied `take` (or an error) closes the producer without
/// waiting for it to finish; a JSON document is read whole (bounded) and decoded
/// at end of stream.
pub struct StreamRows<S: ByteSource> {
    source: S,
    format: Format,
    pending: Vec<u8>,
    rows: VecDeque<Value>,
    state: StreamState,
    source_closed: bool,
}

impl<S: ByteSource> StreamRows<S> {
    pub fn new(source: S, format: Format) -> Self {
        Self {
            source,
            format,
            pending: Vec::new(),
            rows: VecDeque::new(),
            state: StreamState::Reading,
            source_closed: false,
        }
    }

    fn lines(&self) -> bool {
        matches!(self.format, Format::Jsonl | Format::JsonlTyped)
    }

    fn push(&mut self, chunk: &[u8]) -> Result<(), PipelineError> {
        self.pending.extend_from_slice(chunk);
        if self.lines() {
            if let Some(last) = self.pending.iter().rposition(|byte| *byte == b'\n') {
                let complete: Vec<u8> = self.pending.drain(..=last).collect();
                self.rows.extend(pipeline::decode_rows(&self.format, &complete)?);
            }
            if self.pending.len() > pipeline::MAX_RECORD_LINE_BYTES {
                return Err(PipelineError::RecordTooLarge);
            }
        } else if self.pending.len() > pipeline::MAX_JSON_DOCUMENT_BYTES {
            return Err(PipelineError::Source(format!(
                "input exceeds the {} byte bound",
                pipeline::MAX_JSON_DOCUMENT_BYTES
            )));
        }
        Ok(())
    }

    /// End of stream: the final unterminated JSONL line, or the whole JSON document.
    fn end(&mut self) -> Result<(), PipelineError> {
        if !self.lines() || !self.pending.is_empty() {
            let tail = core::mem::take(&mut self.pending);
            self.rows.extend(pipeline::decode_rows(&self.format, &tail)?);
        }
        self.state = StreamState::Ended;
        Ok(())
    }

    /// Stop the producer after a decode or stream error; nothing more is pulled.
    fn abort(&mut self) {
        self.rows.clear();
        self.pending.clear();
        self.close_source(false);
        self.state = StreamState::Closed;
    }

    fn close_source(&mut self, drained: bool) {
        if !self.source_closed {
            self.source_closed = true;
            self.source.close(drained);
        }
    }
}

impl<S: ByteSource> RowSource for StreamRows<S> {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        loop {
            if let Some(row) = self.rows.pop_front() {
                return Ok(Some(Item::Row(row)));
            }
            if !matches!(self.state, StreamState::Reading) {
                return Ok(None);
            }
            let mut chunk = [0u8; STREAM_CHUNK];
            let step = match self.source.read_chunk(&mut chunk) {
                Ok(Some(count)) if count > 0 && count <= STREAM_CHUNK => self.push(&chunk[..count]),
                Ok(Some(_)) => Err(PipelineError::Source(String::from("malformed stream read"))),
                Ok(None) => self.end(),
                Err(StreamStop::Interrupted) => Err(PipelineError::Source(String::from("interrupted"))),
                Err(StreamStop::Failed(message)) => Err(PipelineError::Source(message)),
            };
            if let Err(error) = step {
                self.abort();
                return Err(error);
            }
        }
    }

    fn close(&mut self) {
        let drained = matches!(self.state, StreamState::Ended);
        self.rows.clear();
        self.pending.clear();
        self.close_source(drained);
        self.state = StreamState::Closed;
    }
}

/// Status the shell records for a stage it reaped. `exit` is the code `wait`
/// returned (`None` when the kernel refused the wait) and `reason` the kernel's
/// stop reason (0 when the stage ended by itself). A stage the kernel stopped
/// because its consumer closed (reason 3) reports 141; one stopped for any other
/// reason, or cancelled by the shell with a nonzero exit, reports 130 (unless it
/// had already exited cleanly); a refused wait on a stage nobody cancelled is 125
/// (status unavailable, never guessed).
pub fn reaped_status(exit: Option<i32>, reason: u32, cancelled: bool) -> i32 {
    if reason == CONSUMER_CLOSED_REASON {
        return UPSTREAM_NOT_STARTED_STATUS;
    }
    if reason != 0 {
        return 130;
    }
    match exit {
        Some(0) => 0,
        Some(code) if !cancelled => code,
        Some(_) => 130,
        None if cancelled => 130,
        None => 125,
    }
}

/// Kernel stop reason of a stage whose consumer closed its stream.
pub const CONSUMER_CLOSED_REASON: u32 = 3;

/// Status of an upstream stage that never ran, or was stopped, because its
/// consumer had already finished (128 + SIGPIPE equivalent). It is a pipe
/// outcome, not a failure to report as an exit.
pub const UPSTREAM_NOT_STARTED_STATUS: i32 = 141;

/// Stages start last to first. A `Cancelled` spawn of a stage that still has a
/// consumer in the segment (`index + 1 < stages`) is the kernel's final-reader-close
/// teardown: the consumer exited before this producer was committed. That is not a
/// spawn failure; the producer (and every stage before it) is simply not started and
/// the last stage decides the pipeline status. `Cancelled` for the last stage, and
/// every other spawn error, remains a failure.
pub fn spawn_cancel_means_consumer_finished(status: api::cell_session::Status, index: usize, stages: usize) -> bool {
    status == api::cell_session::Status::Cancelled && index + 1 < stages
}

/// Non-final stage diagnostics: one `NAME exited with status N` line per failed
/// stage before the last (a stage that only lost its consumer, 141, is normal pipe
/// behaviour and is not listed). Empty after a cancellation the shell caused itself.
pub fn earlier_failures(names: &[&str], statuses: &[i32], cancelled: bool) -> Vec<String> {
    if cancelled {
        return Vec::new();
    }
    let last = statuses.len().saturating_sub(1);
    statuses
        .iter()
        .zip(names)
        .take(last)
        .filter(|(status, _)| **status != 0 && **status != UPSTREAM_NOT_STARTED_STATUS)
        .map(|(status, name)| format!("{} exited with status {}", name, status))
        .collect()
}

// ─── Table renderer (display only) ───────────────────────────────────────────

fn escape_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
}

fn escape_octets(bytes: &[u8], out: &mut String) {
    match core::str::from_utf8(bytes) {
        Ok(text) => escape_text(text, out),
        Err(_) => {
            for &b in bytes {
                if b.is_ascii_graphic() || b == b' ' {
                    out.push(b as char);
                } else {
                    out.push_str(&format!("\\x{:02x}", b));
                }
            }
        }
    }
}

/// Human display of one value. Machine encodings never use this.
fn display_cell(value: &Value) -> String {
    let mut out = String::new();
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) | Value::Timestamp(n) | Value::Duration(n) => out.push_str(&n.to_string()),
        Value::UInt(n) => out.push_str(&n.to_string()),
        Value::Float(f) => out.push_str(&f.to_string()),
        Value::String(s) => escape_text(s, &mut out),
        Value::Path(p) => escape_octets(p, &mut out),
        Value::Bytes(b) => {
            out.push_str("0x");
            for byte in b.iter().take(16) {
                out.push_str(&format!("{:02x}", byte));
            }
            if b.len() > 16 {
                out.push_str(&format!("…({} bytes)", b.len()));
            }
        }
        Value::List(_) | Value::Record(_) => {
            out = pipeline::value_to_json(value, false)
                .or_else(|_| pipeline::value_to_json(value, true))
                .unwrap_or_else(|_| String::from("<value>"));
        }
    }
    out
}

fn column_index(columns: &mut Vec<String>, name: &str) -> usize {
    match columns.iter().position(|c| c == name) {
        Some(i) => i,
        None => {
            columns.push(String::from(name));
            columns.len() - 1
        }
    }
}

/// Interrupted collection versus a failure, internal to the sinks.
enum Halt {
    Interrupted,
    Failed(String),
}

fn halt(error: PipelineError) -> Halt {
    Halt::Failed(error.to_string())
}

/// Rows of a terminal table held while the column widths are unknown: this is a
/// genuine whole-set buffer, so it is bounded by rows ([`MAX_TABLE_ROWS`]), by cell
/// text ([`MAX_SINK_BYTES`]) and by its estimated heap ([`MAX_TABLE_HEAP_BYTES`]).
struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<(usize, String)>>,
    widths: Vec<usize>,
}

/// Heap one buffered table row occupies, rounded the way the cell allocator does,
/// plus its slot in the outer vector counted three times (a growing vector keeps
/// the old and the new block alive while it copies).
fn table_row_heap(cells: &Vec<(usize, String)>) -> usize {
    fn block(size: usize) -> usize {
        size.div_ceil(16) * 16
    }
    let slots = block(cells.capacity() * core::mem::size_of::<(usize, String)>());
    let text: usize = cells.iter().map(|(_, cell)| block(cell.capacity())).sum();
    slots + text + 3 * core::mem::size_of::<Vec<(usize, String)>>()
}

/// Pull rows until the end of the stream (Rows) or of the current frame (Frames, the
/// Begin already consumed), polling `interrupted` before every pull. Returns the table
/// (`None` when there was no row: no output, not even a header) and the frame End
/// that stopped it (`None` at the end of the stream).
fn collect_table(
    chain: &mut dyn RowSource,
    mut interrupted: impl FnMut() -> bool,
) -> Result<(Option<Table>, Option<FrameEnd>), Halt> {
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<(usize, String)>> = Vec::new();
    let mut payload = 0usize;
    let mut heap = 0usize;
    let stop = loop {
        if interrupted() {
            return Err(Halt::Interrupted);
        }
        let row = match chain.next_item().map_err(halt)? {
            None => break None,
            Some(Item::End(end)) => break Some(end),
            Some(Item::Row(row)) => row,
            Some(Item::Begin(_)) => return Err(Halt::Failed(String::from("frame begins inside another frame"))),
        };
        if rows.len() >= MAX_TABLE_ROWS {
            return Err(Halt::Failed(format!(
                "more than {} rows for the terminal table; end with `take N` or `to jsonl`",
                MAX_TABLE_ROWS
            )));
        }
        let mut cells = Vec::with_capacity(match &row {
            Value::Record(fields) => fields.len(),
            _ => 1,
        });
        match &row {
            Value::Record(fields) => {
                for (name, value) in fields {
                    let cell = display_cell(value);
                    payload += cell.len();
                    cells.push((column_index(&mut columns, name), cell));
                }
            }
            other => {
                let cell = display_cell(other);
                payload += cell.len();
                cells.push((column_index(&mut columns, "value"), cell));
            }
        }
        if payload > MAX_SINK_BYTES {
            return Err(Halt::Failed(format!("table exceeds the {} byte bound", MAX_SINK_BYTES)));
        }
        heap += table_row_heap(&cells);
        if heap > MAX_TABLE_HEAP_BYTES {
            return Err(Halt::Failed(format!(
                "table exceeds the {} byte memory bound; end with `take N` or `to jsonl`",
                MAX_TABLE_HEAP_BYTES
            )));
        }
        rows.push(cells);
    };
    if rows.is_empty() {
        return Ok((None, stop));
    }
    let mut widths: Vec<usize> = columns.iter().map(|c| c.chars().count()).collect();
    for cells in &rows {
        for (col, cell) in cells {
            widths[*col] = widths[*col].max(cell.chars().count());
        }
    }
    Ok((Some(Table { columns, rows, widths }), stop))
}

impl Table {
    /// One padded line (trailing blanks trimmed, newline appended) into `line`.
    fn fill<'s>(&'s self, line: &mut String, cell: impl Fn(usize) -> Option<&'s str>) {
        line.clear();
        for (col, width) in self.widths.iter().enumerate() {
            let text = cell(col).unwrap_or("");
            line.push_str(text);
            for _ in text.chars().count()..*width {
                line.push(' ');
            }
            line.push_str("  ");
        }
        let trimmed = line.trim_end().len();
        line.truncate(trimmed);
        line.push('\n');
    }

    fn header(&self, line: &mut String) {
        self.fill(line, |col| Some(self.columns[col].as_str()));
    }

    fn row(&self, index: usize, line: &mut String) {
        let cells = &self.rows[index];
        self.fill(line, |col| cells.iter().find(|(c, _)| *c == col).map(|(_, text)| text.as_str()));
    }
}

/// Destination of rendered table text: the live terminal or a bounded buffer.
trait TableOut {
    fn interrupted(&mut self) -> bool;
    fn emit(&mut self, text: &str) -> Result<(), Halt>;
}

fn write_table(table: &Table, out: &mut dyn TableOut) -> Result<(), Halt> {
    let mut line = String::new();
    table.header(&mut line);
    out.emit(&line)?;
    for index in 0..table.rows.len() {
        if out.interrupted() {
            return Err(Halt::Interrupted);
        }
        table.row(index, &mut line);
        out.emit(&line)?;
    }
    Ok(())
}

/// The line closing one displayed frame: completeness, drop count, unavailable fields.
fn frame_footer(end: &FrameEnd) -> String {
    if end.complete && end.unavailable_fields.is_empty() {
        return String::from("-- complete --\n");
    }
    let mut line = if end.complete {
        String::from("-- complete")
    } else {
        format!("-- incomplete: {} dropped", end.dropped)
    };
    if !end.unavailable_fields.is_empty() {
        line.push_str("; unavailable: ");
        line.push_str(&end.unavailable_fields.join(","));
    }
    line.push_str(" --\n");
    line
}

/// Render the default table: one aligned table for a Rows stream; for a Frames stream
/// one `-- sample N --` line, that frame's table and its footer, written at the frame's
/// End so an endless producer is never buffered whole.
fn write_tables(chain: &mut dyn RowSource, category: Category, out: &mut dyn TableOut) -> Result<(), Halt> {
    if category == Category::Rows {
        let (table, _) = collect_table(chain, || out.interrupted())?;
        return match table {
            Some(table) => write_table(&table, out),
            None => Ok(()),
        };
    }
    loop {
        if out.interrupted() {
            return Err(Halt::Interrupted);
        }
        let begin = match chain.next_item().map_err(halt)? {
            None => return Ok(()),
            Some(Item::Begin(begin)) => begin,
            Some(_) => return Err(Halt::Failed(String::from("frame item outside a frame"))),
        };
        let (table, end) = collect_table(chain, || out.interrupted())?;
        let Some(end) = end else {
            return Err(Halt::Failed(String::from("frame stream ended inside a frame")));
        };
        out.emit(&format!("-- sample {} --\n", begin.sample_id))?;
        if let Some(table) = table {
            write_table(&table, out)?;
        }
        out.emit(&frame_footer(&end))?;
    }
}

/// Bounded whole-output buffer for [`render_table`].
struct TableBuffer(Vec<u8>);

impl TableOut for TableBuffer {
    fn interrupted(&mut self) -> bool {
        false
    }

    fn emit(&mut self, text: &str) -> Result<(), Halt> {
        if self.0.len() + text.len() > MAX_SINK_BYTES {
            return Err(Halt::Failed(format!("output exceeds the {} byte bound", MAX_SINK_BYTES)));
        }
        self.0.extend_from_slice(text.as_bytes());
        Ok(())
    }
}

/// Whole table text as bytes, for the sinks that need a bounded buffer (a redirect or
/// a byte consumer). The terminal streams its tables with [`stream_rows`] instead.
fn render_table(chain: &mut dyn RowSource, category: Category) -> Result<Vec<u8>, String> {
    let mut out = TableBuffer(Vec::new());
    match write_tables(chain, category, &mut out) {
        Ok(()) => Ok(out.0),
        Err(Halt::Failed(message)) => Err(message),
        Err(Halt::Interrupted) => Err(String::from("interrupted")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use core::cell::Cell;

    /// Admission with a terminal session (external stages available).
    fn admit(stages: &[StageIn]) -> Admission {
        super::admit(stages, true)
    }

    fn stage(words: &[&str]) -> StageIn {
        StageIn { argv: words.iter().map(|w| String::from(*w)).collect(), redirects: Vec::new() }
    }

    fn with(mut s: StageIn, kind: RedirKind, path: &str) -> StageIn {
        s.redirects.push(Redir { kind, path: String::from(path) });
        s
    }

    fn rejected(stages: &[StageIn]) -> String {
        match admit(stages) {
            Admission::Reject(message) => message,
            other => panic!("expected rejection, got {:?}", other),
        }
    }

    fn record(pid: u64, state: &str) -> Value {
        Value::Record(vec![
            (String::from("pid"), Value::UInt(pid)),
            (String::from("state"), Value::String(String::from(state))),
        ])
    }

    /// Counts pulls and closes so tests can observe early stop.
    struct Counting {
        next: u64,
        limit: u64,
        pulled: Rc<Cell<u64>>,
        closed: Rc<Cell<u32>>,
    }

    impl RowSource for Counting {
        fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
            if self.next >= self.limit {
                return Ok(None);
            }
            self.next += 1;
            self.pulled.set(self.pulled.get() + 1);
            Ok(Some(Item::Row(record(self.next, "running"))))
        }
        fn close(&mut self) {
            self.closed.set(self.closed.get() + 1);
        }
    }

    fn counting(limit: u64) -> (Box<dyn RowSource>, Rc<Cell<u64>>, Rc<Cell<u32>>) {
        let pulled = Rc::new(Cell::new(0));
        let closed = Rc::new(Cell::new(0));
        let source = Counting { next: 0, limit, pulled: pulled.clone(), closed: closed.clone() };
        (Box::new(source), pulled, closed)
    }

    /// Test helper: the next item, which must be a row.
    trait NextRow {
        fn next_row(&mut self) -> Result<Option<Value>, PipelineError>;
    }

    impl<T: RowSource + ?Sized> NextRow for T {
        fn next_row(&mut self) -> Result<Option<Value>, PipelineError> {
            match self.next_item()? {
                Some(Item::Row(row)) => Ok(Some(row)),
                Some(marker) => panic!("unexpected marker {:?}", marker),
                None => Ok(None),
            }
        }
    }

    #[test]
    fn plain_byte_pipelines_stay_legacy() {
        assert!(matches!(admit(&[stage(&["echo", "x"]), stage(&["grep", "x"])]), Admission::Legacy));
        assert!(matches!(admit(&[stage(&["cd", "/"])]), Admission::Legacy));
    }

    #[test]
    fn typed_pipeline_is_admitted_with_producer_head() {
        let plan = match admit(&[
            stage(&["ps", "--records", "--text"]),
            stage(&["where", "state", "==", "\"running\""]),
            stage(&["take", "2"]),
        ]) {
            Admission::Typed(plan) => plan,
            other => panic!("{:?}", other),
        };
        assert_eq!(plan.head, Head::Producer);
        assert!(plan.text_display);
        assert_eq!(plan.producer_argv, ["ps", "--records"]);
        assert_eq!(plan.stages.len(), 2);
        assert_eq!(plan.typed_cmds, 3);
    }

    #[test]
    fn rows_into_a_byte_command_need_an_explicit_to() {
        let message = rejected(&[stage(&["ps", "--records"]), stage(&["grep", "sh"])]);
        assert!(message.contains("to json|jsonl|text"));
    }

    #[test]
    fn bytes_into_a_row_stage_need_from() {
        rejected(&[stage(&["cat", "/tmp/x"]), stage(&["take", "1"])]);
        rejected(&[stage(&["echo", "{}"]), stage(&["where", "a", "==", "1"])]);
    }

    #[test]
    fn stateful_and_session_stages_are_refused_before_effects() {
        rejected(&[stage(&["ps", "--records"]), stage(&["to", "jsonl"]), stage(&["/bin/cat"]), stage(&["cd", "/"])]);
        rejected(&[stage(&["cd", "/tmp"]), stage(&["ps", "--records"])]);
        rejected(&[stage(&["echo", "x"]), stage(&["read", "V"])]);
        rejected(&[stage(&["top"]), stage(&["grep", "x"])]);
    }

    #[test]
    fn more_than_sixteen_stages_are_refused() {
        let mut stages = vec![stage(&["ps", "--records"])];
        for _ in 0..16 {
            stages.push(stage(&["take", "1"]));
        }
        assert!(rejected(&stages).contains("at most 16"));
    }

    #[test]
    fn redirect_rules_for_typed_pipelines() {
        let ok = admit(&[
            stage(&["ps", "--records"]),
            with(stage(&["take", "1"]), RedirKind::Out, "/tmp/o"),
        ]);
        assert!(matches!(ok, Admission::Typed(TypedPlan { output: Some(_), .. })));
        rejected(&[with(stage(&["ps", "--records"]), RedirKind::Out, "/tmp/o"), stage(&["take", "1"])]);
        rejected(&[stage(&["ps", "--records"]), with(stage(&["take", "1"]), RedirKind::Err, "/tmp/e")]);
        rejected(&[with(stage(&["ps", "--records"]), RedirKind::In, "/tmp/i")]);
    }

    #[test]
    fn from_redirect_heads_a_typed_pipeline() {
        let admitted = admit(&[with(stage(&["from", "jsonl"]), RedirKind::In, "/tmp/in"), stage(&["take", "1"])]);
        assert!(matches!(admitted, Admission::Typed(TypedPlan { head: Head::FromFile(_), typed_cmds: 2, .. })));
        rejected(&[stage(&["from", "jsonl"])]);
    }

    #[test]
    fn to_feeds_builtin_byte_consumers_only() {
        let admitted = admit(&[stage(&["ps", "--records"]), stage(&["to", "text", "--field", "name"]), stage(&["grep", "sh"])]);
        assert!(matches!(admitted, Admission::Typed(TypedPlan { typed_cmds: 2, output: None, .. })));
        rejected(&[stage(&["ps", "--records"]), stage(&["to", "jsonl"]), stage(&["take", "1"])]);
    }

    #[test]
    fn background_refusals() {
        assert!(background_refusal(&stage(&["ps", "--records"])).is_some());
        assert!(background_refusal(&stage(&["cd", "/"])).is_some());
        assert!(background_refusal(&stage(&["top"])).is_some());
        assert!(background_refusal(&stage(&["httpd", "9091"])).is_none());
    }

    #[test]
    fn take_closes_the_producer_after_n_rows() {
        let (source, pulled, closed) = counting(1000);
        let out = run_rows(source, Category::Rows, 0, &[Stage::Take { n: 2, frame: false }], DefaultSink::TypedJsonl).unwrap();
        assert_eq!(pulled.get(), 2);
        assert!(closed.get() >= 1);
        assert_eq!(out.iter().filter(|b| **b == b'\n').count(), 2);
    }

    #[test]
    fn take_zero_never_pulls() {
        let (source, pulled, closed) = counting(1000);
        let out = run_rows(source, Category::Rows, 0, &[Stage::Take { n: 0, frame: false }], DefaultSink::TypedJsonl).unwrap();
        assert!(out.is_empty());
        assert_eq!(pulled.get(), 0);
        assert!(closed.get() >= 1);
    }

    #[test]
    fn table_aligns_columns_and_is_not_json() {
        let source: Box<dyn RowSource> =
            Box::new(pipeline::VecSource::new(vec![record(1, "running"), record(22, "dead")]));
        let out = run_rows(source, Category::Rows, 0, &[], DefaultSink::Table).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("pid") && lines[0].contains("state"));
        assert!(!text.contains('{'));
    }

    #[test]
    fn row_failure_is_attributed_and_returns_no_bytes() {
        let source: Box<dyn RowSource> = Box::new(pipeline::VecSource::new(vec![record(1, "x")]));
        let failure = run_rows(
            source,
            Category::Rows,
            0,
            &[Stage::Select(vec![String::from("nosuch")])],
            DefaultSink::TypedJsonl,
        )
        .unwrap_err();
        assert_eq!(failure.stage, 1);
    }

    // ── External stages ──────────────────────────────────────────────────────

    fn admit_legacy(stages: &[StageIn]) -> Admission {
        super::admit(stages, false)
    }

    #[test]
    fn external_tail_after_to_is_admitted_with_a_redirect() {
        let plan = match admit(&[
            stage(&["ps", "--records"]),
            stage(&["to", "jsonl"]),
            stage(&["/bin/cat"]),
            with(stage(&["/bin/cat"]), RedirKind::Out, "/tmp/o"),
        ]) {
            Admission::Typed(plan) => plan,
            other => panic!("{:?}", other),
        };
        assert_eq!(plan.typed_cmds, 2);
        assert_eq!(plan.tail_externals, 2);
        assert_eq!(plan.tail_output, Some((String::from("/tmp/o"), false)));
        assert!(plan.output.is_none());
    }

    #[test]
    fn external_head_must_be_followed_by_from() {
        let plan = match admit(&[stage(&["/bin/echo", "{}"]), stage(&["from", "jsonl"]), stage(&["take", "1"])]) {
            Admission::Typed(plan) => plan,
            other => panic!("{:?}", other),
        };
        assert_eq!(plan.head, Head::External { count: 1 });
        assert_eq!(plan.typed_cmds, 3);
        assert_eq!(plan.stages.len(), 2);
        assert_eq!(plan.tail_externals, 0);
        rejected(&[stage(&["/bin/echo", "{}"]), stage(&["take", "1"])]);
        rejected(&[stage(&["/bin/echo", "{}"]), stage(&["to", "json"])]);
        // An external head may feed built-in consumers after an explicit `to`.
        assert!(matches!(
            admit(&[stage(&["/bin/echo"]), stage(&["from", "json"]), stage(&["to", "text"]), stage(&["wc"])]),
            Admission::Typed(TypedPlan { typed_cmds: 3, tail_externals: 0, .. })
        ));
    }

    #[test]
    fn external_stage_cannot_both_start_and_end_a_typed_pipeline() {
        rejected(&[
            stage(&["/bin/echo", "{}"]),
            stage(&["from", "json"]),
            stage(&["to", "json"]),
            stage(&["/bin/cat"]),
        ]);
        // Rows reach an external stage only through an explicit `to`.
        rejected(&[stage(&["ps", "--records"]), stage(&["/bin/cat"])]);
        rejected(&[stage(&["ps", "--records"]), stage(&["take", "1"]), stage(&["/bin/cat"])]);
        // Built-ins and externals never mix after `to`.
        rejected(&[stage(&["ps", "--records"]), stage(&["to", "jsonl"]), stage(&["/bin/cat"]), stage(&["wc"])]);
        rejected(&[stage(&["ps", "--records"]), stage(&["to", "jsonl"]), stage(&["wc"]), stage(&["/bin/cat"])]);
    }

    #[test]
    fn external_byte_pipelines() {
        let plan = match admit(&[stage(&["/bin/echo", "x"]), with(stage(&["/bin/cat"]), RedirKind::Append, "/tmp/o")]) {
            Admission::Bytes(plan) => plan,
            other => panic!("{:?}", other),
        };
        assert_eq!(plan.head, ByteHead::None);
        assert_eq!(plan.prefix, 0);
        assert_eq!(plan.output, Some((String::from("/tmp/o"), true)));
        let plan = match admit(&[stage(&["cat", "/tmp/f"]), stage(&["/bin/cat"])]) {
            Admission::Bytes(plan) => plan,
            other => panic!("{:?}", other),
        };
        assert_eq!((plan.head, plan.prefix), (ByteHead::Cat(String::from("/tmp/f")), 1));
        let plan = match admit(&[stage(&["echo", "x"]), stage(&["grep", "x"]), stage(&["/bin/cat"])]) {
            Admission::Bytes(plan) => plan,
            other => panic!("{:?}", other),
        };
        assert_eq!((plan.head, plan.prefix), (ByteHead::Capture, 2));
        // A built-in never consumes an external stage's output.
        assert!(rejected(&[stage(&["/bin/echo", "x"]), stage(&["wc"])]).contains("external"));
        rejected(&[stage(&["/bin/cat"]), stage(&["/bin/cat"]), stage(&["grep", "x"])]);
        // A single external stage keeps today's foreground launch, unless its stdout is
        // redirected: then it is a one-stage byte pipeline into the file.
        assert!(matches!(admit(&[stage(&["/bin/cat"])]), Admission::Legacy));
        for (kind, append) in [(RedirKind::Out, false), (RedirKind::Append, true)] {
            let plan = match admit(&[with(stage(&["/bin/echo", "x"]), kind, "/tmp/x")]) {
                Admission::Bytes(plan) => plan,
                other => panic!("{:?}", other),
            };
            assert_eq!((plan.head, plan.prefix), (ByteHead::None, 0));
            assert_eq!(plan.output, Some((String::from("/tmp/x"), append)));
        }
        // Without a terminal session, and for built-ins, a redirect changes nothing.
        assert!(matches!(admit_legacy(&[with(stage(&["/bin/cat"]), RedirKind::Out, "/tmp/x")]), Admission::Legacy));
        assert!(matches!(admit(&[with(stage(&["echo", "x"]), RedirKind::Out, "/tmp/x")]), Admission::Legacy));
        // A lone `2>` keeps the foreground launch; `2>` with `>` is refused before effects.
        assert!(matches!(admit(&[with(stage(&["/bin/cat"]), RedirKind::Err, "/tmp/e")]), Admission::Legacy));
        rejected(&[with(with(stage(&["/bin/cat"]), RedirKind::Out, "/tmp/x"), RedirKind::Err, "/tmp/e")]);
    }

    #[test]
    fn external_pipelines_refuse_bad_forms_before_effects() {
        rejected(&[stage(&["/bin/cat"]), stage(&["cd", "/"])]);
        rejected(&[stage(&["/bin/cat"]), stage(&["top"])]);
        rejected(&[stage(&["echo", "x"]), stage(&["/bin/cat"]), with(stage(&["/bin/cat"]), RedirKind::Err, "/tmp/e")]);
        rejected(&[with(stage(&["/bin/cat"]), RedirKind::In, "/tmp/i"), stage(&["/bin/cat"])]);
        rejected(&[with(stage(&["/bin/cat"]), RedirKind::Out, "/tmp/o"), stage(&["/bin/cat"])]);
        rejected(&[with(stage(&["cat", "/tmp/f"]), RedirKind::In, "/tmp/i"), stage(&["/bin/cat"])]);
        // `<` stays valid on a built-in producer that is captured.
        assert!(matches!(
            admit(&[with(stage(&["wc"]), RedirKind::In, "/tmp/i"), stage(&["/bin/cat"])]),
            Admission::Bytes(_)
        ));
        let long = "x".repeat(600);
        assert!(rejected(&[stage(&["/bin/echo", &long]), stage(&["/bin/cat"])]).contains("512-byte"));
        assert!(rejected(&[stage(&["ps", "--records"]), stage(&["to", "jsonl"]), stage(&["/bin/echo", &long])])
            .contains("512-byte"));
        rejected(&[stage(&["/bin/echo", "a\0b"]), stage(&["/bin/cat"])]);
    }

    #[test]
    fn externals_stay_refused_without_a_terminal_session() {
        let message = match admit_legacy(&[stage(&["ps", "--records"]), stage(&["to", "jsonl"]), stage(&["/bin/cat"])]) {
            Admission::Reject(message) => message,
            other => panic!("{:?}", other),
        };
        assert!(message.contains("terminal session"));
        assert!(matches!(
            admit_legacy(&[stage(&["/bin/echo", "{}"]), stage(&["from", "json"])]),
            Admission::Reject(_)
        ));
        // Plain byte pipelines keep the legacy composition there.
        assert!(matches!(admit_legacy(&[stage(&["/bin/echo", "x"]), stage(&["/bin/cat"])]), Admission::Legacy));
    }

    #[test]
    fn argv_limit_matches_the_child_context_bound() {
        let word = |len: usize| "a".repeat(len);
        // 7 framing bytes + (len + 1) per argument: 504 + 1 + 7 == 512 fits, one more does not.
        assert!(argv_fits(&[word(504)]));
        assert!(!argv_fits(&[word(505)]));
        assert!(argv_fits(&[]));
    }

    #[test]
    fn upstream_cancel_at_spawn_is_a_finished_consumer() {
        use api::cell_session::Status;
        assert!(spawn_cancel_means_consumer_finished(Status::Cancelled, 0, 2));
        assert!(spawn_cancel_means_consumer_finished(Status::Cancelled, 1, 3));
        // The last stage has no consumer in the segment, and other errors stay failures.
        assert!(!spawn_cancel_means_consumer_finished(Status::Cancelled, 1, 2));
        assert!(!spawn_cancel_means_consumer_finished(Status::Cancelled, 0, 1));
        assert!(!spawn_cancel_means_consumer_finished(Status::NotFound, 0, 2));
        assert!(!spawn_cancel_means_consumer_finished(Status::Denied, 0, 2));
        assert_eq!(UPSTREAM_NOT_STARTED_STATUS, 141);
    }

    #[test]
    fn reaped_statuses() {
        assert_eq!(reaped_status(Some(0), 0, false), 0);
        assert_eq!(reaped_status(Some(3), 0, false), 3);
        assert_eq!(reaped_status(Some(0), 0, true), 0);
        assert_eq!(reaped_status(Some(9), 0, true), 130);
        assert_eq!(reaped_status(None, 0, true), 130);
        assert_eq!(reaped_status(None, 0, false), 125);
        // A stage the kernel stopped: consumer closed is 141, any other reason 130.
        assert_eq!(reaped_status(Some(0), CONSUMER_CLOSED_REASON, false), 141);
        assert_eq!(reaped_status(Some(0), CONSUMER_CLOSED_REASON, true), 141);
        assert_eq!(reaped_status(Some(0), 2, false), 130);
        assert_eq!(reaped_status(Some(0), 5, true), 130);
    }

    #[test]
    fn earlier_stage_failures_are_listed_unless_the_shell_cancelled() {
        let names = ["a", "b", "c"];
        assert_eq!(earlier_failures(&names, &[1, 0, 2], false), ["a exited with status 1"]);
        assert!(earlier_failures(&names, &[1, 1, 1], true).is_empty());
        assert!(earlier_failures(&names, &[0, 0, 0], false).is_empty());
        // A stage that only lost its consumer (141) is normal pipe behaviour.
        assert_eq!(earlier_failures(&names, &[141, 7, 0], false), ["b exited with status 7"]);
    }

    /// Endless record source that counts pulls and closes.
    fn endless_rows() -> (Box<dyn RowSource>, Rc<Cell<u64>>, Rc<Cell<u32>>) {
        struct Endless {
            next: u64,
            pulled: Rc<Cell<u64>>,
            closed: Rc<Cell<u32>>,
        }
        impl RowSource for Endless {
            fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
                self.next += 1;
                self.pulled.set(self.next);
                Ok(Some(Item::Row(record(self.next, "running"))))
            }
            fn close(&mut self) {
                self.closed.set(self.closed.get() + 1);
            }
        }
        let pulled = Rc::new(Cell::new(0));
        let closed = Rc::new(Cell::new(0));
        (Box::new(Endless { next: 0, pulled: pulled.clone(), closed: closed.clone() }), pulled, closed)
    }

    #[test]
    fn row_feed_pulls_only_what_the_consumer_takes() {
        let (source, pulled, closed) = endless_rows();
        let mut feed = RowFeed::new(source, Category::Rows, 0, &[Stage::To(Format::Jsonl)], DefaultSink::TypedJsonl).unwrap();
        assert_eq!(pulled.get(), 0, "constructing the feed pulls nothing");
        let mut out = Vec::new();
        assert!(feed.fill(&mut out, 100).unwrap());
        let first_fill = pulled.get();
        assert!(out.len() >= 100 && first_fill < 10, "one bounded fill, not the whole stream");
        assert!(out.ends_with(b"\n"));
        out.clear();
        assert!(feed.fill(&mut out, 100).unwrap());
        assert!(pulled.get() > first_fill);
        assert_eq!(closed.get(), 0);
        feed.close();
        feed.close();
        assert_eq!(closed.get(), 1, "closing is idempotent and stops the producer");
        assert_eq!(feed.fill(&mut out, 100), Ok(false));
        drop(feed);
        assert_eq!(closed.get(), 1);
    }

    /// S10-3b: an endless producer under a Collected run stops at the first interrupt,
    /// for every sink kind: no bytes, the chain closed, status 130.
    #[test]
    fn a_collected_run_stops_at_the_interrupt_and_returns_nothing() {
        for (stages, sink) in [
            (vec![Stage::To(Format::Jsonl)], DefaultSink::Table),
            (vec![], DefaultSink::TypedJsonl),
            (vec![], DefaultSink::Table),
        ] {
            let (source, pulled, closed) = endless_rows();
            let polls = Cell::new(0u64);
            let mut interrupted = || {
                polls.set(polls.get() + 1);
                polls.get() > 5
            };
            let failure = run_rows_within(source, Category::Rows, 0, &stages, sink, COLLECT_BOUND, &mut interrupted)
                .unwrap_err();
            assert_eq!(failure.status, INTERRUPTED_STATUS);
            assert_eq!(pulled.get(), 5, "no pull after the interrupt");
            assert_eq!(closed.get(), 1, "the producer is released");
        }
        // Without an interrupt the same producer runs to the bound (unchanged behaviour).
        let (source, _, closed) = endless_rows();
        let bound = CollectBound { bytes: 1024, status: 1 };
        let failure = run_rows_within(source, Category::Rows, 0, &[], DefaultSink::TypedJsonl, bound, &mut || false)
            .unwrap_err();
        assert_eq!((failure.status, closed.get()), (1, 1));
    }

    #[test]
    fn row_feed_streams_the_same_bytes_as_run_rows() {
        for format in [Format::Jsonl, Format::JsonlTyped, Format::Json] {
            let rows = || -> Box<dyn RowSource> {
                Box::new(pipeline::VecSource::new(vec![record(1, "running"), record(22, "dead"), record(3, "x")]))
            };
            let stages = [Stage::Take { n: 2, frame: false }, Stage::To(format.clone())];
            let whole = run_rows(rows(), Category::Rows, 0, &stages, DefaultSink::TypedJsonl).unwrap();
            let mut feed = RowFeed::new(rows(), Category::Rows, 0, &stages, DefaultSink::TypedJsonl).unwrap();
            let mut streamed = Vec::new();
            let mut out = Vec::new();
            loop {
                let more = feed.fill(&mut out, 1).unwrap();
                streamed.extend_from_slice(&out);
                out.clear();
                if !more {
                    break;
                }
            }
            assert_eq!(streamed, whole, "{:?}", format);
        }
        // Zero rows still end a `json` document.
        let mut feed = RowFeed::new(Box::new(pipeline::VecSource::new(Vec::new())), Category::Rows, 0, &[Stage::To(Format::Json)], DefaultSink::TypedJsonl).unwrap();
        let mut out = Vec::new();
        assert_eq!(feed.fill(&mut out, 16), Ok(false));
        assert_eq!(out, b"[]\n");
    }

    /// Terminal stand-in: records every write, reports Ctrl-C once `interrupt_after`
    /// writes have been delivered.
    struct RecordingSink {
        bytes: Vec<u8>,
        writes: usize,
        largest: usize,
        interrupt_after: Option<usize>,
    }

    impl RecordingSink {
        fn new(interrupt_after: Option<usize>) -> Self {
            Self { bytes: Vec::new(), writes: 0, largest: 0, interrupt_after }
        }
    }

    impl ChunkSink for RecordingSink {
        fn write(&mut self, bytes: &[u8]) -> Result<(), SinkStop> {
            self.writes += 1;
            self.largest = self.largest.max(bytes.len());
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }
        fn interrupted(&mut self) -> bool {
            self.interrupt_after.is_some_and(|limit| self.writes >= limit)
        }
    }

    #[test]
    fn endless_terminal_stream_is_live_bounded_and_interruptible() {
        for stages in [vec![Stage::To(Format::Jsonl)], Vec::new()] {
            let default_sink = if stages.is_empty() { DefaultSink::TypedJsonl } else { DefaultSink::Table };
            let (source, pulled, closed) = endless_rows();
            let mut sink = RecordingSink::new(Some(5_000));
            let end = stream_rows(source, Category::Rows, 0, &stages, default_sink, &mut sink).unwrap();
            assert_eq!(end.status(), 130);
            assert_eq!(closed.get(), 1, "Ctrl-C closes the producer");
            // One write per pulled row: each row reaches the terminal before the next pull.
            assert_eq!(sink.writes as u64, pulled.get());
            assert!(sink.largest < 128, "one row per write, got {}", sink.largest);
            assert_eq!(sink.bytes.iter().filter(|b| **b == b'\n').count(), 5_000);
        }
    }

    #[test]
    fn terminal_stream_matches_collected_output() {
        let rows = || -> Box<dyn RowSource> {
            Box::new(pipeline::VecSource::new(vec![record(1, "running"), record(22, "dead"), record(3, "x")]))
        };
        let cases: [(&[Stage], DefaultSink); 5] = [
            (&[Stage::To(Format::Jsonl)], DefaultSink::Table),
            (&[Stage::To(Format::Json)], DefaultSink::Table),
            (&[Stage::Take { n: 2, frame: false }], DefaultSink::TypedJsonl),
            (&[], DefaultSink::Table),
            (&[Stage::Take { n: 0, frame: false }, Stage::To(Format::Json)], DefaultSink::Table),
        ];
        for (stages, default_sink) in cases {
            let mut whole = run_rows(rows(), Category::Rows, 0, stages, default_sink).unwrap();
            if whole.last().is_some_and(|byte| *byte != b'\n') {
                whole.push(b'\n');
            }
            let mut sink = RecordingSink::new(None);
            assert_eq!(stream_rows(rows(), Category::Rows, 0, stages, default_sink, &mut sink), Ok(StreamEnd::Complete));
            assert_eq!(sink.bytes, whole, "{:?}", stages);
        }
    }

    #[test]
    fn collected_sink_still_refuses_past_its_bound() {
        let (source, _, closed) = endless_rows();
        let failure = run_rows(source, Category::Rows, 0, &[Stage::To(Format::Jsonl)], DefaultSink::Table).unwrap_err();
        assert_eq!(failure.message, format!("output exceeds the {} byte bound", MAX_SINK_BYTES));
        assert_eq!(closed.get(), 1);
    }

    #[test]
    fn row_feed_failure_names_its_stage_and_closes_the_producer() {
        let (source, _, closed) = endless_rows();
        let stages = [Stage::Select(vec![String::from("nosuchfield")]), Stage::To(Format::Jsonl)];
        let mut feed = RowFeed::new(source, Category::Rows, 0, &stages, DefaultSink::TypedJsonl).unwrap();
        let mut out = Vec::new();
        let failure = feed.fill(&mut out, 64).unwrap_err();
        assert_eq!(failure.stage, 1);
        assert!(failure.message.contains("nosuchfield"), "{}", failure.message);
        assert_eq!(closed.get(), 1);
    }

    // ── Frames (S6-3) ────────────────────────────────────────────────────────

    /// Endless Frames producer: `rows` rows per frame. Counts items pulled and closes.
    fn endless_frames(rows: u64) -> (Box<dyn RowSource>, Rc<Cell<u64>>, Rc<Cell<u32>>) {
        struct Frames {
            rows: u64,
            sample: u64,
            pos: u64,
            pulled: Rc<Cell<u64>>,
            closed: Rc<Cell<u32>>,
        }
        impl RowSource for Frames {
            fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
                if self.closed.get() > 0 {
                    return Ok(None);
                }
                self.pulled.set(self.pulled.get() + 1);
                let pos = self.pos;
                self.pos = if pos == self.rows + 1 { 0 } else { pos + 1 };
                Ok(Some(match pos {
                    0 => {
                        self.sample += 1;
                        let meta = Value::Record(vec![(String::from("schema"), Value::String(String::from("t.v1")))]);
                        Item::Begin(pipeline::FrameBegin { sample_id: self.sample, monotonic_ns: self.sample * 7, meta: Some(meta) })
                    }
                    p if p <= self.rows => Item::Row(record(self.sample * 100 + p, "running")),
                    _ => Item::End(FrameEnd { complete: false, unavailable_fields: vec![String::from("cpu_tenths")], dropped: 2 }),
                }))
            }
            fn close(&mut self) {
                self.closed.set(self.closed.get() + 1);
            }
        }
        let pulled = Rc::new(Cell::new(0));
        let closed = Rc::new(Cell::new(0));
        let source = Frames { rows, sample: 0, pos: 0, pulled: pulled.clone(), closed: closed.clone() };
        (Box::new(source), pulled, closed)
    }

    fn take_frames(n: usize) -> Stage {
        Stage::Take { n, frame: true }
    }

    #[test]
    fn producer_category_is_decided_by_name() {
        let argv = |words: &[&str]| -> Vec<String> { words.iter().map(|w| String::from(*w)).collect() };
        assert_eq!(crate::native_rows::producer_category(&argv(&["top", "--records"])), Category::Frames);
        assert_eq!(crate::native_rows::producer_category(&argv(&["ps", "--records"])), Category::Rows);
        assert_eq!(crate::native_rows::producer_category(&argv(&["ls", "--records"])), Category::Rows);
    }

    #[test]
    fn frames_admission_refuses_rows_only_stages_before_any_producer() {
        // Admission is pure: a refusal here means no producer is ever constructed or pulled.
        let top = || stage(&["top", "--records"]);
        assert!(rejected(&[top(), stage(&["take", "2"])]).contains("--frame"));
        assert!(rejected(&[top(), stage(&["sort-by", "cpu_tenths"])]).contains("--frame"));
        rejected(&[top(), stage(&["to", "json"])]);
        rejected(&[top(), stage(&["to", "text", "--field", "name"])]);
        rejected(&[top(), take_stage_frame(1), stage(&["to", "json", "--typed"])]);
        rejected(&[stage(&["ps", "--records"]), take_stage_frame(1)]);
        let plan = match admit(&[top(), take_stage_frame(2), stage(&["sort-by", "--frame", "cpu_tenths"]), stage(&["to", "jsonl"])]) {
            Admission::Typed(plan) => plan,
            other => panic!("{:?}", other),
        };
        assert_eq!(plan.category, Category::Frames);
        let plan = match admit(&[stage(&["ps", "--records"]), stage(&["take", "1"])]) {
            Admission::Typed(plan) => plan,
            other => panic!("{:?}", other),
        };
        assert_eq!(plan.category, Category::Rows);
    }

    fn take_stage_frame(n: usize) -> StageIn {
        let n = n.to_string();
        stage(&["take", "--frame", n.as_str()])
    }

    #[test]
    fn row_feed_and_run_rows_encode_frames_identically() {
        for format in [Format::Jsonl, Format::JsonlTyped] {
            let stages = [take_frames(3), Stage::To(format.clone())];
            let (source, _, closed) = endless_frames(2);
            let whole = run_rows(source, Category::Frames, 0, &stages, DefaultSink::Table).unwrap();
            assert_eq!(closed.get(), 1);
            let (source, _, _) = endless_frames(2);
            let mut feed = RowFeed::new(source, Category::Frames, 0, &stages, DefaultSink::TypedJsonl).unwrap();
            let mut streamed = Vec::new();
            loop {
                let want = streamed.len() + 7;
                if !feed.fill(&mut streamed, want).unwrap() {
                    break;
                }
            }
            assert_eq!(streamed, whole, "{:?}", format);
            assert_eq!(whole.iter().filter(|b| **b == b'\n').count(), 3 * 4);
        }
    }

    #[test]
    fn redirect_default_for_frames_is_typed_envelopes() {
        let (source, _, _) = endless_frames(1);
        let out = run_rows(source, Category::Frames, 0, &[take_frames(2)], DefaultSink::TypedJsonl).unwrap();
        let envelopes = pipeline::decode_jsonl_typed(&out).unwrap();
        let kinds: Vec<&Value> = envelopes.iter().map(|e| e.get_field("frame").unwrap()).collect();
        let s = |k: &str| Value::String(String::from(k));
        assert_eq!(kinds, [&s("begin"), &s("row"), &s("end"), &s("begin"), &s("row"), &s("end")]);
        assert_eq!(envelopes[2].get_field("dropped"), Some(&Value::UInt(2)));
        assert_eq!(envelopes[2].get_field("complete"), Some(&Value::Bool(false)));
    }

    /// Records how many items had been pulled when each write reached the terminal.
    struct PullTrackingSink {
        pulled: Rc<Cell<u64>>,
        writes: Vec<(u64, Vec<u8>)>,
    }

    impl ChunkSink for PullTrackingSink {
        fn write(&mut self, bytes: &[u8]) -> Result<(), SinkStop> {
            self.writes.push((self.pulled.get(), bytes.to_vec()));
            Ok(())
        }
        fn interrupted(&mut self) -> bool {
            false
        }
    }

    #[test]
    fn terminal_table_streams_one_frame_at_a_time() {
        let (source, pulled, closed) = endless_frames(3);
        let mut sink = PullTrackingSink { pulled: pulled.clone(), writes: Vec::new() };
        let end = stream_rows(source, Category::Frames, 0, &[take_frames(4)], DefaultSink::Table, &mut sink).unwrap();
        assert_eq!(end, StreamEnd::Complete);
        assert_eq!(closed.get(), 1);
        // Every frame (Begin + 3 rows + End = 5 items) is written at its End, before the
        // next frame is pulled: the first write sees exactly 5 pulls, never the whole stream.
        let per_frame = 5;
        let mut frames_seen: Vec<u64> = sink.writes.iter().map(|(p, _)| *p).collect();
        frames_seen.dedup();
        assert_eq!(frames_seen, [per_frame, 2 * per_frame, 3 * per_frame, 4 * per_frame]);
        let text: Vec<u8> = sink.writes.into_iter().flat_map(|(_, bytes)| bytes).collect();
        let text = String::from_utf8(text).unwrap();
        // Per frame: a sample line, a header, three rows and a completeness line; no JSON.
        assert_eq!(text.lines().count(), 4 * 6);
        assert!(!text.contains('{'));
        for frame in 1..=4u64 {
            for row in 1..=3u64 {
                assert!(text.contains(&(frame * 100 + row).to_string()));
            }
        }
        // The collected (`--text` redirect) form is the same text.
        let (source, _, _) = endless_frames(3);
        let whole = run_rows(source, Category::Frames, 0, &[take_frames(4)], DefaultSink::Table).unwrap();
        assert_eq!(whole, text.as_bytes());
    }

    #[test]
    fn frames_out_of_order_fail_at_the_producer_stage() {
        let source: Box<dyn RowSource> = Box::new(pipeline::VecSource::new(vec![record(1, "x")]));
        let failure = run_rows(source, Category::Frames, 0, &[take_frames(1)], DefaultSink::TypedJsonl).unwrap_err();
        assert_eq!(failure.stage, 0);
    }

    /// Scripted chunks, then end of stream (or an endless repeat of one chunk).
    struct Scripted {
        chunks: VecDeque<Vec<u8>>,
        endless: Option<Vec<u8>>,
        reads: Rc<Cell<u32>>,
        closed: Rc<RefCell<Vec<bool>>>,
        fail_after: Option<StreamStop>,
    }

    impl ByteSource for Scripted {
        fn read_chunk(&mut self, out: &mut [u8]) -> Result<Option<usize>, StreamStop> {
            self.reads.set(self.reads.get() + 1);
            let chunk = match self.chunks.pop_front() {
                Some(chunk) => chunk,
                None => match (&self.endless, &self.fail_after) {
                    (_, Some(stop)) => return Err(stop.clone()),
                    (Some(chunk), None) => chunk.clone(),
                    (None, None) => return Ok(None),
                },
            };
            out[..chunk.len()].copy_from_slice(&chunk);
            Ok(Some(chunk.len()))
        }
        fn close(&mut self, drained: bool) {
            self.closed.borrow_mut().push(drained);
        }
    }

    fn scripted(chunks: &[&[u8]]) -> (Scripted, Rc<Cell<u32>>, Rc<RefCell<Vec<bool>>>) {
        let reads = Rc::new(Cell::new(0));
        let closed = Rc::new(RefCell::new(Vec::new()));
        let source = Scripted {
            chunks: chunks.iter().map(|chunk| chunk.to_vec()).collect(),
            endless: None,
            reads: reads.clone(),
            closed: closed.clone(),
            fail_after: None,
        };
        (source, reads, closed)
    }

    fn pull_all(rows: &mut dyn RowSource) -> Result<Vec<Value>, PipelineError> {
        let mut out = Vec::new();
        while let Some(row) = rows.next_row()? {
            out.push(row);
        }
        Ok(out)
    }

    #[test]
    fn jsonl_lines_split_across_chunks_decode_incrementally() {
        let (source, _, closed) = scripted(&[b"{\"a\":1}\n{\"a\"", b":2}\n{\"a\":3}"]);
        let mut rows = StreamRows::new(source, Format::Jsonl);
        let all = pull_all(&mut rows).unwrap();
        assert_eq!(all.len(), 3);
        rows.close();
        assert_eq!(*closed.borrow(), [true]);
    }

    #[test]
    fn take_stops_an_endless_producer() {
        let (mut source, reads, closed) = scripted(&[]);
        source.endless = Some(b"{\"a\":1}\n".to_vec());
        let rows: Box<dyn RowSource> = Box::new(StreamRows::new(source, Format::Jsonl));
        let out = run_rows(rows, Category::Rows, 0, &[Stage::Take { n: 2, frame: false }], DefaultSink::TypedJsonl).unwrap();
        assert_eq!(out.iter().filter(|b| **b == b'\n').count(), 2);
        // Two rows needed two reads; the producer was told it was not drained.
        assert_eq!(reads.get(), 2);
        assert_eq!(*closed.borrow(), [false]);
    }

    #[test]
    fn malformed_line_aborts_and_closes_the_producer() {
        let (source, _, closed) = scripted(&[b"{\"a\":1}\nnot json\n"]);
        let mut rows = StreamRows::new(source, Format::Jsonl);
        let first = rows.next_row();
        // The whole chunk decodes at once; the error is raised before any row is handed out.
        assert!(first.is_err());
        assert!(rows.next_row().unwrap().is_none());
        assert_eq!(*closed.borrow(), [false]);
    }

    #[test]
    fn stream_failure_and_interrupt_surface_as_row_errors() {
        let (mut source, _, closed) = scripted(&[]);
        source.fail_after = Some(StreamStop::Interrupted);
        let mut rows = StreamRows::new(source, Format::Jsonl);
        assert_eq!(rows.next_row().unwrap_err().to_string(), "interrupted");
        assert_eq!(*closed.borrow(), [false]);
        let (mut source, _, _) = scripted(&[]);
        source.fail_after = Some(StreamStop::Failed(String::from("stream broke")));
        let mut rows = StreamRows::new(source, Format::Jsonl);
        assert_eq!(rows.next_row().unwrap_err().to_string(), "stream broke");
    }

    #[test]
    fn json_document_is_decoded_at_end_of_stream() {
        let (source, reads, _) = scripted(&[b"[{\"a\":1},", b"{\"a\":2}]"]);
        let mut rows = StreamRows::new(source, Format::Json);
        assert_eq!(pull_all(&mut rows).unwrap().len(), 2);
        assert_eq!(reads.get(), 3);
        let (source, _, _) = scripted(&[]);
        let mut empty = StreamRows::new(source, Format::Json);
        assert!(empty.next_row().is_err());
    }

    #[test]
    fn overlong_unterminated_line_is_refused() {
        let (mut source, _, closed) = scripted(&[]);
        source.endless = Some(vec![b'a'; STREAM_CHUNK]);
        let mut rows = StreamRows::new(source, Format::Jsonl);
        assert_eq!(rows.next_row().unwrap_err(), PipelineError::RecordTooLarge);
        assert_eq!(*closed.borrow(), [false]);
    }

    #[test]
    fn empty_stream_yields_no_jsonl_rows() {
        let (source, _, closed) = scripted(&[]);
        let mut rows = StreamRows::new(source, Format::Jsonl);
        assert!(rows.next_row().unwrap().is_none());
        rows.close();
        assert_eq!(*closed.borrow(), [true]);
    }
}
