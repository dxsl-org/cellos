//! IRQ wait/pending tables for Driver Cell `sys_wait_irq`.
//!
//! Provides lock-free, ISR-safe primitives for the deferred-wake pattern:
//!
//! - `register_waiter(irq, tid, mmio_base)` — called from syscall context; records
//!   the TID waiting on this IRQ line and the VirtIO MMIO base for InterruptACK.
//! - `signal_irq(irq)` — called from ISR; sets `IRQ_PENDING[irq]` atomically.
//!   NO scheduler access, NO lock; ISR safety is the invariant.
//! - `take_pending(irq)` — swap-and-read; used by both the lost-wakeup guard in
//!   `sys_wait_irq` and the scheduler sweep in `pick_next`.
//! - `clear_waiter(irq)` — scheduler calls after transitioning the task to Ready.
//!
//! # ISR safety
//! `signal_irq` uses only `AtomicBool::store(Release)` — no Spinlock, no allocation,
//! no scheduler access.  This is the invariant.  Any attempt to wake a task directly
//! from ISR context (by setting TaskState::Ready inside an interrupt handler) will
//! deadlock because the scheduler Spinlock is already held by the interrupted task's
//! scheduler tick.  Model: `kernel/src/task/waker.rs:consume_pending`.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Number of IRQ lines; every `u8` IRQ number has a slot.
///
/// Covers VirtIO MMIO slots 1-8 (IRQs 1-8), the e1000 NIC (IRQ 33 on QEMU ARM/RV),
/// the NVMe SSD (IRQ 34), GPIOs (< 64) and, on x86_64, the MSI/MSI-X vectors
/// 0x30..=0xDF that `AllocPcieIrq` hands out as IRQ numbers (ADR-0031 CP1 §4.1).
pub const MAX_IRQ: usize = 256;

/// Sentinel: no task is waiting on this IRQ.
const NO_WAITER: usize = 0;

/// TID of the Driver Cell waiting on each IRQ line. 0 = no waiter.
///
/// Written by `register_waiter` (syscall context, SeqCst).
/// Read by `get_waiter` (scheduler sweep).
/// Cleared by `clear_waiter` (scheduler sweep after task transitions to Ready).
static IRQ_WAITERS: [AtomicUsize; MAX_IRQ] = {
    // `AtomicUsize::new(0)` is const; arrays of const-init atomics are safe.
    // `ZERO` is consumed exactly once, by `[ZERO; MAX_IRQ]` below — rustc evaluates
    // a `const` operand fresh per array slot, so each IRQ line gets its OWN
    // independent atomic, not a shared one. Switching to `static` would break the
    // repeat expression (needs a `const` for non-`Copy` element types).
    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO: AtomicUsize = AtomicUsize::new(NO_WAITER);
    [ZERO; MAX_IRQ]
};

/// Pending flag for each IRQ line.
///
/// Set by `signal_irq` (ISR, Release ordering).
/// Cleared by `take_pending` (scheduler sweep or lost-wakeup guard, AcqRel swap).
static IRQ_PENDING: [AtomicBool; MAX_IRQ] = {
    // Same independent-per-slot rationale as `IRQ_WAITERS` above.
    #[allow(clippy::declare_interior_mutable_const)]
    const FALSE: AtomicBool = AtomicBool::new(false);
    [FALSE; MAX_IRQ]
};

/// VirtIO MMIO slot base for each IRQ line; used by the ISR to write InterruptACK
/// (offset 0x64) and prevent interrupt storms on level-triggered VirtIO devices.
/// 0 = non-VirtIO device (PCIe MSI, GPIO, …) — ISR skips the ack write.
static IRQ_MMIO_BASE: [AtomicUsize; MAX_IRQ] = {
    // Same independent-per-slot rationale as `IRQ_WAITERS` above.
    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO: AtomicUsize = AtomicUsize::new(0);
    [ZERO; MAX_IRQ]
};

/// Register `tid` as the waiter for `irq`, storing `mmio_base` for the ISR ack.
///
/// Returns `false` if another task is already waiting (single-waiter-per-IRQ
/// policy). The caller (`sys_wait_irq`) must return `AlreadyClaimed` to the Cell.
///
/// SeqCst on the TID store so that a concurrent ISR that reads `has_waiter` after
/// `register_waiter` returns will always see the new TID.
pub fn register_waiter(irq: u8, tid: usize, mmio_base: usize) -> bool {
    let idx = irq as usize;
    if idx >= MAX_IRQ {
        return false;
    }
    // compare_exchange: only register if slot is empty.
    let result =
        IRQ_WAITERS[idx].compare_exchange(NO_WAITER, tid, Ordering::SeqCst, Ordering::SeqCst);
    if result.is_ok() {
        IRQ_MMIO_BASE[idx].store(mmio_base, Ordering::Release);
        true
    } else {
        false
    }
}

/// Atomically take the pending flag for `irq`.
///
/// Returns `true` if an IRQ was pending (and now cleared); `false` if not.
/// Used as both the lost-wakeup guard in `sys_wait_irq` AND the scheduler sweep.
pub fn take_pending(irq: u8) -> bool {
    let idx = irq as usize;
    if idx >= MAX_IRQ {
        return false;
    }
    // swap(false): clears the flag and returns the old value in one atomic op.
    IRQ_PENDING[idx].swap(false, Ordering::AcqRel)
}

