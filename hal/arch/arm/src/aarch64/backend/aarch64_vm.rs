//! Hosted AArch64 VM backend (`board-aarch64-vm`): QEMU `virt` with a GICv3,
//! under any accelerator (TCG, Apple HVF, KVM).
//!
//! The device map is QEMU virt's and is reused from [`QemuVirt`]: PL011
//! console, PL031 RTC, PL061 GPIO, VirtIO MMIO slots, generic-timer tick and
//! PSCI secondaries. Only the interrupt controller differs: GICv3 with its
//! distributor and first redistributor region taken from the firmware tree
//! ([`firmware_gic`]). RAM comes from the tree's memory node
//! (`DtbPolicy::Optional`).
//!
//! The kernel derives a hart's MPIDR from its index (Aff0 = hart, Aff1 = 0),
//! which matches QEMU virt's GICv3 numbering (16 CPUs per Aff1 cluster) for up
//! to 16 cores.

mod firmware_gic;

use hal_soc_arm_virt::QEMU_ARM_VIRT;

use super::qemu_virt::{self, QemuVirt};
use super::{
    ArmIrqController, ArmPlatform, ArmResources, ArmSoc, ArmTimer, DisplayFirmware, DtbPolicy,
    MmioMap, MmioSpan, MmioWindow, NoEl2Monitor, NoVcpuExt, PlatformAnnounce, PlatformFacts,
    SdhciFacts, SecondaryStart, VgicWindows,
};
use crate::aarch64::gic::GicV2Layout;
use crate::aarch64::gicv3::{self, GicV3Layout};
use crate::aarch64::gicv3_layout::GicV3Windows;
use crate::aarch64::trap::TrapFrame;
use crate::aarch64::{timer, uart_pl011};

/// Hosted AArch64 virtual machine (QEMU virt map, GICv3).
pub struct Aarch64Vm;

impl ArmPlatform for Aarch64Vm {
    const PL011_BASE: usize = <QemuVirt as ArmPlatform>::PL011_BASE;
    const SECONDARY_START: SecondaryStart = SecondaryStart::Psci;
    const CONSOLE_RX_IRQ: bool = false;
    /// An EL2 entry keeps a VHE EL2 host, or drops to EL1 without FEAT_VHE;
    /// hosted accelerators (HVF, KVM) enter at EL1.
    const PI_MONITOR_BOOT: bool = false;
    const DTB_POLICY: DtbPolicy = DtbPolicy::Optional;
    const IDLE_PROBES: usize = 0;
    type VcpuExt = NoVcpuExt;
    type El2Monitor = NoEl2Monitor;
    /// Hosted loaders (QEMU `-kernel Image`) take a raw `Image` and place it
    /// at the link address.
    const LINUX_IMAGE_HEADER: bool = true;

    #[inline(always)]
    fn console_init() {
        uart_pl011::init();
    }

    #[inline(always)]
    fn early_putc(byte: u8) {
        uart_pl011::putchar(byte);
    }

    #[inline(always)]
    fn early_puts(s: &str) {
        uart_pl011::puts(s);
    }

    #[inline(always)]
    fn console_write_byte(byte: u8, _service_rx: impl Fn() + Copy) {
        uart_pl011::putchar(byte);
    }

    #[inline(always)]
    fn console_poll_rx() -> Option<u8> {
        uart_pl011::poll_rx()
    }

    #[inline(always)]
    fn console_enable_rx_irq() {}

    #[inline(always)]
    fn diag_probe(_byte: u8) {}

    #[inline(always)]
    fn irq_snapshot_probe() {}

    #[inline(always)]
    fn probe_uncategorized_trap(_frame: &TrapFrame, _vector_kind: u8) {}

    fn note_firmware_tree(tree: &fdt::Fdt<'_>) {
        firmware_gic::note(tree);
    }
}

impl GicV3Layout for Aarch64Vm {
    const GPIO_GIC_ID: u32 = <QemuVirt as GicV2Layout>::GPIO_GIC_ID;

    #[inline(always)]
    fn windows() -> GicV3Windows {
        firmware_gic::windows()
    }

    fn enable_board_irqs() {
        for irq in qemu_virt::board_spis() {
            gicv3::enable_spi::<Self>(irq);
        }
    }

    #[inline(always)]
    fn dispatch_spi(irq: u32) {
        <QemuVirt as GicV2Layout>::dispatch_spi(irq);
    }
}

impl ArmIrqController for Aarch64Vm {
    const GICV3_CPU_INTERFACE: bool = true;
    const HAS_IPI: bool = true;
    const USB_ONE_SHOT_IRQ: Option<u32> = None;

    fn init() {
        firmware_gic::resolve::<Self>();
        gicv3::init_distributor::<Self>();
        gicv3::init_cpu::<Self>();
    }

    #[inline(always)]
    fn init_cpu() {
        gicv3::init_cpu::<Self>();
    }

    #[inline(always)]
    fn send_ipi(_target_cpu: u32, target_mpidr: u64) {
        gicv3::send_sgi(target_mpidr, gicv3::SGI_IPI);
    }

    #[inline(always)]
    fn unmask_usb_irq() {}

    #[inline(always)]
    fn handle_irq() {
        gicv3::handle_gicv3_irq::<Self>();
    }

    /// Nothing is routed to Group 0, but a single-security-state GIC lets this
    /// EL take it; a spurious one is acknowledged rather than treated as fatal.
    #[inline(always)]
    fn handle_fiq() {
        gicv3::handle_gicv3_fiq::<Self>();
    }
}

impl ArmTimer for Aarch64Vm {
    /// The tick's PPI is enabled per core by `gicv3::init_cpu`.
    #[inline(always)]
    fn init() {
        timer::arm_generic();
    }

    #[inline(always)]
    fn reset() {
        timer::reset_generic();
    }
}

impl ArmResources for Aarch64Vm {
    /// The QEMU virt device map, so the kernel keeps its QEMU virt MMIO
    /// authority table; the kernel's `board-aarch64-vm` feature selects the
    /// `aarch64-vm` descriptor rather than the one this identity implies.
    const SOC: ArmSoc = ArmSoc::QemuArmVirt;
    const PLATFORM: PlatformFacts = PlatformFacts {
        announce: Some(PlatformAnnounce {
            label: "aarch64-vm (QEMU virt map, GICv3)",
            peripheral_base: QEMU_ARM_VIRT.peripheral_map.base,
        }),
        ..<QemuVirt as ArmResources>::PLATFORM
    };
    const KERNEL_MMIO_MAPS: &'static [MmioMap] = <QemuVirt as ArmResources>::KERNEL_MMIO_MAPS;
    const VIRTIO_MMIO: Option<hal_soc_arm_virt::VirtioMmioLayout> =
        <QemuVirt as ArmResources>::VIRTIO_MMIO;
    /// GICv3 virtualizes through `ICH_*` system registers, not a GICH window;
    /// the EL2 host's vGIC drives GICv2 list registers only.
    const VGIC: Option<VgicWindows> = None;
    const GPIO_MMIO_BASE: usize = <QemuVirt as ArmResources>::GPIO_MMIO_BASE;
    const DWC2_WINDOW: Option<MmioWindow> = None;
    const DISPLAY: Option<DisplayFirmware> = None;
    const SDHCI: Option<SdhciFacts> = None;
    const BOARD_PINMUX_GPIO_BASE: Option<usize> = None;
    const SUPERVISOR_DEVICE_MMIO: &'static [MmioSpan] =
        <QemuVirt as ArmResources>::SUPERVISOR_DEVICE_MMIO;
}
