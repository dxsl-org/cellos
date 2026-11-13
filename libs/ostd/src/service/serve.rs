// SPDX-License-Identifier: MPL-2.0

//! The one service loop: attested receive, owner-death watches, heartbeat and
//! idle work, so a service writes only what it does with a request.
//!
//! [`serve`] owns the receive. Every request arrives with the sender's
//! kernel-attested [`CallerIdentity`]; a message without one goes to
//! [`Handler::on_unattributed`] (a kernel push, a raw producer) or, when its
//! sender is the root of a watched owner, to [`Handler::on_owner_death`]. It is
//! never handled as a request on a guessed identity. The [`Incoming`] of a
//! request is captured before the handler runs, so nested receives inside the
//! handler (a bounded call to another service) cannot replace the operation
//! the reply completes.
//!
//! With [`WatchPolicy`] set, `serve` subscribes to the caller's owner death
//! before the handler runs (the only turn in which the kernel grants it), and
//! reports retired generations and deaths through [`Handler::on_owner_death`].
//!
//! With a heartbeat armed, the loop never blocks longer than half the interval,
//! and it re-arms after every receive. A handler that runs longer than that must
//! call [`keepalive`]; otherwise the kernel's heartbeat sweep kills the service,
//! which is the point: a service that stops turning is restarted by init.

use api::caller_identity::{CallerIdentity, CALLER_IDENTITY_LEN};
use api::ipc::IPC_BUF_SIZE;

use super::owners::{KernelWatch, Owner, OwnerWatches};
use crate::ipc::{Incoming, PostError};
use crate::syscall::{self, SyscallResult};

/// Which callers' owner deaths [`serve`] watches.
#[derive(Clone, Copy)]
pub enum WatchPolicy {
    /// None.
    Off,
    /// Callers whose request satisfies the predicate (it creates state that
    /// outlives the call). A refused watch still serves the request: its
    /// state is then not reclaimed on death.
    OnRequest(fn(&[u8]) -> bool),
    /// Every caller, before any request. A refused watch refuses the request
    /// ([`Handler::on_unwatched`]): no state may outlive an unwatched owner.
    Every,
}

/// How [`serve`] runs.
#[derive(Clone, Copy)]
pub struct ServeConfig {
    /// Heartbeat interval in scheduler ticks; 0 arms none.
    pub heartbeat_ticks: u64,
    pub watch: WatchPolicy,
    /// Owners watched at once; callers beyond it are unwatched.
    pub watch_capacity: usize,
}

impl ServeConfig {
    /// 60 s: longer than any bounded call a handler makes (the longest SDK
    /// budget is 50 s), so only a service that stopped turning misses it.
    pub const DEFAULT_HEARTBEAT_TICKS: u64 = 6_000;

    /// A heartbeat of [`Self::DEFAULT_HEARTBEAT_TICKS`], no owner watches.
    pub const fn with_heartbeat() -> Self {
        Self {
            heartbeat_ticks: Self::DEFAULT_HEARTBEAT_TICKS,
            watch: WatchPolicy::Off,
            watch_capacity: 0,
        }
    }

