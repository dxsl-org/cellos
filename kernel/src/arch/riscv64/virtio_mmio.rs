// SPDX-License-Identifier: MPL-2.0
//! `virtio_mmio`: the platform's VirtIO MMIO window, for
//! `crate::dev::virtio_common`.

use crate::dev::virtio_common::VirtioSlot;
use alloc::vec::Vec;

/// Every VirtIO MMIO slot the platform discovered.
pub(crate) fn slots() -> alloc::vec::IntoIter<VirtioSlot> {
    let slots: Vec<VirtioSlot> = crate::platform::with(|p| {
        p.virtio_mmio
            .iter()
            .filter_map(|e| {
                e.as_ref().map(|e| VirtioSlot {
                    base: e.base,
                    irq: e.irq,
                })
            })
            .collect()
    });
    slots.into_iter()
}

/// Base of the slot whose interrupt is `irq`, if any.
pub(crate) fn slot_base_for_irq(irq: u32) -> Option<usize> {
    crate::platform::virtio_mmio_base_for_irq(irq)
}

/// Run one VirtIO MMIO ACK sequence with SUM set for exactly that scope.
pub(crate) fn with_ack_access<T>(ack: impl FnOnce() -> T) -> T {
    // This helper is used only for short-lived VirtIO MMIO ACK sequences. It
    // records the prior SUM bit, sets only SUM for the duration of `ack`, and
    // restores the exact prior SUM state before returning. It does not widen trap
    // entry globally or alter SSIE/SEIE/STIE.
    const SUM_MASK: usize = 1usize << 18;
    let sstatus = crate::arch::user_access::virtio_ack_status();
    if sstatus & SUM_MASK == 0 {
        crate::arch::user_access::sum_mask_set();
    }
    let result = ack();
    if sstatus & SUM_MASK == 0 {
        crate::arch::user_access::sum_mask_clear();
    }
    result
}
