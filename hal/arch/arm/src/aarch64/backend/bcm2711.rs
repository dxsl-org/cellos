//! BCM2711 (Raspberry Pi 4) backend: PL011 console at the profile's UART0,
//! GIC-400, generic-timer tick, and PSCI secondaries.

use hal_soc_bcm27xx::BCM2711;

use super::{
    ArmIrqController, ArmPlatform, ArmResources, ArmSoc, ArmTimer, DisplayFirmware, DtbPolicy,
    MmioMap, MmioSpan, MmioWindow, NoEl2Monitor, NoIommu, NoVcpuExt, PlatformFacts, SdhciFacts,
    SecondaryStart, VgicWindows,
};
use crate::aarch64::gic::{self, GicV2Layout};
use crate::aarch64::trap::{self, TrapFrame};
use crate::aarch64::{timer, uart_pl011};

/// Raspberry Pi 4 (BCM2711).
pub struct Bcm2711;

impl ArmPlatform for Bcm2711 {
    type Iommu = NoIommu;

    const PL011_BASE: usize = BCM2711.mmio.uart_base;
    const SECONDARY_START: SecondaryStart = SecondaryStart::Psci;
    const CONSOLE_RX_IRQ: bool = false;
    /// Boot installs the Pi EL2 monitor, but this backend never initialises
    /// or uses it ([`NoEl2Monitor`]).
    const PI_MONITOR_BOOT: bool = true;
    const DTB_POLICY: DtbPolicy = DtbPolicy::Required;
    const IDLE_PROBES: usize = 0;
    type VcpuExt = NoVcpuExt;
    type El2Monitor = NoEl2Monitor;

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

impl GicV2Layout for Bcm2711 {
    const DISTRIBUTOR_BASE: usize = BCM2711.mmio.gic_distributor_base;
    const CPU_INTERFACE_BASE: usize = BCM2711.mmio.gic_cpu_base;
    /// No GPIO controller interrupt is routed through the GIC.
    const GPIO_GIC_ID: u32 = u32::MAX;

    #[inline(always)]
    fn enable_board_irqs() {}

    #[inline(always)]
    fn dispatch_spi(_irq: u32) {}
}

impl ArmIrqController for Bcm2711 {
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

impl ArmTimer for Bcm2711 {
    #[inline(always)]
    fn init() {
        timer::init_gic::<Self>();
    }

    #[inline(always)]
    fn reset() {
        timer::reset_generic();
    }
}

/// `base + size`, or a compile-time error when a profile window overflows.
const fn window_end(base: usize, size: usize) -> usize {
    match base.checked_add(size) {
        Some(end) => end,
        None => panic!("BCM2711 MMIO window overflows the address space"),
    }
}

impl ArmResources for Bcm2711 {
    const SOC: ArmSoc = ArmSoc::Bcm2711;

    const PLATFORM: PlatformFacts = PlatformFacts {
        uart_base: BCM2711.mmio.uart_base,
        uart_irq: 0,
        rtc_base: None,
        published_virtio_slots: 0,
        announce: None,
    };

    const KERNEL_MMIO_MAPS: &'static [MmioMap] = &[
        // GPIO, PL011 UART, SDHCI: kernel-only at boot. `RequestMmio` sets USER
        // on the GPIO/UART pages a Cell claims; EMMC2 (ADMA: it can write any
        // RAM) is never in the allowlist, so no Cell ever reaches it.
        MmioMap {
            base: BCM2711.mmio.gpio_base,
            end: window_end(BCM2711.mmio.gpio_base, BCM2711.mmio.gpio_grant_size),
            cell_access: false,
        },
        MmioMap {
            base: BCM2711.mmio.uart_base,
            end: window_end(BCM2711.mmio.uart_base, BCM2711.mmio.uart_grant_size),
            cell_access: false,
        },
        MmioMap {
            base: BCM2711.mmio.sdhci_base,
            end: window_end(BCM2711.mmio.sdhci_base, BCM2711.mmio.sdhci_grant_size),
            cell_access: false,
        },
        // GIC-400 distributor and CPU interface (kernel-only).
        MmioMap {
            base: BCM2711.mmio.gic_distributor_base,
            end: window_end(
                BCM2711.mmio.gic_distributor_base,
                BCM2711.mmio.gic_distributor_size,
            ),
            cell_access: false,
        },
        MmioMap {
            base: BCM2711.mmio.gic_cpu_base,
            end: window_end(BCM2711.mmio.gic_cpu_base, BCM2711.mmio.gic_cpu_size),
            cell_access: false,
        },
    ];

    const VIRTIO_MMIO: Option<hal_soc_arm_virt::VirtioMmioLayout> = None;
    const VGIC: Option<VgicWindows> = None;
    const GPIO_MMIO_BASE: usize = BCM2711.mmio.gpio_base;
    const DWC2_WINDOW: Option<MmioWindow> = None;
    const DISPLAY: Option<DisplayFirmware> = None;
    const SDHCI: Option<SdhciFacts> = Some(SdhciFacts {
        base: BCM2711.mmio.sdhci_base,
        word_access_only: BCM2711.sdhci.word_access_only,
        minimum_write_spacing_us: BCM2711.sdhci.minimum_write_spacing_us,
        pinmux_gpio_base: Some(BCM2711.mmio.gpio_base),
    });
    const BOARD_PINMUX_GPIO_BASE: Option<usize> = None;
    const SUPERVISOR_DEVICE_MMIO: &'static [MmioSpan] = &[];
}

// H-18: no boot map gives EL0 any part of the board's MMIO; a Cell reaches a
// window only through a live `RequestMmio` grant. Fails the build otherwise.
const _: () = {
    let maps = <Bcm2711 as ArmResources>::KERNEL_MMIO_MAPS;
    let mut i = 0;
    while i < maps.len() {
        assert!(
            !maps[i].cell_access,
            "BCM2711 boot MMIO maps must be kernel-only"
        );
        i += 1;
    }
};
