//! Launch-context decision (Phase 05 S5-3 annex option C): every shell is the controller
//! of its own session; a launching frontend holder is only an attachment.
//!
//! The decision is made once, in `main`, from the kernel's answer to `ReadOwnContext`
//! (and, for a readable context, its argv through `oneshot::classify`):
//!
//! - `Denied`: no context was committed for this cell (the init launch). The caller
//!   keeps the serial path unchanged.
//! - a context carrying an input frontend grant (kind 6, slot 8) and the holder's
//!   stdout/stderr writers (kind 1, slots 1/2) with argv `[]` or `["sh"]`: the shell
//!   creates its own session for that frontend (`CreateForFrontend`) and runs the
//!   serial-controller path, relaying its output rings to the holder's writers instead
//!   of the physical UART. Any other argv with a frontend is refused (2).
//! - a context without a frontend: a one-shot catalog command (`oneshot`), never a
//!   shell; `["sh"]` there is refused (125).
//! - an unreadable context or a refused creation: refused. A shell that was given a
//!   context never falls back to `create_serial`, so it cannot reach the physical UART
//!   by accident.
//!
//! A holder that draws a single terminal stream (the GUI Terminal) adds the launch
//! environment entry `CELLBOX_STREAMS=merged`. The shell reads it once, here: its own
//! diagnostics then go to the stdout ring and the relay forwards both rings to the
//! holder's stdout writer, so the holder sees echo, diagnostics and prompt in program
//! order. Without the entry the holder's stdout and stderr writers stay separate.

#[cfg(any(test, not(feature = "shell_test")))]
use api::cell_session::Status;
#[cfg(not(feature = "shell_test"))]
use terminal_sdk::context::ChildContext;
#[cfg(not(feature = "shell_test"))]
use terminal_sdk::session::{FrontendOutput, TerminalSession};

/// How the first `ReadOwnContext` classifies the launch.
#[cfg(any(test, not(feature = "shell_test")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// No committed context: a legacy (init) launch.
    Legacy,
    /// A context may exist but cannot be trusted; the shell must not start.
    Refused(Status),
}

/// Classify a failed `ChildContext::read_own`. Only the kernel's "no context for this
/// cell" answer (`Denied`) selects the legacy path; every other failure is refused.
#[cfg(any(test, not(feature = "shell_test")))]
pub fn classify_read_failure(status: Status) -> Launch {
    match status {
        Status::Denied => Launch::Legacy,
        other => Launch::Refused(other),
    }
}

/// A frontend-launched shell: its own controller session, the holder's writers and
/// whether the holder asked for merged streams.
#[cfg(not(feature = "shell_test"))]
pub struct FrontendLaunch {
    pub session: TerminalSession,
    pub output: FrontendOutput,
    pub merged: bool,
}

/// Create the shell's session for the launching frontend from the context `main` already
/// read (S10-0: read once). `main` routes here only a frontend context with argv `[]` or
/// `["sh"]` (`oneshot::classify`); any other frontend argv is refused there with 2. `Err`
/// is a launch that must not start a shell (see the module documentation).
#[cfg(not(feature = "shell_test"))]
pub fn open(context: &ChildContext) -> Result<FrontendLaunch, Status> {
    let (session, output) = TerminalSession::create_for_frontend(context)?;
    Ok(FrontendLaunch { session, output, merged: streams_merged(context.environment()) })
}

/// Launch-context environment key a holder sets to ask for one merged output stream.
#[cfg(any(test, not(feature = "shell_test")))]
pub const STREAMS_KEY: &str = "CELLBOX_STREAMS";

/// The only value of [`STREAMS_KEY`] that merges the streams.
#[cfg(any(test, not(feature = "shell_test")))]
pub const STREAMS_MERGED: &str = "merged";

/// True when the launch context's environment asks for merged streams. The key and value
/// must match exactly; any other value, or no entry, keeps the separate writers.
#[cfg(any(test, not(feature = "shell_test")))]
pub fn streams_merged<'a>(environment: impl IntoIterator<Item = (&'a str, &'a str)>) -> bool {
    environment
        .into_iter()
        .any(|(key, value)| key == STREAMS_KEY && value == STREAMS_MERGED)
}

/// A shell launched without a cwd grant (the unsealed launch: a grant would path-seal it)
/// starts with an empty working directory. Relative paths and `pwd` need one: the shell's
/// default is the root.
#[cfg(any(test, not(feature = "shell_test")))]
pub const DEFAULT_CWD: &str = "/";

