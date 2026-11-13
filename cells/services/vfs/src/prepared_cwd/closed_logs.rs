//! Retirement of a live creator's per-Operation request logs.
//!
//! A log fences two ID sequences for one `(creator, Operation)` scope: the creator's
//! requests to VFS and VFS's own kernel mutations in that Operation. While the
//! Operation exists, dropping the log would let a retransmitted request ID run again
//! and would restart `kernel_next` against the kernel's ledger. Once the kernel has
//! removed the Operation, no request in the scope can be admitted again: ReserveCwd
//! resolves the prepared record (removed with the Operation), Commit/Resolve need a
//! reservation, and Acknowledge/Query need the log. Operation references are never
//! reused, so a removed scope cannot be named again.
use abi::cell_session::{KernelOp, ObjectRefV1, Status, Writer, HEADER_LEN};
use alloc::vec::Vec;
use types::CellId;

use super::{kernel_query, scope, PreparedCwd, Scope, LOG_CHARGE};
use crate::caller::Caller;
use crate::manager::VfsManager;

/// Logs that only the Operation's removal can still change, with the kernel ID they
/// were acknowledged through: no reservation names the scope, no outcome is retained,
/// and every kernel mutation was resolved and acknowledged. At least one kernel
/// mutation is required, because the removal probe asks about VFS's own ledger in the
/// Operation and the kernel creates that ledger with the first mutation.
pub(super) fn quiescent(state: &PreparedCwd, only: Option<Caller>) -> Vec<(Scope, u64)> {
    state
        .logs
        .iter()
        .filter(|(key, log)| {
            only.is_none_or(|c| key.0 == c.cell.0 && key.1 == c.generation)
                && log.outcomes.is_empty()
                && log.kernel_pending.is_empty()
                && log.kernel_next > 1
                && log.kernel_acked == log.kernel_next - 1
                && !state
                    .reservations
                    .values()
                    .any(|value| scope(value.owner, value.operation) == **key)
        })
        .map(|(key, log)| (*key, log.kernel_acked))
        .collect()
}

/// The kernel keeps VFS's request ledger for an Operation until `remove_operation`.
/// Re-acknowledging an already-acknowledged ID changes nothing on a live ledger and
/// answers `UnknownRequest` once the ledger is gone. Any other answer, including a
/// transport failure, keeps the log.
fn operation_removed(key: Scope, acknowledged: u64) -> bool {
    let mut body = [0u8; 24];
    let mut w = Writer::new(&mut body);
    let operation = ObjectRefV1 {
        id: key.2,
        generation: key.3,
    };
    if w.reference(operation).is_err() || w.u64(acknowledged).is_err() {
        return false;
    }
    let mut out = [0u8; HEADER_LEN];
    kernel_query(KernelOp::AcknowledgeRequests, &body, &mut out) == Err(Status::UnknownRequest)
}

/// Drop one log and return its charge to the creator.
pub(super) fn drop_log(state: &mut PreparedCwd, vfs: &mut VfsManager, key: Scope) -> bool {
    if state.logs.remove(&key).is_none() {
        return false;
    }
    vfs.quota.release(CellId(key.0), LOG_CHARGE);
    true
}

/// Retire every quiescent log (of `only`, when given) whose Operation the kernel has
/// removed. Costs one kernel query per quiescent log.
pub(super) fn retire_closed(state: &mut PreparedCwd, vfs: &mut VfsManager, only: Option<Caller>) {
    for (key, acknowledged) in quiescent(state, only) {
        if operation_removed(key, acknowledged) {
            drop_log(state, vfs, key);
        }
    }
}
