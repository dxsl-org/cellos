//! Kernel IPC objects serialized by the scheduler lock: the IPC wire copy, pending
//! mailboxes, pipes, futex wait queues and Cell sessions (Spec 15 §2, amended).
// Same lint posture as `task`, which these modules came from.
#![allow(clippy::result_unit_err)]

pub(crate) mod cell_session;
/// Futex wait queues — wait-on-address for cell threads (ADR-0018 §2.1).
pub(crate) mod futex;
pub mod ipc_wire;
#[cfg(test)]
mod ipc_wire_tests;
pub mod pending_mailbox;
/// Kernel-owned pipes — one-way byte streams between tasks (ADR-0018 §2.1).
pub(crate) mod pipe;
