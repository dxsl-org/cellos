// SPDX-License-Identifier: MPL-2.0
//! `virtio_mmio`: the board backend's VirtIO MMIO window, for
//! `crate::dev::virtio_common`.

use crate::dev::virtio_common::VirtioSlot;
use alloc::vec::Vec;

/// Every VirtIO MMIO slot of the board backend's layout.
pub(crate) fn slots() -> alloc::vec::IntoIter<VirtioSlot> {
    use hal::aarch64::backend::{ArmResources, Board};
    let slots: Vec<VirtioSlot> = match <Board as ArmResources>::VIRTIO_MMIO {
        Some(layout) => (0..layout.count)
            .map(|i| VirtioSlot {
                base: layout.slot_base(i).expect("bounded VirtIO slot"),
                irq: layout.spi(i).expect("bounded VirtIO IRQ"),
            })
            .collect(),
        None => Vec::new(),
    };
    slots.into_iter()
}

/// Base of the slot whose interrupt is `irq`, if any.
pub(crate) fn slot_base_for_irq(irq: u32) -> Option<usize> {
    <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::VIRTIO_MMIO.and_then(
        |layout| {
            irq.checked_sub(layout.first_spi)
                .and_then(|index| layout.slot_base(index as usize))
        },
    )
}

/// Run one VirtIO MMIO ACK sequence: no privilege change is needed here.
pub(crate) fn with_ack_access<T>(ack: impl FnOnce() -> T) -> T {
    ack()
}

const _: crate::hal::HandleVirtioIrq = crate::dev::virtio_common::vi_handle_virtio_irq;
