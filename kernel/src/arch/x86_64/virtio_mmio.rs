// SPDX-License-Identifier: MPL-2.0
//! `virtio_mmio`: x86_64 has no VirtIO MMIO window (VirtIO is PCI there).

use crate::dev::virtio_common::VirtioSlot;
use alloc::vec::Vec;

/// No VirtIO MMIO slots.
pub(crate) fn slots() -> alloc::vec::IntoIter<VirtioSlot> {
    Vec::new().into_iter()
}

/// x86_64 routes VirtIO through PCI MSI-X, not a fixed MMIO IRQ window, so
/// there is no slot base to derive and nothing to acknowledge here.
pub(crate) fn slot_base_for_irq(irq: u32) -> Option<usize> {
    let _ = irq;
    None
}

/// Run one VirtIO MMIO ACK sequence: no privilege change is needed here.
pub(crate) fn with_ack_access<T>(ack: impl FnOnce() -> T) -> T {
    ack()
}
