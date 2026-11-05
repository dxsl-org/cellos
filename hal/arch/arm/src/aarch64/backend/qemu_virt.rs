//! QEMU `virt` backend: PL011 console at the `ArmVirtProfile` UART, GICv2,
//! generic-timer tick, PSCI secondaries, and VirtIO MMIO/PL061 device IRQs.

use hal_arch_trait::vi_handle_virtio_irq;
use hal_soc_arm_virt::{ArmVirtProfile, QEMU_ARM_VIRT};

use super::{
    ArmIrqController, ArmPlatform, ArmResources, ArmSoc, ArmTimer, DisplayFirmware, DtbPolicy,
    HostRegime, MmioMap, MmioSpan, MmioWindow, NoEl2Monitor, NoIommu, NoVcpuExt, PlatformFacts,
    SdhciFacts, SecondaryStart, VgicWindows,
};
use crate::aarch64::gic::{self, GicV2Layout};
use crate::aarch64::trap::{self, TrapFrame};
use crate::aarch64::{timer, uart_pl011};

/// The `virt` host regime, chosen at build time by `CELLOS_AARCH64_HOST_REGIME`:
/// absent or `nonvhe` keeps the non-VHE EL2 host, `vhe` selects the VHE host
/// (the `virt-vhe` lane). rustc records the variable as a build input, so
/// changing it rebuilds this crate. Any other value fails the build.
///
/// A cargo feature is the board-selection convention; this variable stands in
/// for one until the feature lines land (phase 02a deviation log).
const VIRT_HOST_REGIME: HostRegime = match option_env!("CELLOS_AARCH64_HOST_REGIME") {
    None => HostRegime::El2NonVhe,
    Some(value) if str_eq(value, "nonvhe") => HostRegime::El2NonVhe,
    Some(value) if str_eq(value, "vhe") => HostRegime::El2Vhe,
    Some(_) => panic!("CELLOS_AARCH64_HOST_REGIME must be absent, `nonvhe` or `vhe`"),
};

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// QEMU `virt` machine.
pub struct QemuVirt;

impl ArmPlatform for QemuVirt {
    type Iommu = NoIommu;

    const PL011_BASE: usize = QEMU_ARM_VIRT.uart.mmio.base;
    const SECONDARY_START: SecondaryStart = SecondaryStart::Psci;
    const CONSOLE_RX_IRQ: bool = false;
    /// The host keeps EL2 when firmware enters there (`virtualization=on`).
    const PI_MONITOR_BOOT: bool = false;
    const DTB_POLICY: DtbPolicy = DtbPolicy::Optional;
    const IDLE_PROBES: usize = 0;
    type VcpuExt = NoVcpuExt;
    type El2Monitor = NoEl2Monitor;
    const HOST_REGIME: HostRegime = VIRT_HOST_REGIME;
    /// QEMU loads a raw `Image` at RAM base + `text_offset`, which is exactly
    /// the link address (`linker-aarch64.ld`).
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
}

impl GicV2Layout for QemuVirt {
    const DISTRIBUTOR_BASE: usize = QEMU_ARM_VIRT.gic_distributor.base;
    const CPU_INTERFACE_BASE: usize = QEMU_ARM_VIRT.gic_cpu.base;
    /// PL061 GPIO controller (SPI 7 = GIC ID 39).
    const GPIO_GIC_ID: u32 = ArmVirtProfile::gic_id_for_spi(QEMU_ARM_VIRT.gpio.spi);

    #[inline(always)]
    fn enable_board_irqs() {
        // Enable VirtIO MMIO IRQs: QEMU virt assigns SPI 16..47 (GIC IDs 48..79)
        // to the 32 VirtIO MMIO slots.  Without this, GICD_ISENABLER bit is 0 and
        // the GIC never delivers VirtIO interrupts even after claim/complete.
        // NIC is at slot 30 (SPI 46, GIC ID 78); Block at slot 31 (SPI 47, GIC ID 79).
        let virtio = QEMU_ARM_VIRT.virtio;
        let first = ArmVirtProfile::gic_id_for_spi(virtio.first_spi);
        for i in first..first + virtio.count as u32 {
            gic::enable_irq::<Self>(i);
        }
        gic::enable_irq::<Self>(ArmVirtProfile::gic_id_for_spi(QEMU_ARM_VIRT.gpio.spi));
    }

    #[inline(always)]
    fn dispatch_spi(irq: u32) {
        // VirtIO MMIO: convert GIC ID → SPI number.
        // SAFETY: vi_handle_virtio_irq is #[no_mangle] in kernel/src/task/drivers.
        unsafe {
            vi_handle_virtio_irq(irq - ArmVirtProfile::GIC_SPI_OFFSET);
        }
    }
}