    /// Also watch owners per `watch`, at most `capacity` at once.
    pub const fn watching(self, watch: WatchPolicy, capacity: usize) -> Self {
        Self {
            watch,
            watch_capacity: capacity,
            ..self
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
    /// Do not wait: take a queued message if there is one, else run
    /// [`Handler::on_empty_poll`].
    Poll,
}

/// One attested request.
pub struct Request<'a> {
    incoming: Incoming,
    caller: CallerIdentity,
    payload: &'a [u8],
    watched: bool,
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

    /// The caller's Cell incarnation, the key of state kept for it.
    pub const fn owner(&self) -> Owner {
        Owner::of(&self.caller)
    }

    /// Whether the caller's owner death is watched, so state kept for it is
    /// reclaimed through [`Handler::on_owner_death`].
    pub const fn owner_watched(&self) -> bool {
        self.watched
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

    /// [`WatchPolicy::Every`] could not watch this caller: refuse the request.
    /// Unanswered by default.
    fn on_unwatched(&mut self, _request: Request<'_>) {}

    /// `owner` is gone: its root died, or a newer generation of its CellId
    /// was watched. Reclaim what was kept for it.
    fn on_owner_death(&mut self, _owner: Owner) {}

    /// A message without a valid identity trailer that is no watched owner's
    /// death: sender 0 is the kernel, other senders are raw producers.
    /// Dropped by default.
    fn on_unattributed(&mut self, _sender: usize, _payload: &[u8]) {}

    /// Runs after every receive attempt, message or not (render, pump).
    fn on_turn(&mut self) {}

    /// A [`Wait::Poll`] found nothing queued: park the way the service needs
    /// (a completion wait on its device), bounded below half the heartbeat.
    /// Yields by default.
    fn on_empty_poll(&mut self) {
        crate::task::yield_now();
    }

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
    let mut watches = OwnerWatches::new(config.watch_capacity);
    loop {
        keepalive(&config);
        let wait = handler.on_idle();
        // A stale trailer from the previous message must never attest this one.
        buf.fill(0);
        let received = match wait {
            Wait::Poll => syscall::sys_try_recv_attested(0, &mut buf),
            wait => match receive_ticks(wait, config.heartbeat_ticks) {
                0 => syscall::sys_recv_attested(0, &mut buf),
                ticks => syscall::sys_recv_attested_timeout(0, &mut buf, ticks),
            },
        };
        match received {
            SyscallResult::Ok(sender) if sender != 0 || wait != Wait::Poll => {
                dispatch(&config, &mut watches, handler, sender, &buf);
            }
            // An empty poll; expiry and refused receives also land here.
            _ if wait == Wait::Poll => handler.on_empty_poll(),
            _ => {}
        }
        handler.on_turn();
    }
}

fn dispatch<H: Handler>(
    config: &ServeConfig,
    watches: &mut OwnerWatches,
    handler: &mut H,
    sender: usize,
    buf: &[u8; IPC_BUF_SIZE],
) {
    let payload = &buf[..IPC_BUF_SIZE - CALLER_IDENTITY_LEN];
    // The trailer must name this sender; a death record carries none.
    let caller = CallerIdentity::from_recv_buf(buf)
        .filter(|caller| sender != 0 && caller.sender_tid == sender as u64);
    let Some(caller) = caller else {
        match watches.take_death(sender) {
            Some(owner) => handler.on_owner_death(owner),
            None => handler.on_unattributed(sender, payload),
        }
        return;
    };
    // Captured before any hook runs: a nested receive would replace it.
    let incoming = Incoming::current(sender);
    let required = match config.watch {
        WatchPolicy::Off => None,
        WatchPolicy::OnRequest(creates) => creates(payload).then_some(false),
        WatchPolicy::Every => Some(true),
    };
    let mut watched = false;
    if let Some(fail_closed) = required {
        let admission = watches.admit(&caller, &mut KernelWatch);
        for owner in admission.stale {
            handler.on_owner_death(owner);
        }
        watched = admission.watched;
        if fail_closed && !watched {
            handler.on_unwatched(Request {
                incoming,
                caller,
                payload,
                watched,
            });
            return;
        }
    }
    handler.on_request(Request {
        incoming,
        caller,
        payload,
        watched,
    });
}

/// Ticks the next receive waits; 0 = untimed. A heartbeat caps every wait at
/// half its interval so the loop re-arms it in time.
fn receive_ticks(wait: Wait, heartbeat_ticks: u64) -> u64 {
    let cap = heartbeat_ticks / 2;
    // 0 would mean "untimed": an explicit tick wait never becomes one.
    let ticks = match wait {
        Wait::Block => return cap,
        Wait::Ticks(ticks) => ticks.max(1),
        Wait::Poll => 1,
    };
    if cap == 0 {
        ticks
    } else {
        ticks.min(cap)
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
        assert_eq!(receive_ticks(Wait::Ticks(0), 0), 1);
        assert_eq!(receive_ticks(Wait::Ticks(0), 6_000), 1);
    }
}
