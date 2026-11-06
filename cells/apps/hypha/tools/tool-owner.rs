//! Owner binding shared by the Hypha tool cells and the `core` that spawns them.
//!
//! WHY: `tool-fs` (VfsMutate) and `tool-spawn` (SpawnCap) act for whoever sends
//! them `AgentToolRequest::Invoke`. Every task's tid is public (`GetProcs`), so an
//! unbound tool is a confused deputy that lends its authority to any cell.
//!
//! Binding: `core` publishes `--owner=<its tid>` as the tool's spawn argv, and the
//! tool serves only messages whose kernel-stamped sender equals that tid. This is
//! sound because (1) only the spawner can publish a child's argv — the kernel
//! moves the stash into the child's private slot at spawn, (2) the receive path
//! stamps the real sender, never a payload claim, and (3) tids are never
//! re-issued within one boot, so the binding cannot outlive `core` by being
//! inherited by a later task.
//!
//! `core` cannot read its own tid from the kernel; it learns it from the
//! capability-free `tool-sys`, which echoes the kernel-stamped sender of a
//! `caller_tid` request as `{"tid":N}` ([`parse_caller_tid`]).

use alloc::string::String;

/// The single argv entry a bound tool is spawned with.
const OWNER_FLAG: &str = "--owner=";
/// `tool-sys`'s reply to `caller_tid`, around the decimal tid.
const CALLER_TID_PREFIX: &str = "{\"tid\":";
const CALLER_TID_SUFFIX: &str = "}";

/// The argv entry that binds a tool to `tid`.
pub fn owner_arg(tid: usize) -> String {
    alloc::format!("{OWNER_FLAG}{tid}")
}

/// The owner a tool was spawned with, or `None` when the argv is anything other
/// than exactly one well-formed `--owner=<nonzero decimal>` entry. `None` means
/// "unbound": the tool must refuse to serve at all.
pub fn parse_owner(argv: &[String]) -> Option<usize> {
    let [only] = argv else {
        return None;
    };
    nonzero_decimal(only.strip_prefix(OWNER_FLAG)?)
}

/// Whether a request whose kernel-stamped sender is `sender` may be served by a
/// tool bound to `owner`. Never pass a tid taken from a message payload.
pub fn admits(owner: usize, sender: usize) -> bool {
    sender == owner
}

/// Parse `tool-sys`'s `caller_tid` reply. Strict: anything but `{"tid":N}` with a
/// nonzero decimal `N` is `None`.
pub fn parse_caller_tid(result_json: &str) -> Option<usize> {
    let digits = result_json
        .strip_prefix(CALLER_TID_PREFIX)?
        .strip_suffix(CALLER_TID_SUFFIX)?;
    nonzero_decimal(digits)
}

fn nonzero_decimal(digits: &str) -> Option<usize> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<usize>().ok().filter(|tid| *tid != 0)
}
