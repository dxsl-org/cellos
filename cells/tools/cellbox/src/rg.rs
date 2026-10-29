//! `rg`: ripgrep-derived search over the Cellos VFS as a typed row producer
//! (Phase 07 S7-3, `phase-07-s3-proposal.md`).
//!
//! * [`args`] parses the frozen argv surface at admission (status 2 before any
//!   effect);
//! * [`producer`] / [`stdin_producer`] compile the matcher and globs and return a
//!   pull-based [`RowSource`] of `cellbox.search.line` records. Nothing touches
//!   the VFS until the first pull;
//! * `tree` adapts `ostd::clients::VfsClient` to `vigrep::walk::ViSearchTree`
//!   (per-directory reopen with one cached parent handle);
//! * `source` drives `vigrep`'s resumable file search one bounded segment per
//!   pull (8 rows / 32 KiB queued at most), reports per-entry diagnostics and
//!   one end summary, and exposes the 0/1/2 completion status;
//! * [`text`] is the streaming plain-text renderer with terminal-safe escaping.
//!
//! Memory: [`RG_PROFILE`] tightens the contract ceilings so a search fits the
//! cellbox arena next to the shell (DV7-10, D76-3 M2); the bounds are measured by the
//! host heap test in `tests.rs`.

pub mod args;
mod source;
pub mod text;
mod tree;

#[cfg(test)]
mod tests;

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use text_engine::pipeline::{InterruptLatch, RowSource};
use vigrep::ignore::overrides::OverrideBuilder;
use vigrep::regex::{RegexMatcher, RegexMatcherBuilder};
use vigrep::searcher::{Searcher, SearcherBuilder};
use vigrep::walk::{BinaryMode, WalkBuilder, WalkLimits};

use crate::typed::ByteSource;
pub use args::RgArgs;
pub use source::Limits;

/// Search resource bounds (DV7-10). Contract numbers are ceilings; these are the
/// values that fit the cellbox arena (proposal §6, D76-3 M2).
#[derive(Debug, Clone, Copy)]
pub struct RgProfile {
    /// Compiled regex (NFA) bound.
    pub regex_size_limit: usize,
    /// Lazy-DFA cache bound.
    pub dfa_size_limit: usize,
    /// Searcher read buffer.
    pub buffer_capacity: usize,
    /// Searcher buffer growth bound: capacity plus the 65,536-byte line limit.
    pub heap_limit: usize,
    /// Row queue and submatch bounds.
    pub limits: Limits,
    /// Resume window of a streamed stdin (an external head cannot rewind).
    pub stdin_window: usize,
    /// Walker bounds.
    pub walk: WalkLimits,
}

/// The profile every `rg` uses.
pub const RG_PROFILE: RgProfile = RgProfile {
    regex_size_limit: 128 * 1024,
    dfa_size_limit: 64 * 1024,
    buffer_capacity: 16 * 1024,
    heap_limit: 16 * 1024 + 65_536,
    limits: Limits { queue_rows: 8, queue_bytes: 32 * 1024, max_submatches: 4096 },
    stdin_window: 96 * 1024,
    walk: WalkLimits {
        max_depth: 64,
        max_entries: 65_536,
        max_dir_entries: 4_096,
        max_pending_bytes: 96 * 1024,
        max_pending_entries: 8_192,
        max_path_len: 1_024,
        max_ignore_file_bytes: 16 * 1024,
        max_ignore_rules_per_file: 256,
        max_ignore_bytes_total: 32 * 1024,
        max_ignore_rules_total: 65_536,
        scratch_bytes: 16 * 1024,
        resume_window_bytes: 32 * 1024,
    },
};

/// Bytes of an `rg -` input.
pub enum StdinInput {
    /// Whole bytes of an in-memory head (`cat`, a captured built-in, `< FILE`).
    Bytes(Vec<u8>),
    /// The stdout of an external head, read as the search consumes it.
    Stream(Box<dyn ByteSource>),
}

/// Print `rg: MESSAGE` on stderr.
fn usage_error(message: &str) -> i32 {
    crate::executor::shell_stderr("rg: ");
    crate::executor::shell_stderrln(message);
    2
}

/// The diagnostic sink of a running search (stderr).
fn stderr_report() -> source::Report {
    Box::new(|line: &str| crate::executor::shell_stderrln(line))
}