impl ArmIrqController for QemuVirt {
    const HAS_IPI: bool = true;
    const USB_ONE_SHOT_IRQ: Option<u32> = None;

    #[inline(always)]
    fn init() {
        gic::init::<Self>();
    }

    #[inline(always)]
    fn init_cpu() {
        gic::init_cpu::<Self>();
    }

    #[inline(always)]
    fn send_ipi(target_cpu: u32) {
        gic::send_sgi::<Self>(target_cpu, gic::SGI_IPI);
    }

    #[inline(always)]
    fn unmask_usb_irq() {}

    #[inline(always)]
    fn handle_irq() {
        trap::handle_gic_irq::<Self>();
    }
}

impl ArmTimer for QemuVirt {
    #[inline(always)]
    fn init() {
        timer::init_gic::<Self>();
    }

    #[inline(always)]
    fn reset() {
        timer::reset_generic();
    }
}

impl ArmResources for QemuVirt {
    const SOC: ArmSoc = ArmSoc::QemuArmVirt;

    /// PL011 UART at 0x0900_0000; PL031 RTC at 0x0901_0000; the first four of
    /// the 32 VirtIO MMIO slots at 0x0a000000 (512 bytes each, SPI 16+i).
    const PLATFORM: PlatformFacts = PlatformFacts {
        uart_base: QEMU_ARM_VIRT.uart.mmio.base,
        uart_irq: QEMU_ARM_VIRT.uart.spi,
        rtc_base: Some(QEMU_ARM_VIRT.rtc.base),
        published_virtio_slots: 4,
        announce: None,
    };

    const KERNEL_MMIO_MAPS: &'static [MmioMap] = &[
        // GIC (EL1-only): cells must not modify interrupt routing.
        MmioMap {
            base: QEMU_ARM_VIRT.gic_map.base,
            end: QEMU_ARM_VIRT.gic_map.end(),
            cell_access: false,
        },
        // Peripheral MMIO (GPIO PL061, UART PL011, RTC): cells access directly via
        // driver rlibs (bit-bang GPIO/I2C/SPI/PWM). LBI guarantees only cells with
        // the GPIO capability can construct driver objects, so USER access is safe.
        MmioMap {
            base: QEMU_ARM_VIRT.peripheral_map.base,
            end: QEMU_ARM_VIRT.peripheral_map.end(),
            cell_access: true,
        },
        MmioMap {
            base: QEMU_ARM_VIRT.virtio.region().base,
            end: QEMU_ARM_VIRT.virtio.region().end(),
            cell_access: true,
        },
        MmioMap {
            base: QEMU_ARM_VIRT.platform_bus_map.base,
            end: QEMU_ARM_VIRT.platform_bus_map.end(),
            cell_access: false,
        },
        // PCIe ECAM bus-0 window (1 MiB at 0x3F00_0000) for ARM64 virt gpex.
        // Required before pcie_ecam::init() accesses config space.
        MmioMap {
            base: QEMU_ARM_VIRT.pcie_ecam_bus0.base,
            end: QEMU_ARM_VIRT.pcie_ecam_bus0.end(),
            cell_access: false,
        },
    ];

    const VIRTIO_MMIO: Option<hal_soc_arm_virt::VirtioMmioLayout> = Some(QEMU_ARM_VIRT.virtio);
    const VGIC: Option<VgicWindows> = Some(VgicWindows {
        gich_base: QEMU_ARM_VIRT.gic_hyp.base,
        gicv: MmioWindow {
            base: QEMU_ARM_VIRT.gic_vcpu.base,
            size: QEMU_ARM_VIRT.gic_vcpu.size,
        },
    });
    const GPIO_MMIO_BASE: usize = QEMU_ARM_VIRT.gpio.mmio.base;
    const DWC2_WINDOW: Option<MmioWindow> = None;
    const DISPLAY: Option<DisplayFirmware> = None;
    const SDHCI: Option<SdhciFacts> = None;
    const BOARD_PINMUX_GPIO_BASE: Option<usize> = None;
    /// GIC map and the peripheral window (PL011, PL031, PL061).
    const SUPERVISOR_DEVICE_MMIO: &'static [MmioSpan] = &[
        MmioSpan {
            base: QEMU_ARM_VIRT.gic_map.base,
            end: QEMU_ARM_VIRT.gic_map.end(),
        },
        MmioSpan {
            base: QEMU_ARM_VIRT.peripheral_map.base,
            end: QEMU_ARM_VIRT.peripheral_map.end(),
        },
    ];
}
