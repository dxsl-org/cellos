// SPDX-License-Identifier: MPL-2.0
//! aarch64 platform facts: the board backend's MMIO layout, published once
//! into `crate::platform`.

use crate::platform::{publish_boot, PlatformInfo, VirtioEntry};

// ── AArch64: board facts from the backend ────────────────────────────────────
// QEMU ARM virt publishes PL011, PL031 and four VirtIO MMIO slots; the BCM
// boards publish their console UART only (no VirtIO MMIO, no RTC).
pub fn init(_dtb_ptr: usize) {
    use hal::aarch64::backend::{ArmResources, Board};
    let board = crate::board::active();
    let facts = <Board as ArmResources>::PLATFORM;
    if let Some(rtc_base) = facts.rtc_base {
        hal::rtc::init(rtc_base);
    }
    let info = PlatformInfo {
        uart_base: facts.uart_base,
        uart_irq: facts.uart_irq,
        plic_base: 0,
        plic_size: 0,
        clint_base: 0,
        virtio_mmio: match <Board as ArmResources>::VIRTIO_MMIO {
            Some(layout) => arm_virtio_slots(layout, facts.published_virtio_slots),
            None => [None; 8],
        },
        rtc_base: facts.rtc_base.unwrap_or(0),
    };
    // SAFETY: `kmain` invokes this once during single-core boot.
    unsafe { publish_boot(info) };
    if let Some(announce) = facts.announce {
        log::info!(
            "[platform] {}: UART={:#x} periph={:#x} fallback-end={:#x}",
            announce.label,
            facts.uart_base,
            announce.peripheral_base,
            board.fallback_memory[1].base + board.fallback_memory[1].size
        );
    }
}

fn arm_virtio_slots(
    layout: hal_soc_arm_virt::VirtioMmioLayout,
    limit: usize,
) -> [Option<VirtioEntry>; 8] {
    let mut entries = [None, None, None, None, None, None, None, None];
    let mut index = 0;
    while index < layout.count && index < entries.len() && index < limit {
        entries[index] = Some(VirtioEntry {
            base: layout
                .slot_base(index)
                .expect("validated ARM VirtIO layout"),
            irq: layout.first_spi + index as u32,
        });
        index += 1;
    }
    entries
}