/// Matcher for `args` with the profile's size limits, configured as upstream rg
/// configures its Rust regex engine (`hiargs.rs` `matcher_rust`): multi-line
/// syntax so `^`/`$` see line boundaries, LF as the banned line terminator, NUL
/// banned unless `-a`, patterns joined leftmost-first in argument order.
pub(crate) fn build_matcher(args: &RgArgs, profile: &RgProfile) -> Result<RegexMatcher, String> {
    let mut builder = RegexMatcherBuilder::new();
    builder
        .multi_line(true)
        .unicode(true)
        .octal(false)
        .dot_matches_new_line(false)
        .line_terminator(Some(b'\n'))
        .case_insensitive(args.ignore_case)
        .fixed_strings(args.fixed)
        .size_limit(profile.regex_size_limit)
        .dfa_size_limit(profile.dfa_size_limit);
    if args.word {
        builder.word(true);
    }
    if args.line {
        builder.whole_line(true);
    }
    if !args.search_binary {
        builder.ban_byte(Some(0));
    }
    builder.build_many(&args.patterns).map_err(|error| format!("regex: {}", error))
}

pub(crate) fn build_searcher(args: &RgArgs, profile: &RgProfile) -> Searcher {
    SearcherBuilder::new()
        .line_number(true)
        .before_context(args.before)
        .after_context(args.after)
        .buffer_capacity(profile.buffer_capacity)
        .heap_limit(Some(profile.heap_limit))
        .build()
}

/// The walker over `roots` (canonical absolute paths); `cwd` anchors `-g` globs.
pub(crate) fn build_walk(
    args: &RgArgs,
    roots: &[Vec<u8>],
    cwd: &[u8],
    profile: &RgProfile,
) -> Result<WalkBuilder, String> {
    let mut overrides = OverrideBuilder::new(cwd);
    for glob in &args.globs {
        overrides.add(glob).map_err(|error| format!("glob: {}", error))?;
    }
    let overrides = overrides.build().map_err(|error| format!("glob: {}", error))?;
    let mut walk = WalkBuilder::new(roots);
    walk.hidden(args.hidden)
        .ignore_files(!args.no_ignore)
        .overrides(overrides)
        .binary(if args.search_binary { BinaryMode::Search } else { BinaryMode::Skip })
        .limits(profile.walk);
    Ok(walk)
}

/// Search roots: every path resolved against the shell cwd, or the cwd itself
/// with no path. A cwd that cannot be read fails the search; `/` is never used
/// as a stand-in. Also returns the glob base (the cwd, `/` only when no glob and
/// no relative path needs it).
fn roots(args: &RgArgs) -> Result<(Vec<Vec<u8>>, Vec<u8>), String> {
    let needs_cwd = args.paths.is_empty()
        || !args.globs.is_empty()
        || args.paths.iter().any(|path| !path.starts_with('/'));
    let cwd = if needs_cwd {
        let cwd = crate::cmd_cwd::get_shell_cwd()
            .map_err(|_| String::from("cannot read the current directory"))?;
        crate::cmd_fs::resolve_path_from(&cwd, "")
    } else {
        String::from("/")
    };
    let roots = if args.paths.is_empty() {
        alloc::vec![cwd.clone().into_bytes()]
    } else {
        args.paths.iter().map(|path| crate::cmd_fs::resolve_path_from(&cwd, path).into_bytes()).collect()
    };
    Ok((roots, cwd.into_bytes()))
}

/// Tree-mode producer for parsed `args` (`rg PATTERN [PATH…]`). Compile errors
/// print one `rg: regex: …`/`rg: glob: …` line and return status 2 before any
/// VFS request. `-h` prints the usage and yields nothing (completion 0).
pub fn producer(args: &RgArgs, latch: &Rc<InterruptLatch>) -> Result<Box<dyn RowSource>, i32> {
    if args.help {
        crate::executor::shell_println(args::USAGE);
        return Ok(Box::new(source::Empty));
    }
    let profile = &RG_PROFILE;
    let matcher = build_matcher(args, profile).map_err(|message| usage_error(&message))?;
    let (roots, cwd) = roots(args).map_err(|message| usage_error(&message))?;
    let walk = build_walk(args, &roots, &cwd, profile).map_err(|message| usage_error(&message))?;
    let searcher = build_searcher(args, profile);
    Ok(Box::new(source::TreeSource::new(
        tree::VfsTree::new(),
        walk.build(),
        searcher,
        matcher,
        args.quiet,
        profile.limits,
        latch.clone(),
        stderr_report(),
    )))
}

/// Stdin-mode producer (`… | rg -a PATTERN -`): one search over `input`.
pub fn stdin_producer(
    args: &RgArgs,
    input: StdinInput,
    latch: &Rc<InterruptLatch>,
) -> Result<Box<dyn RowSource>, i32> {
    let profile = &RG_PROFILE;
    let matcher = match build_matcher(args, profile) {
        Ok(matcher) => matcher,
        Err(message) => {
            if let StdinInput::Stream(mut stream) = input {
                stream.close(false);
            }
            return Err(usage_error(&message));
        }
    };
    let searcher = build_searcher(args, profile);
    Ok(Box::new(source::StdinSource::new(
        input,
        searcher,
        matcher,
        args.quiet,
        profile.limits,
        profile.stdin_window,
        latch.clone(),
        stderr_report(),
    )))
}
