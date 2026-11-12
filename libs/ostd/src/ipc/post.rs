// SPDX-License-Identifier: MPL-2.0
//! Non-parking sends: [`post`] for one-way messages, [`Incoming::reply`] for
//! answering a request a service received (Phase 11 CP1 §3).
//!
//! A blocking `Send` parks the sender until the target receives, so a client
//! that never receives can stall a service. Nothing here ever parks.

use super::IpcOpId;
use crate::syscall::{sys_post, SyscallResult};

/// Why [`post`] or [`Incoming::reply`] did not deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostError {
    /// The target is gone, or its bounded mailbox is full.
    Undeliverable,
    /// The request's exact operation no longer accepts a reply: the caller
    /// cancelled it, it expired, its caller exited, or it was already answered.
    ReplyRefused,
}

/// Queue one message in `target`'s bounded mailbox without parking.
///
/// The target's next receive admitting this sender takes it, so the target need
/// not be receiving now. Uses `SendGather` (202); a Cell with an explicit
/// syscall allowlist must list it.
///
/// # Errors
/// [`PostError::Undeliverable`] when the target is gone or its mailbox is full.
/// The message is then dropped; nothing is retried.
pub fn post(target: usize, msg: &[u8]) -> Result<(), PostError> {
    match sys_post(target, msg) {
        SyscallResult::Ok(0) => Ok(()),
        _ => Err(PostError::Undeliverable),
    }
}

/// Send one message and park until `target` takes it.
///
/// The one sanctioned blocking send, for producers that need backpressure
/// from a receiver that polls its mailbox and never answers: input-source
/// registration and HID events. Every other message uses [`post`] or a reply.
///
/// # Errors
/// [`PostError::Undeliverable`] when the target is gone or refuses senders.
pub fn post_blocking(target: usize, msg: &[u8]) -> Result<(), PostError> {
    match crate::syscall::sys_send(target, msg) {
        SyscallResult::Ok(_) => Ok(()),
        SyscallResult::Err(_) => Err(PostError::Undeliverable),
    }
}

/// A request a service is answering: its sender and, when it arrived as an
/// exact operation (`IpcSubmit`), that operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Incoming {
    sender: usize,
    op: Option<IpcOpId>,
}

impl Incoming {
    /// Capture the request this task received last, from `sender`.
    ///
    /// Call it before any other receive: the kernel records only the latest
    /// request, so a later receive would replace the operation this captures.
    pub fn current(sender: usize) -> Self {
        Self {
            sender,
            op: super::current(),
        }
    }

    /// The task that sent this request.
    pub const fn sender(&self) -> usize {
        self.sender
    }

    /// Answer the request without parking.
    ///
    /// An exact operation is answered with `IpcReply`, which completes it
    /// whether or not the caller is receiving. A request that arrived as a raw
    /// `Send` has no operation, so its reply is [`post`]ed to the sender's
    /// mailbox for the sender's next receive.
    ///
    /// # Errors
    /// [`PostError::ReplyRefused`] when the operation no longer accepts a reply,
    /// and [`PostError::Undeliverable`] when a posted reply cannot be queued.
    pub fn reply(&self, reply: &[u8]) -> Result<(), PostError> {
        match self.op {
            Some(op) => super::reply(op, reply).map_err(|_| PostError::ReplyRefused),
            None => post(self.sender, reply),
        }
    }
}

/// Answer `sender`'s request from a reply wrapper, without parking when the
/// request is the one this task received last.
///
/// Every receive replaces the kernel's record of the current request (VFS
/// keeps its outer request across masked receives), so a wrapper cannot trust
/// a token it did not capture. Without a current operation the reply is
/// [`post`]ed. With one, it goes through the kernel's reply bridge, which
/// completes the operation only when `sender` is the caller it belongs to; for
/// any other sender the bridge falls back to a raw send, which can park. A
/// handler that receives again before replying should capture [`Incoming`].
///
/// # Errors
/// [`PostError::Undeliverable`] when the bridge refuses the reply or a posted
/// reply cannot be queued.
pub fn reply_to(sender: usize, reply: &[u8]) -> Result<(), PostError> {
    if super::current().is_none() {
        return post(sender, reply);
    }
    match crate::syscall::sys_send(sender, reply) {
        SyscallResult::Ok(_) => Ok(()),
        SyscallResult::Err(_) => Err(PostError::Undeliverable),
    }
}
