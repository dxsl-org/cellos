//! Shared VirtIO MMIO slot enumeration.
//!
//! `virtio_blk`, `input_irq_ack`, and Driver Cells use `virtio_slots()` to
//! iterate all VirtIO MMIO slots for the current platform.
//!
//! AArch64: scans all 32 slots at 0x0a000000, stride 0x200 (QEMU virt layout).
//! QEMU assigns devices to slots in an implementation-defined order so we must
//! probe all 32.  The identity map in paging.rs covers the full 0x0a004000 range.
//!
//! Other arches: reads DTB-confirmed slots from `platform::PLATFORM`.

extern crate alloc;
use alloc::vec::Vec;

/// A VirtIO MMIO slot with base address and IRQ.
pub struct VirtioSlot {
    pub base: usize,
    pub irq: u32,
}

/// Iterator over all VirtIO MMIO slots for the current platform.
pub fn virtio_slots() -> impl Iterator<Item = VirtioSlot> {
    #[cfg(target_arch = "aarch64")]
    {
        // QEMU ARM virt: 32 VirtIO MMIO slots at 0x0a000000, 512 bytes each, SPI 16+i.
        // All 32 slots are identity-mapped by init_kernel_paging (0x0a000000..0x0a004000).
        const BASE: usize = 0x0a00_0000;
        const STRIDE: usize = 0x200;
        let slots: Vec<VirtioSlot> = (0..32_usize)
            .map(|i| VirtioSlot {
                base: BASE + i * STRIDE,
                irq: 16 + i as u32,
            })
            .collect();
        slots.into_iter()
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
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
}

/// VirtIO MMIO IRQ dispatcher — called from the arch trap handlers (riscv64/aarch64)
/// when any VirtIO MMIO IRQ fires. Routes the IRQ to whichever Driver Cell registered
/// for it via `sys_wait_irq` (the block / net / gpu cells all rely on this), ACKs the
/// input slot when the input service Cell isn't up yet, and warns on an unclaimed slot.
///
/// Relocated here from the deleted kernel `virtio_blk` driver (G2 loader redesign
/// phase 06): it is VirtIO-common, not block-specific. The former kernel-block ACK
/// branch is gone — the virtio-blk Driver Cell now ACKs its own IRQ via the
/// `sys_wait_irq` path above.
#[no_mangle]
pub extern "Rust" fn vi_handle_virtio_irq(irq: u32) {
    // Driver Cell IRQ routing: a Cell registered for this IRQ via sys_wait_irq —
    // signal it (sets IRQ_PENDING + writes VirtIO InterruptACK) and return.
    if crate::task::drivers::irq_wait::has_waiter(irq as u8) {
        crate::task::drivers::irq_wait::signal_irq(irq as u8);
        return;
    }
    // Input (keyboard) slot: ACK to prevent an interrupt storm before the input
    // service Cell is up; event routing lives entirely in that Cell.
    if crate::task::drivers::input_irq_ack::ack_if_input(irq) {
        return;
    }
    // Unknown VirtIO slot — no device registered. InterruptStatus is already cleared
    // by plic_complete in the trap handler.
    log::warn!(
        "[virtio] unhandled IRQ {} — no registered device for this slot",
        irq
    );
}
