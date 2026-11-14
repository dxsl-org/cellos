//! Fault-injection controls for the grant negative fixtures (Phase 15).
//!
//! Each [`GuardedCheck`] names exactly one isolation check on the domain grant
//! or domain copy path. A negative fixture runs twice: once as built, where it
//! must report `PASS`, and once as its CONTROL, with that one check disabled,
//! where it must observe the violation the check exists to prevent and report
//! `CAUGHT`. A fixture that still passes with its check disabled proves nothing
//! about that check, so the lane fails on it.
//!
//! The module exists only in `test-hooks` images: a production kernel has no
//! switch, and every consult site is compiled out with it.

use core::sync::atomic::{AtomicU32, Ordering};

/// One guarded check a CONTROL run may disable.
#[derive(Clone, Copy, Debug)]
pub(crate) enum GuardedCheck {
    /// `DomainGrant::revoke` drains the receiver half before the owner half.
    RevokeDrainsReceiver,
    /// Receiver death drains the receiver half of every grant it held.
    DeathDrainsReceiver,
    /// A domain slice resolves only a `Live` record.
    SliceNeedsLive,
    /// A domain slice resolves only for the task the grant is shared to.
    SliceNeedsGrantee,
    /// A domain copy commit re-walks every page under its reader pin and
    /// refuses one whose leaf is gone or no longer grants the access.
    CommitRewalksPages,
}

impl GuardedCheck {
    /// This check's bit in [`DISABLED`].
    pub(crate) const fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// Checks disabled for the running CONTROL, one bit per [`GuardedCheck`].
///
/// Written only by the negative fixtures (`task::grant_negative_selftest`), in
/// the boot fixture section before any cell runs, and always cleared again
/// before the fixture returns.
pub(crate) static DISABLED: AtomicU32 = AtomicU32::new(0);

/// Is `check` disabled? Always `false` outside a CONTROL run.
#[inline]
pub(crate) fn disabled(check: GuardedCheck) -> bool {
    DISABLED.load(Ordering::Acquire) & check.bit() != 0
}
