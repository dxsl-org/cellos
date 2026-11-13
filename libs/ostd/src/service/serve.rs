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
//!
//! A hot-swap envelope (`[MAGIC, KIND_SNAPSHOT | KIND_RESTORE, …]`) from the
//! live `SUPERVISOR` provider is serve's own: it runs [`Handler::on_snapshot`]
//! or [`Handler::on_restore`] and never replies, since the supervisor posts it
//! without an operation to answer. The same bytes from anyone else are an
//! ordinary request.

use abi::caller_identity::{CallerIdentity, CALLER_IDENTITY_LEN};
use alloc::vec::Vec;
use wire::ipc::IPC_BUF_SIZE;
use wire::proto::app_envelope::{
    HEADER_LEN as ENVELOPE_HEADER_LEN, KIND_RESTORE, KIND_SNAPSHOT, MAGIC, RESTORE_LEN,
    SNAPSHOT_LEN,
};

use super::owners::{KernelWatch, Owner, OwnerWatches};
use crate::hotswap;
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

    /// The supervisor is swapping this service out: the state its replacement
    /// restores, stashed by `serve` under the swap's key. `None` carries
    /// nothing, so the replacement never signals Ready and the swap rolls back.
    /// Off by default.
    fn on_snapshot(&mut self) -> Option<Vec<u8>> {
        None
    }

    /// This incarnation is the replacement: apply `state`, the bytes the
    /// previous incarnation's [`Handler::on_snapshot`] returned. `true` makes
    /// `serve` release the stash and signal Ready (`sys_hotswap_ready`);
    /// `false` leaves the supervisor to roll back. Off by default.
    fn on_restore(&mut self, _state: &[u8]) -> bool {
        false
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
    // A swap envelope only from the live supervisor, and never answered.
    if let Some(swap) = swap_envelope(payload) {
        if super::sender_is_live_provider(super::service::SUPERVISOR, sender) {
            run_swap(handler, swap);
            return;
        }
    }
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

/// A supervisor hot-swap envelope, by its `[MAGIC, kind]` prefix.
#[derive(Debug, PartialEq, Eq)]
enum Swap {
    Snapshot(u64),
    Restore(u64),
    /// The prefix of one whose swap id does not parse: dropped.
    Malformed,
}

fn swap_envelope(payload: &[u8]) -> Option<Swap> {
    let swap = match payload {
        [MAGIC, KIND_SNAPSHOT, ..] => payload
            .get(ENVELOPE_HEADER_LEN..SNAPSHOT_LEN)
            .and_then(|id| <[u8; 8]>::try_from(id).ok())
            .map(|id| Swap::Snapshot(u64::from_le_bytes(id))),
        [MAGIC, KIND_RESTORE, ..] => payload
            .get(ENVELOPE_HEADER_LEN..RESTORE_LEN)
            .and_then(hotswap::swap_id_from_restore_key)
            .map(Swap::Restore),
        _ => return None,
    };
    Some(swap.unwrap_or(Swap::Malformed))
}

/// Snapshot: stash what the handler returns under the swap's key. Restore:
/// hand the stashed bytes to the handler, and on success release them and
/// signal Ready. Any failure leaves the replacement un-Ready, which is the
/// supervisor's cue to roll back.
fn run_swap<H: Handler>(handler: &mut H, swap: Swap) {
    match swap {
        Swap::Snapshot(swap_id) => {
            if let Some(state) = handler.on_snapshot() {
                let _ = hotswap::stash(hotswap::hotswap_key(swap_id), &state);
            }
        }
        Swap::Restore(swap_id) => {
            let key = hotswap::hotswap_key(swap_id);
            if let Ok(state) = hotswap::restore(key) {
                if handler.on_restore(&state) {
                    hotswap::clear(key);
                    syscall::sys_hotswap_ready();
                }
            }
        }
        Swap::Malformed => {}
    }
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
    use super::{receive_ticks, swap_envelope, Swap, Wait};
    use wire::proto::app_envelope::{
        KIND_MESSAGE, KIND_RESTORE, KIND_SNAPSHOT, MAGIC, SNAPSHOT_LEN,
    };

    fn window(prefix: &[u8]) -> [u8; 128] {
        let mut payload = [0u8; 128];
        payload[..prefix.len()].copy_from_slice(prefix);
        payload
    }

    #[test]
    fn swap_envelopes_are_recognised_by_their_prefix() {
        let mut snapshot = window(&[MAGIC, KIND_SNAPSHOT]);
        snapshot[2..SNAPSHOT_LEN].copy_from_slice(&0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(
            swap_envelope(&snapshot),
            Some(Swap::Snapshot(0x0102_0304_0506_0708))
        );
        let restore = window(&[MAGIC, KIND_RESTORE, b'4', b'2']);
        assert_eq!(swap_envelope(&restore), Some(Swap::Restore(42)));
    }

    #[test]
    fn a_swap_prefix_with_an_unreadable_id_is_still_a_swap_envelope() {
        // Never handed to `on_request`, so never answered.
        assert_eq!(
            swap_envelope(&window(&[MAGIC, KIND_RESTORE])),
            Some(Swap::Malformed)
        );
        assert_eq!(
            swap_envelope(&window(&[MAGIC, KIND_RESTORE, b'x'])),
            Some(Swap::Malformed)
        );
        assert_eq!(
            swap_envelope(&[MAGIC, KIND_SNAPSHOT, 1]),
            Some(Swap::Malformed)
        );
        assert_eq!(
            swap_envelope(&[MAGIC, KIND_RESTORE][..]),
            Some(Swap::Malformed)
        );
    }

    #[test]
    fn other_payloads_are_requests() {
        for payload in [
            &window(&[MAGIC, KIND_MESSAGE])[..],
            &window(&[0, KIND_SNAPSHOT])[..],
            &window(&[MAGIC])[..1],
            &[][..],
            &window(&[1, 2, 3])[..],
        ] {
            assert_eq!(swap_envelope(payload), None);
        }
    }

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