/// Signal that IRQ `irq` has fired.
///
/// Called ONLY from ISR context (interrupt handler). Sets the pending flag and
/// optionally writes the VirtIO InterruptACK register to de-assert the IRQ line
/// before PLIC completion, preventing interrupt storms on level-triggered devices.
///
/// # ISR safety
/// Uses only `AtomicBool::store(Release)` — no Spinlock, no scheduler access.
/// The actual task state transition (WaitIrq → Ready) happens in `pick_next`.
pub fn signal_irq(irq: u8) {
    let idx = irq as usize;
    if idx >= MAX_IRQ {
        return;
    }

    let mmio_base = IRQ_MMIO_BASE[idx].load(Ordering::Acquire);
    if mmio_base != 0 {
        // Ack VirtIO InterruptACK (MMIO base + 0x64) to clear the device IRQ line.
        // Without this, a level-triggered VirtIO interrupt re-fires immediately
        // after plic_complete, spinning the ISR forever.
        //
        // SAFETY: mmio_base was validated by sys_request_mmio before the Cell
        // stored it; the MMIO window is identity-mapped and within the known device
        // allowlist. ACK the exact pending status bits before PLIC completion.
        let _ = crate::task::drivers::virtio_common::ack_virtio_interrupt_status(mmio_base);
    }

    // Set pending AFTER the MMIO ack so the scheduler sweep races correctly:
    // either the cell drains the used ring or the next ISR acks again.
    IRQ_PENDING[idx].store(true, Ordering::Release);
}

/// Clear the waiter registration for `irq`.
///
/// Called by the scheduler sweep after transitioning the waiting task to Ready,
/// so the IRQ slot is open for a new `sys_wait_irq` call from the same Cell on
/// the next iteration.
pub fn clear_waiter(irq: u8) {
    let idx = irq as usize;
    if idx >= MAX_IRQ {
        return;
    }
    IRQ_WAITERS[idx].store(NO_WAITER, Ordering::Release);
    IRQ_MMIO_BASE[idx].store(0, Ordering::Release);
}

/// Release every IRQ line whose waiter is `tid`; returns how many were released.
///
/// Called by the retirement funnel for a task that will never run again. A
/// waiter is otherwise cleared only when the scheduler wakes it, so a driver
/// that dies inside `WaitIrq` would keep its line forever and its replacement
/// would get `TryAgain` on every wait.
///
/// The MMIO base is cleared first, so the ISR stops acking the dead owner's
/// device before the slot can be claimed again. An ISR that loaded the base just
/// before the clear issues one extra InterruptACK to the same, still-present
/// virtio device, which is harmless. The pending flag is dropped last: a
/// replacement must not take the dead owner's interrupt as its first wake.
pub fn release_waiters_for(tid: usize) -> usize {
    if tid == NO_WAITER {
        return 0;
    }
    let mut released = 0;
    for idx in 0..MAX_IRQ {
        if IRQ_WAITERS[idx].load(Ordering::Acquire) != tid {
            continue;
        }
        IRQ_MMIO_BASE[idx].store(0, Ordering::Release);
        if IRQ_WAITERS[idx]
            .compare_exchange(tid, NO_WAITER, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            IRQ_PENDING[idx].store(false, Ordering::Release);
            released += 1;
        }
    }
    released
}

/// Return `true` if any Driver Cell is currently registered for `irq`.
///
/// Called from the IRQ dispatcher to decide whether to route to `signal_irq`
/// (Cell-claimed) or the kernel-internal VirtIO driver (not yet migrated).
pub fn has_waiter(irq: u8) -> bool {
    let idx = irq as usize;
    if idx >= MAX_IRQ {
        return false;
    }
    IRQ_WAITERS[idx].load(Ordering::Acquire) != NO_WAITER
}

/// Return the MMIO base stored for `irq`, or 0 if none.
pub fn get_mmio_base(irq: u8) -> usize {
    let idx = irq as usize;
    if idx >= MAX_IRQ {
        return 0;
    }
    IRQ_MMIO_BASE[idx].load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The tables are process-global: each test owns distinct IRQ lines.

    #[test]
    fn release_frees_only_the_dead_owners_lines() {
        assert!(register_waiter(60, 901, 0x1000));
        assert!(register_waiter(61, 902, 0x2000));
        signal_irq_pending_only(60);

        assert_eq!(release_waiters_for(901), 1);
        assert!(!has_waiter(60));
        assert_eq!(get_mmio_base(60), 0);
        assert!(
            !take_pending(60),
            "a replacement must not inherit the dead owner's wake"
        );
        assert!(
            register_waiter(60, 903, 0x3000),
            "released line is claimable again"
        );

        assert!(has_waiter(61));
        assert_eq!(get_mmio_base(61), 0x2000);

        clear_waiter(60);
        clear_waiter(61);
    }

    #[test]
    fn release_of_a_non_waiter_changes_nothing() {
        assert!(register_waiter(62, 911, 0));
        assert_eq!(release_waiters_for(912), 0);
        assert_eq!(release_waiters_for(NO_WAITER), 0);
        assert!(has_waiter(62));
        clear_waiter(62);
    }

    /// `signal_irq` would ack a real MMIO base; tests set the flag directly.
    fn signal_irq_pending_only(irq: u8) {
        IRQ_PENDING[irq as usize].store(true, Ordering::Release);
    }
}