/// True when the shell's current directory is unset and must be defaulted to [`DEFAULT_CWD`].
#[cfg(any(test, not(feature = "shell_test")))]
pub fn needs_default_cwd(cwd: &str) -> bool {
    cwd.is_empty()
}

/// Give a cwd-less frontend shell the root as its working directory. A shell that already
/// has one is left alone. Failure is reported to the caller, which logs it: the shell is
/// still usable with absolute paths.
#[cfg(not(feature = "shell_test"))]
pub fn ensure_default_cwd() -> Result<(), Status> {
    let current = crate::cmd_cwd::get_shell_cwd().unwrap_or_default();
    if !needs_default_cwd(&current) {
        return Ok(());
    }
    ostd::syscall::sys_chdir(DEFAULT_CWD).map_err(|_| Status::NotFound)
}

/// Yields before a wait sleeps one timer tick per attempt.
const WAIT_YIELDS: u32 = 8;

/// How the shell spends one turn while the holder has not drained its writer yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    Yield,
    Tick,
}

/// The pause for attempt number `attempt` (0-based): a few yields, then ticks.
pub fn pause_for(attempt: u32) -> Pause {
    if attempt < WAIT_YIELDS {
        Pause::Yield
    } else {
        Pause::Tick
    }
}

/// Counts consecutive blocked writes so a slow holder is waited on politely.
#[derive(Debug, Default)]
pub struct Backoff {
    attempt: u32,
}

impl Backoff {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sleep one turn of the current policy and advance it.
    pub fn pause(&mut self) {
        match pause_for(self.attempt) {
            Pause::Yield => ostd::task::yield_now(),
            Pause::Tick => {
                let _ = ostd::syscall::sys_set_timer(1);
            }
        }
        self.attempt = self.attempt.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_denied_selects_the_legacy_launch() {
        assert_eq!(classify_read_failure(Status::Denied), Launch::Legacy);
        for status in [
            Status::Malformed,
            Status::Stale,
            Status::WrongOwner,
            Status::QuotaExceeded,
            Status::WouldBlock,
            Status::TimedOut,
            Status::Io,
            Status::Unsupported,
            Status::UnknownOperation,
        ] {
            assert_eq!(classify_read_failure(status), Launch::Refused(status));
        }
    }

    #[test]
    fn only_an_unset_cwd_defaults_to_the_root() {
        assert!(needs_default_cwd(""));
        assert!(!needs_default_cwd("/"));
        assert!(!needs_default_cwd("/tmp"));
        assert_eq!(DEFAULT_CWD, "/");
    }

    #[test]
    fn waiting_yields_first_then_sleeps_a_tick() {
        for attempt in 0..WAIT_YIELDS {
            assert_eq!(pause_for(attempt), Pause::Yield);
        }
        assert_eq!(pause_for(WAIT_YIELDS), Pause::Tick);
        assert_eq!(pause_for(u32::MAX), Pause::Tick);
    }

    fn merged_for(environment: &[(&str, &str)]) -> bool {
        use api::cell_session::ObjectRefV1;
        use terminal_sdk::context::ChildContext;
        let operation = ObjectRefV1 { id: 5, generation: 2 };
        let built = ChildContext::new(operation, &["/bin/cellbox"], b"", environment, &[])
            .expect("valid context");
        // Read it back from its canonical bytes, as `read_own` does.
        let context = ChildContext::from_bytes(built.as_bytes().to_vec()).expect("canonical bytes");
        streams_merged(context.environment())
    }

    #[test]
    fn the_exact_merged_entry_in_the_launch_context_requests_merged_streams() {
        assert!(merged_for(&[("CELLBOX_STREAMS", "merged")]));
        assert!(merged_for(&[
            ("A", "1"),
            ("CELLBOX_STREAMS", "merged"),
            ("HOME", "/root"),
        ]));
    }

    #[test]
    fn no_entry_or_any_other_entry_keeps_separate_streams() {
        assert!(!merged_for(&[]));
        assert!(!merged_for(&[("HOME", "/root"), ("TERM", "merged")]));
        assert!(!merged_for(&[("CELLBOX_STREAMS", "")]));
        assert!(!merged_for(&[("CELLBOX_STREAMS", "separate")]));
        assert!(!merged_for(&[("CELLBOX_STREAMS", "Merged")]));
        assert!(!merged_for(&[("CELLBOX_STREAMS", "merged ")]));
        assert!(!merged_for(&[("CELLBOX_STREAMS", "merged,x")]));
        assert!(!merged_for(&[("CELLBOX_STREAMS_X", "merged")]));
        assert!(!merged_for(&[("cellbox_streams", "merged")]));
        assert!(!merged_for(&[("CELLBOX_STREAM", "merged")]));
    }
}
