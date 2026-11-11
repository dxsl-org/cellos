//! Lease between Sas commits and the paths that retire a shared-root user page.
//!
//! A Sas commit copies through the user VA itself. The release paths retire
//! such a page by re-mapping the same identity VA kernel-RWX and returning the
//! frame to the allocator, so a commit that merely probed earlier would keep
//! copying — without faulting, since kernel-RWX is writable from the kernel —
//! into a frame that may already be a page table or another cell's memory.
//!
//! Protocol:
//! * commit ([`commit_sas`]): interrupts masked, this hart's window
//!   sequence goes odd, full fence, every page is re-walked for
//!   VALID | USER | R/W, the bytes move, the sequence goes even.
//! * release (`task::stack::unpublish_user_frames`): clear the PTEs,
//!   invalidate every hart's TLB, then [`wait_sas_copies_quiescent`] — full
//!   fence, wait for every hart that was inside a window to leave it. Only then
//!   may the caller re-map the frames kernel-RWX and hand them to the allocator.
//!
//! Store-buffering argument: each side stores (sequence / PTE), fences SeqCst,
//! then loads the other's location, so at least one observes the other. Either
//! the commit sees the cleared PTE and fails with a recoverable error before
//! moving a byte, or the release sees the odd sequence and waits until the
//! commit's last byte has landed in a frame that is not yet back in the
//! allocator. A commit validating after the frame was re-issued as another
//! Sas USER page writes memory every Sas cell can already reach — never a
//! kernel-only frame, because those are never mapped USER.
//!
//! Lock order: the window holds no lock and runs a bounded byte loop, so the
//! release-side wait is bounded under any caller lock and takes no lock itself.

use super::guard::commit_guarded;
use super::range::{CopyError, Direction};
use super::sv39_probe::{current_satp_root, sas_span_live, sv39_leaf};
use crate::sched::smp::MAX_HARTS;
use core::sync::atomic::{fence, AtomicUsize, Ordering};

/// One window sequence per hart: odd while that hart is inside a Sas commit.
pub(super) struct WindowSeqs<const N: usize> {
    seq: [AtomicUsize; N],
}

impl<const N: usize> WindowSeqs<N> {
    pub(super) const fn new() -> Self {
        Self {
            seq: [const { AtomicUsize::new(0) }; N],
        }
    }

    /// Open `hart`'s window. The caller must have interrupts masked so the
    /// window can neither migrate nor nest. `None` (unknown hart) fails closed.
    pub(super) fn open(&self, hart: usize) -> Option<Window<'_>> {
        let slot = self.seq.get(hart)?;
        slot.fetch_add(1, Ordering::SeqCst);
        // Orders the odd sequence before the PTE re-walk that follows.
        fence(Ordering::SeqCst);
        Some(Window { slot })
    }

    /// Wait until every window open at the time of the call has closed. Must
    /// run after the caller's PTE stores; windows opened later observe them.
    pub(super) fn wait_quiescent(&self) {
        fence(Ordering::SeqCst);
        for slot in &self.seq {
            let seen = slot.load(Ordering::SeqCst);
            if seen & 1 == 0 {
                continue;
            }
            // Any change means that window closed; a newer one re-walks PTEs.
            while slot.load(Ordering::Acquire) == seen {
                core::hint::spin_loop();
            }
        }
    }
}

/// An open window; closing publishes every byte the commit moved.
pub(super) struct Window<'a> {
    slot: &'a AtomicUsize,
}

impl Drop for Window<'_> {
    fn drop(&mut self) {
        self.slot.fetch_add(1, Ordering::Release);
    }
}

static SAS_WINDOWS: WindowSeqs<MAX_HARTS> = WindowSeqs::new();

/// Sas commit under the release lease: the user pages are re-validated against
/// the live shared root inside the same interrupt-masked window that moves the
/// bytes, so a page retired after the caller's probe fails here instead.
pub(super) fn commit_sas(
    direction: Direction,
    user_ptr: usize,
    other: *mut u8,
    len: usize,
) -> Result<(), CopyError> {
    if len == 0 {
        return Ok(());
    }
    let saved = crate::hal::arch::save_and_disable_interrupts();
    let result = leased_body(direction, user_ptr, other, len);
    // SAFETY: restores exactly the interrupt state saved above.
    unsafe { crate::hal::arch::restore_sstatus(saved) };
    result
}

fn leased_body(
    direction: Direction,
    user_ptr: usize,
    other: *mut u8,
    len: usize,
) -> Result<(), CopyError> {
    let window = SAS_WINDOWS
        .open(crate::sched::hart_local::current_hart_id())
        .ok_or(CopyError::InvalidAddress)?;
    let root_pa = current_satp_root().ok_or(CopyError::InvalidAddress)?;
    sas_span_live(user_ptr, len, direction, |page| {
        sv39_leaf(root_pa, page).map(|(bits, _)| bits)
    })?;
    let (src, dst) = if direction == Direction::FromUser {
        (user_ptr as *const u8, other)
    } else {
        (other as *const u8, user_ptr as *mut u8)
    };
    let result = commit_guarded(src, dst, len, user_ptr, user_ptr + len);
    drop(window);
    result
}

/// Return once every Sas commit window open at the call has closed. Call after
/// the PTE stores that retire a Sas user page; later windows observe them.
pub(crate) fn wait_sas_copies_quiescent() {
    SAS_WINDOWS.wait_quiescent();
}

#[cfg(all(test, not(target_os = "none")))]
#[path = "sas-lease-tests.rs"]
mod tests;
