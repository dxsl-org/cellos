// SPDX-License-Identifier: MPL-2.0
//! `virtio_mmio` for the 32-bit targets: no VirtIO MMIO window is wired up.

use crate::dev::virtio_common::VirtioSlot;
use alloc::vec::Vec;

/// No VirtIO MMIO slots.
pub(crate) fn slots() -> alloc::vec::IntoIter<VirtioSlot> {
    Vec::new().into_iter()
}

/// No slot base to derive and nothing to acknowledge.
pub(crate) fn slot_base_for_irq(irq: u32) -> Option<usize> {
    let _ = irq;
    None
}

/// Run one VirtIO MMIO ACK sequence: no privilege change is needed here.
pub(crate) fn with_ack_access<T>(ack: impl FnOnce() -> T) -> T {
    ack()
}
