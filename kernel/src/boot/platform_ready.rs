// SPDX-License-Identifier: MPL-2.0
//! What the architecture boot phases hand the arch-neutral part of `kmain`.
//!
//! `kmain` runs `crate::arch::boot::early` (console, traps, boot memory,
//! paging), then the heap, then `crate::arch::boot::platform_init`, which
//! returns a [`PlatformReady`]. Everything after it is arch-neutral. The order
//! the architecture phases enforce is written into these types: a
//! `PlatformReady` cannot exist without an [`IommuReady`], so DMA isolation is
//! in its boot state before the tail can spawn the first DMA-capable cell.

use super::{BootInfo, LimineBootInfo, SimpleBootInfo};

/// The boot memory description `kmain` works from: Limine's answer, or the
/// architecture's fallback when no Limine request was answered.
pub enum BootInfoSource {
    Limine(LimineBootInfo),
    Fallback(&'static SimpleBootInfo),
}

impl BootInfoSource {
    /// The description, whichever source supplied it.
    pub fn get(&self) -> &dyn BootInfo {
        match self {
            Self::Limine(info) => info,
            Self::Fallback(info) => *info,
        }
    }
}

/// The platform after `crate::arch::boot::platform_init`.
pub struct PlatformReady {
    pub console: ConsoleReady,
    pub irq: IrqReady,
    pub clock: ClockReady,
    /// The PCIe window the Platform Cell scans; `None` when there is none.
    pub pci: Option<PciReady>,
    /// Required: DMA isolation state before the first DMA-capable cell.
    pub iommu: IommuReady,
    pub storage: BootStorage,
}

/// The log backend is registered and the console RX path armed (or refused).
/// Keeps the boot description for the late framebuffer console (x86_64).
pub struct ConsoleReady {
    pub boot_info: BootInfoSource,
}

/// The interrupt controllers are set up: the PLIC (riscv64), the GIC from
/// `ARCH.init` (aarch64), the LAPIC and I/O APICs or a logged refusal (x86_64).
pub struct IrqReady;

impl IrqReady {
    /// Enable interrupts on the boot CPU. Needs the tick source set up too.
    pub fn enable(self, _clock: ClockReady) {
        crate::arch::irq::enable();
    }
}

/// The tick source is set up (x86_64: the `init_timers` outcome, which may be
/// a logged refusal; riscv64 and aarch64: from `ARCH.init`).
pub struct ClockReady;

/// The PCIe window the Platform Cell (PCIe ECAM scanner) is spawned for.
pub enum PciReady {
    /// The compile-time GPEX window (riscv64); the cell needs no argv.
    Gpex,
    /// The validated ACPI MCFG window (x86_64), passed to the cell by argv.
    Ecam {
        base: u64,
        bus_start: u8,
        bus_end: u8,
    },
}

/// DMA isolation when the platform phase returns.
pub enum IommuReady {
    /// `iommu::init` (VT-d from ACPI DMAR) and `activate_isolation` ran before
    /// any DMA-capable driver (x86_64).
    VtdActive,
    /// Activation waits for the Platform Cell to register the IOMMU device
    /// (`iommu::try_deferred_init`, riscv64).
    Deferred,
    /// No IOMMU in the boot path.
    Absent { reason: &'static str },
}

/// The boot block storage the early loader reads.
pub enum BootStorage {
    /// VirtIO MMIO block (riscv64, aarch64): snapshot restore, MBR check and
    /// the cell bootstrap table.
    VirtioMmio,
    /// No kernel block device (x86_64 serves cells from the embedded ramdisk).
    None,
}
