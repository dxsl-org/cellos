//! Linearization of domain-grant publication against revocation.
//!
//! A grant's receiver PTE is published by `GrantSlice`/`GrantShare` under the
//! grant-table lock, while revocation (`GrantFree`, unregister, task exit)
//! deliberately runs *outside* that lock because it drains, shoots down and
//! may retry. Without a shared point of order the receiver could read `Live`,
//! revocation could flip to `Revoking`, find no receiver to drain, finish and
//! free the frames, and the receiver would then map the freed frames into its
//! private root.
//!
//! Invariant: the `Live` check, the receiver mapping and the recording of the
//! receiver all happen inside one hold of `gate`, and the only `Live → Revoking`
//! transition happens inside a hold of the same `gate`. So either a publication
//! completes (and is recorded) before revocation starts — the drain that
//! follows removes it — or it observes `Revoking` and maps nothing. After
//! revocation completes no receiver holds a mapping of the grant's frames.
//!
//! Lock order: grant table (`PAGE_GRANT_TABLE`/`REG_GRANT_TABLE`) → `gate` →
//! the record's receiver slot → the receiver root's internal locks /
//! `KERNEL_ROOT`. Revocation takes `gate` with no table lock held and releases
//! it before draining; nothing here takes `SCHEDULER`. Every wait a gate holder
//! performs (tag-invalidation probes) is bounded, so a revoker spinning on the
//! gate with interrupts masked delays — never deadlocks — the publisher.

use crate::sync::Spinlock;
use core::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomainGrantState {
    Live = 1,
    Revoking = 2,
    Revoked = 3,
}

/// The state word of one grant plus the gate that orders publication with it.
pub(crate) struct GrantLifecycle {
    state: AtomicU8,
    gate: Spinlock<()>,
}

impl GrantLifecycle {
    pub(crate) const fn new() -> Self {
        Self {
            state: AtomicU8::new(DomainGrantState::Live as u8),
            gate: Spinlock::new(()),
        }
    }

    /// Unordered snapshot, for early refusals only: a `Live` result here does
    /// not authorize a mapping — only [`Self::while_live`] does.
    pub(crate) fn state(&self) -> DomainGrantState {
        match self.state.load(Ordering::Acquire) {
            1 => DomainGrantState::Live,
            2 => DomainGrantState::Revoking,
            _ => DomainGrantState::Revoked,
        }
    }

    /// Run `publish` (check-dependent mapping and its recording) only while the
    /// grant is `Live`, holding the gate so revocation cannot begin in between.
    pub(crate) fn while_live<T, E>(
        &self,
        not_live: E,
        publish: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let _gate = self.gate.lock();
        if self.state() != DomainGrantState::Live {
            return Err(not_live);
        }
        publish()
    }

    /// Run `f` serialized with publication without consulting the state (a
    /// receiver drain must never interleave with a half-published receiver).
    pub(crate) fn serialized<T>(&self, f: impl FnOnce() -> T) -> T {
        let _gate = self.gate.lock();
        f()
    }

    /// Linearize revocation: `Live → Revoking` under the gate. Returns the state
    /// after the transition (`Revoked` when an earlier revoke already finished).
    pub(crate) fn begin_revoke(&self) -> DomainGrantState {
        let _gate = self.gate.lock();
        let _ = self.state.compare_exchange(
            DomainGrantState::Live as u8,
            DomainGrantState::Revoking as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.state()
    }

    /// Both halves are drained and invalidated; the frames may leave the table.
    pub(crate) fn finish_revoke(&self) {
        self.state
            .store(DomainGrantState::Revoked as u8, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "domain-grant-gate-tests.rs"]
mod tests;
