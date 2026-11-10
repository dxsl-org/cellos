//! GICv3 windows of the hosted VM: recorded from the firmware tree's
//! `arm,gic-v3` node, validated once the console is up, then read on every
//! distributor or redistributor access.
//!
//! A direct ELF boot passes no tree; QEMU virt's fixed GICv3 map
//! (`hw/arm/virt.c`: GICD 0x0800_0000, GICR 0x080A_0000 + 0xF6_0000) applies.

use core::fmt::Write;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use hal_soc_arm_virt::QEMU_ARM_VIRT;

use crate::aarch64::backend::{ArmResources, MmioMap};
use crate::aarch64::gicv3_layout::{select_windows, GicV3Windows};
use crate::aarch64::uart_pl011;

/// No firmware tree was read: the QEMU virt defaults stand.
const SOURCE_DEFAULT: u8 = 0;
/// The tree's `arm,gic-v3` node supplied the `reg` entries.
const SOURCE_TREE: u8 = 1;
/// The tree has no `arm,gic-v3` node (e.g. QEMU `gic-version=2`).
const SOURCE_TREE_WITHOUT_GICV3: u8 = 2;

/// Raw `reg` entries until [`resolve`] validates them, then the windows in
/// use. Written only on the boot core before any other core runs.
struct GicWindows {
    source: AtomicU8,
    /// `reg` entries recorded (at most 2: distributor, first redistributor region).
    reg_len: AtomicU8,
    distributor: AtomicUsize,
    distributor_size: AtomicUsize,
    redistributors: AtomicUsize,
    redistributors_size: AtomicUsize,
}

static GIC: GicWindows = GicWindows {
    source: AtomicU8::new(SOURCE_DEFAULT),
    reg_len: AtomicU8::new(2),
    distributor: AtomicUsize::new(QEMU_ARM_VIRT.gic_distributor.base),
    distributor_size: AtomicUsize::new(QEMU_ARM_VIRT.gic_distributor.size),
    redistributors: AtomicUsize::new(0x080A_0000),
    redistributors_size: AtomicUsize::new(0x00F6_0000),
};

/// Record the tree's GICv3 description; validation waits for [`resolve`],
/// which runs with a console to report a refusal on.
pub(super) fn note(tree: &fdt::Fdt<'_>) {
    let Some(node) = tree.find_compatible(&["arm,gic-v3"]) else {
        GIC.source
            .store(SOURCE_TREE_WITHOUT_GICV3, Ordering::Relaxed);
        return;
    };
    let mut reg_len = 0u8;
    for (index, region) in node.reg().into_iter().flatten().take(2).enumerate() {
        let (base, size) = if index == 0 {
            (&GIC.distributor, &GIC.distributor_size)
        } else {
            (&GIC.redistributors, &GIC.redistributors_size)
        };
        base.store(region.starting_address as usize, Ordering::Relaxed);
        size.store(region.size.unwrap_or(0), Ordering::Relaxed);
        reg_len += 1;
    }
    GIC.reg_len.store(reg_len, Ordering::Relaxed);
    GIC.source.store(SOURCE_TREE, Ordering::Relaxed);
}

/// The windows in use.
#[inline(always)]
pub(super) fn windows() -> GicV3Windows {
    GicV3Windows {
        distributor: GIC.distributor.load(Ordering::Relaxed),
        redistributors: GIC.redistributors.load(Ordering::Relaxed),
        redistributors_size: GIC.redistributors_size.load(Ordering::Relaxed),
    }
}

/// Validate the recorded windows against `maps` (the kernel's MMIO identity
/// maps) and report them on the boot console.
///
/// # Panics
/// When the tree describes no GICv3 or one the kernel cannot reach: every
/// interrupt depends on it, and a guessed window would fault on first access.
pub(super) fn resolve<R: ArmResources>() {
    let source = GIC.source.load(Ordering::Relaxed);
    if source == SOURCE_TREE_WITHOUT_GICV3 {
        panic!(
            "[gicv3] firmware tree has no arm,gic-v3 node; aarch64-vm needs a GICv3 \
             (QEMU: -M virt,gic-version=3)"
        );
    }
    let reg = [
        (
            GIC.distributor.load(Ordering::Relaxed),
            Some(GIC.distributor_size.load(Ordering::Relaxed)),
        ),
        (
            GIC.redistributors.load(Ordering::Relaxed),
            Some(GIC.redistributors_size.load(Ordering::Relaxed)),
        ),
    ];
    let reg_len = usize::from(GIC.reg_len.load(Ordering::Relaxed)).min(reg.len());
    let mapped = |base: usize, end: usize| {
        R::KERNEL_MMIO_MAPS
            .iter()
            .any(|map: &MmioMap| map.base <= base && end <= map.end)
    };
    let windows = select_windows(&reg[..reg_len], mapped)
        .unwrap_or_else(|error| panic!("[gicv3] unusable GICv3 description: {error:?}"));
    let _ = writeln!(
        Console,
        "[gicv3] GICD={:#x} GICR={:#x}+{:#x} ({})",
        windows.distributor,
        windows.redistributors,
        windows.redistributors_size,
        if source == SOURCE_TREE {
            "firmware tree"
        } else {
            "no firmware tree: QEMU virt default"
        }
    );
}

/// Boot-console writer for the one-line report (before the kernel logger).
struct Console;

impl Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        uart_pl011::puts(s);
        Ok(())
    }
}
