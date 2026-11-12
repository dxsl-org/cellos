// SPDX-License-Identifier: MPL-2.0

//! The one service loop: attested receive, heartbeat and idle work, so a service
//! writes only what it does with a request.
//!
//! [`serve`] owns the receive. Every request arrives with the sender's
//! kernel-attested [`CallerIdentity`]; a message without one (a kernel push, a
//! raw producer) goes to [`Handler::on_unattributed`] and is never handled as a
//! request on a guessed identity. The [`Incoming`] of a request is captured
//! before the handler runs, so nested receives inside the handler (a bounded
//! call to another service) cannot replace the operation the reply completes.
//!
//! With a heartbeat armed, the loop never blocks longer than half the interval,
//! and it re-arms after every receive. A handler that runs longer than that must
//! call [`keepalive`]; otherwise the kernel's heartbeat sweep kills the service,
//! which is the point: a service that stops turning is restarted by init.

use api::caller_identity::{CallerIdentity, CALLER_IDENTITY_LEN};
use api::ipc::IPC_BUF_SIZE;

use crate::ipc::{Incoming, PostError};
use crate::syscall::{self, SyscallResult};

/// How [`serve`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServeConfig {
    /// Heartbeat interval in scheduler ticks; 0 arms none.
    pub heartbeat_ticks: u64,
}

impl ServeConfig {
    /// 60 s: longer than any bounded call a handler makes (the longest SDK
    /// budget is 50 s), so only a service that stopped turning misses it.
    pub const DEFAULT_HEARTBEAT_TICKS: u64 = 6_000;

    /// A heartbeat of [`Self::DEFAULT_HEARTBEAT_TICKS`].
    pub const fn with_heartbeat() -> Self {
        Self {
            heartbeat_ticks: Self::DEFAULT_HEARTBEAT_TICKS,
        }
    }
}

/// How long the next receive may wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// Until a message arrives (or half the heartbeat interval).
    Block,
    /// At most this many ticks, then [`Handler::on_idle`] runs again.
    Ticks(u64),
}

/// One attested request.
pub struct Request<'a> {
    incoming: Incoming,
    caller: CallerIdentity,
    payload: &'a [u8],
}

impl<'a> Request<'a> {
    /// The task that sent the request: an address for replies, never an identity.
    pub const fn sender(&self) -> usize {
        self.incoming.sender()
    }

    /// The kernel-attested sender.
    pub const fn caller(&self) -> &CallerIdentity {
        &self.caller
    }

    /// The receive buffer up to the identity trailer. Raw IPC carries no byte
    /// count, so this is the whole zero-padded payload window.
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    /// The request's reply route, for a handler that answers later.
    pub const fn incoming(&self) -> Incoming {
        self.incoming
    }

    /// Answer without parking ([`Incoming::reply`]).
    ///
    /// # Errors
    /// As [`Incoming::reply`].
    pub fn reply(&self, bytes: &[u8]) -> Result<(), PostError> {
        self.incoming.reply(bytes)
    }
}

/// What a service does; [`serve`] does the rest.
pub trait Handler {
    /// Handle one attested request. Answer it with [`Request::reply`] or keep
    /// [`Request::incoming`] to answer later.
    fn on_request(&mut self, request: Request<'_>);

    /// A message without a valid identity trailer: sender 0 is the kernel, other
    /// senders are raw producers. Dropped by default.
    fn on_unattributed(&mut self, _sender: usize, _payload: &[u8]) {}

    /// Runs before every receive; periodic work goes here. Its answer bounds the
    /// receive that follows.
    fn on_idle(&mut self) -> Wait {
        Wait::Block
    }
}

/// Re-arm the heartbeat from inside a long handler.
pub fn keepalive(config: &ServeConfig) {
    if config.heartbeat_ticks > 0 {
        syscall::sys_heartbeat(config.heartbeat_ticks);
    }
}

/// Serve requests forever.
pub fn serve<H: Handler>(config: ServeConfig, handler: &mut H) -> ! {
    let mut buf = [0u8; IPC_BUF_SIZE];
    loop {
        keepalive(&config);
        let ticks = receive_ticks(handler.on_idle(), config.heartbeat_ticks);
        // A stale trailer from the previous message must never attest this one.
        buf.fill(0);
        let received = if ticks == 0 {
            syscall::sys_recv_attested(0, &mut buf)
        } else {
            syscall::sys_recv_attested_timeout(0, &mut buf, ticks)
        };
        // Expiry and refused receives both mean "nothing to handle".
        let SyscallResult::Ok(sender) = received else {
            continue;
        };
        let payload_len = buf.len() - CALLER_IDENTITY_LEN;
        let incoming = Incoming::current(sender);
        match CallerIdentity::from_recv_buf(&buf) {
            Some(caller) if sender != 0 => handler.on_request(Request {
                incoming,
                caller,
                payload: &buf[..payload_len],
            }),
            _ => handler.on_unattributed(sender, &buf[..payload_len]),
        }
    }
}

/// Ticks the next receive waits; 0 = untimed. A heartbeat caps every wait at
/// half its interval so the loop re-arms it in time.
fn receive_ticks(wait: Wait, heartbeat_ticks: u64) -> u64 {
    let cap = heartbeat_ticks / 2;
    match (wait, cap) {
        (Wait::Block, 0) => 0,
        (Wait::Block, cap) => cap,
        (Wait::Ticks(ticks), 0) => ticks.max(1),
        (Wait::Ticks(ticks), cap) => ticks.clamp(1, cap),
    }
}

#[cfg(test)]
mod tests {
    use super::{receive_ticks, Wait};

    #[test]
    fn a_heartbeat_bounds_every_wait_by_half_its_interval() {
        assert_eq!(receive_ticks(Wait::Block, 0), 0);
        assert_eq!(receive_ticks(Wait::Block, 6_000), 3_000);
        assert_eq!(receive_ticks(Wait::Ticks(10_000), 6_000), 3_000);
        assert_eq!(receive_ticks(Wait::Ticks(100), 6_000), 100);
        // 0 would mean "untimed"; an explicit tick wait never becomes one.
        assert_eq!(receive_ticks(Wait::Ticks(0), 0), 1);
        assert_eq!(receive_ticks(Wait::Ticks(0), 6_000), 1);
    }
}
