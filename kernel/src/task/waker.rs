//! The NET_RX event source: where a NIC RX frame is reported to the cell
//! waiting for one.
//!
//! Two mechanisms serve this one source, and they are not alternatives:
//!
//! - **A reservation.** `WaitCompletion(NET_RX)` reserves a slot on the calling
//!   cell's completion queue and records it here. That reservation is what the
//!   RX signal completes, so the result lands in a place that already existed
//!   before the frame arrived — the interrupt path never allocates and can never
//!   be refused.
//! - **A level flag.** `NET_RX_PENDING` remembers a frame that arrived while
//!   nobody was waiting. A hardware condition is level-triggered: a frame that
//!   turns up between two waits must still be visible to the next one, or the
//!   cell sleeps on data it already has. The flag is that memory, and it is the
//!   lost-wakeup guard the wait applies before parking. Removing it would not
//!   simplify anything — it would lose frames.
//!
//! Exactly one reservation may be outstanding, because there is one source and
//! one consumer. Arming while one is already outstanding displaces it, and the
//! displaced reservation is completed as abandoned rather than dropped: a slot
//! that has been promised a landing must always get one, or its waiter never
//! runs again. Refusing the newcomer instead was rejected — a reservation left
//! behind by a cell that died mid-wait would then wedge the source for the cell
//! that replaces it, and a service dying and being restarted is routine here.
//!
//! Lock order: `NET_RX_WAIT` is a leaf and is taken alone. It is never held
//! across `CompletionQueue::complete`, so the append path's lock set stays
//! exactly one lock, which is what makes it safe from interrupt context. It is
//! also never taken while `SCHEDULER` is held, so an interrupt on another hart
//! never waits behind a scheduler critical section.

pub(super) use self::net_rx_reservation::{DisarmResult, PendingCompletion};
use super::completion::{CompletionQueue, SlotId};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};

mod net_rx_reservation;

/// Set when an RX frame arrives with no reservation outstanding.
/// Consumed by the next waiter before it parks.
pub static NET_RX_PENDING: AtomicBool = AtomicBool::new(false);

/// Returns true if any event is currently pending.
pub fn has_any_pending() -> bool {
    NET_RX_PENDING.load(Ordering::Relaxed)
}

/// Record `slot` on `queue` as the landing place for the next RX frame.
///
/// Call from the submitting cell's own context, never from an interrupt: a
/// displaced reservation is completed here, and the handle it displaced is
/// dropped here, both of which may run arbitrary destructor work.
///
/// # Panics
/// Never panics.
pub fn arm_net_rx(queue: Arc<CompletionQueue>, slot: SlotId) {
    net_rx_reservation::arm(queue, slot);
}

/// Release the outstanding reservation if it belongs to `queue`.
///
/// Returns the slot only to the caller that took it, so a waiter and the
/// interrupt path can race for the same reservation and exactly one of them
/// wins. A `None` means the interrupt path got there first and the result is
/// already on the queue.
pub fn disarm_net_rx(queue: &Arc<CompletionQueue>) -> DisarmResult {
    net_rx_reservation::disarm(queue)
}

pub(super) fn begin_signal_net_rx_for_test() -> Option<PendingCompletion> {
    net_rx_reservation::begin_signal()
}

pub(super) fn finish_signal_net_rx_for_test(pending: PendingCompletion) {
    net_rx_reservation::finish_signal(pending, api::syscall::events::NET_RX as isize);
}

/// Signal a NIC RX event.  Called from the VirtIO interrupt handler (ISR context).
///
/// Completes the outstanding reservation if there is one, and otherwise records
/// the frame in `NET_RX_PENDING` for whoever waits next. Waking the completed
/// waiter is deliberately not done here — it needs the scheduler, which this
/// context may not take, so it is deferred to `deliver_pending_wakes`.
/// On RISC-V we pend local SSIP so the timer handler fires without waiting for
/// the next mtime tick — sub-millisecond latency on the handling hart.
///
/// # Safety contract
/// Callers must be in S-mode trap context (SIE already cleared by hardware entry).
pub fn signal_net_rx() {
    match net_rx_reservation::begin_signal() {
        Some(pending) => {
            net_rx_reservation::finish_signal(pending, api::syscall::events::NET_RX as isize)
        }
        None => NET_RX_PENDING.store(true, Ordering::Release),
    }

    // Pend a software interrupt on the current hart so vi_timer_tick fires immediately.
    // SAFETY: csrsi sip.SSIP is permitted from S-mode (RISC-V priv spec §4.1.3).
    // SIE is currently cleared by the hardware trap entry, so this is queued and
    // fires once the ISR returns and sret restores sstatus.SIE.
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("csrsi sip, 0x2", options(nomem, nostack))
    };
}

/// Check whether event `mask` has any pending bits.  Returns the matching fired bits,
/// or 0 if none.  Clears the matching bits as a side effect (consume-on-read).
///
/// Called by the timer sweep (already under SCHEDULER), by the WaitForEvent
/// syscall handler before parking the task, and by `WaitCompletion` after it
/// arms — the ordering that closes the window where a frame arrives between the
/// two.
pub fn consume_pending(mask: u32) -> u32 {
    let mut fired: u32 = 0;
    if mask & api::syscall::events::NET_RX != 0
        && NET_RX_PENDING
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    {
        fired |= api::syscall::events::NET_RX;
    }
    fired
}
