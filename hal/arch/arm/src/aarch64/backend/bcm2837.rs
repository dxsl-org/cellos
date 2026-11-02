//! BCM2837 (Raspberry Pi 3) backend: mini UART (AUX UART1) console with
//! interrupt-driven RX and single-byte boot probes, the BCM2836 local and
//! BCM2835 legacy interrupt controllers, the BCM2835 system-timer tick, and a
//! single hart.

pub mod bcm2835_legacy_irq;
pub mod bcm2835_systimer;
pub mod bcm2836_irq;
pub mod timer;
mod trap;
pub mod uart_bcm_mini;

use hal_soc_bcm27xx::BCM2837;

use super::{
    ArmIrqController, ArmPlatform, ArmResources, ArmSoc, ArmTimer, DisplayFirmware, MmioMap,
    MmioWindow, NoIommu, PlatformAnnounce, PlatformFacts, SdhciFacts, SecondaryStart,
};
use crate::aarch64::trap::TrapFrame;

/// Raspberry Pi 3 (BCM2837).
pub struct Bcm2837;

impl ArmPlatform for Bcm2837 {
    type Iommu = NoIommu;

    /// The console is the mini UART, so no RPi3 path routes output through the
    /// PL011 driver; it keeps the QEMU `virt` address it has always had here.
    const PL011_BASE: usize = hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.base;
    /// The BCM2836 local controller has no software-interrupt path in this
    /// kernel and firmware parks the secondaries, so the board stays single-hart.
    const SECONDARY_START: SecondaryStart =
        SecondaryStart::None("BCM2836 has no SGI path in this kernel — keeping Cellos single-hart");
    const CONSOLE_RX_IRQ: bool = true;

    #[inline(always)]
    fn console_init() {
        uart_bcm_mini::init();
    }

    #[inline(always)]
    fn early_putc(byte: u8) {
        uart_bcm_mini::putchar(byte);
    }

    #[inline(always)]
    fn early_puts(s: &str) {
        uart_bcm_mini::puts(s);
    }

    fn console_write_byte(byte: u8, service_rx: impl Fn() + Copy) {
        if byte == b'\n' {
            Self::console_write_byte(b'\r', service_rx);
        }
        loop {
            // Synchronous exceptions mask IRQs while `sys_log` prints. Keep draining
            // the eight-symbol RX FIFO into the 4096-byte kernel ring so full-duplex
            // terminal traffic cannot overrun while TX waits for space.
            service_rx();
            if uart_bcm_mini::try_putchar(byte) {
                break;
            }
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    fn console_poll_rx() -> Option<u8> {
        uart_bcm_mini::poll_rx()
    }

    #[inline(always)]
    fn console_enable_rx_irq() {
        uart_bcm_mini::enable_rx_interrupt();
    }

    #[inline(always)]
    fn diag_probe(byte: u8) {
        uart_bcm_mini::probe_put(byte);
    }

    #[inline(always)]
    fn probe_uncategorized_trap(frame: &TrapFrame, vector_kind: u8) {
        trap::probe_uncategorized_el2_fault(frame, vector_kind);
    }
}

impl ArmIrqController for Bcm2837 {
    /// No SGI path: the BCM2836 local controller has no software interrupt in
    /// this kernel, and the board starts no secondary.
    const HAS_IPI: bool = false;
    const USB_ONE_SHOT_IRQ: Option<u32> = Some(bcm2835_legacy_irq::USB_IRQ);

    #[inline(always)]
    fn init() {
        bcm2836_irq::init();
        bcm2835_legacy_irq::init();
    }

    #[inline(always)]
    fn init_cpu() {
        // BCM2836 local controller + BCM2835 legacy bank: the RPi3 has no GIC.
        bcm2836_irq::init();
        bcm2835_legacy_irq::init();
    }

    /// Never called: [`HAS_IPI`](Self::HAS_IPI) is `false`.
    #[inline(always)]
    fn send_ipi(_target_cpu: u32) {}

    #[inline(always)]
    fn unmask_usb_irq() {
        // One-shot unmask: re-enable USB legacy IRQ 9 after driver has cleared GINTSTS
        bcm2835_legacy_irq::enable_irq(bcm2835_legacy_irq::USB_IRQ);
    }

    #[inline(always)]
    fn handle_irq() {
        trap::handle_irq();
    }
}

impl ArmTimer for Bcm2837 {
    #[inline(always)]
    fn init() {
        timer::init();
    }

    #[inline(always)]
    fn reset() {
        timer::reset();
    }
}

impl ArmResources for Bcm2837 {
    const SOC: ArmSoc = ArmSoc::Bcm2837;

    /// Peripherals at 0x3F000000, no GIC, no VirtIO, no Goldfish/PL031 RTC
    /// (uptime only via the ARM counter, epoch = 0). The console register is
    /// the mini UART IO register at 0x3F215040 (AUX_MU_IO).
    const PLATFORM: PlatformFacts = PlatformFacts {
        uart_base: BCM2837.mmio.mini_uart_io,
        uart_irq: 0,
        rtc_base: None,
        published_virtio_slots: 0,
        announce: Some(PlatformAnnounce {
            label: "RPi 3 BCM2837",
            peripheral_base: BCM2837.mmio.peripheral_base,
        }),
    };

    const KERNEL_MMIO_MAPS: &'static [MmioMap] = &[
        // BCM2837 peripheral space 0x3F000000–0x3FFFFFFF (16 MiB): cell MMIO.
        // Covers mini UART, GPIO, I2C, SPI, EMMC — kernel maps them all; individual
        // cells receive sub-regions via sys_request_mmio / resource_registry.
        MmioMap {
            base: BCM2837.mmio.peripheral_base,
            end: match BCM2837.mmio.peripheral_end() {
                Some(end) => end,
                None => panic!("BCM2837 peripheral span must not overflow"),
            },
            cell_access: true,
        },
        // BCM2836 local interrupt controller 0x40000000–0x40001000 (kernel-only).
        MmioMap {
            base: BCM2837.mmio.local_controller_base,
            end: match BCM2837.mmio.local_controller_end() {
                Some(end) => end,
                None => panic!("BCM2837 local-controller span must not overflow"),
            },
            cell_access: false,
        },
    ];

    const VIRTIO_MMIO: Option<hal_soc_arm_virt::VirtioMmioLayout> = None;
    const GPIO_MMIO_BASE: usize = BCM2837.mmio.gpio_base;
    const DWC2_WINDOW: Option<MmioWindow> = Some(MmioWindow {
        base: BCM2837.mmio.dwc2_base,
        size: BCM2837.mmio.dwc2_grant_size,
    });
    /// VideoCore mailbox; framebuffers live below the peripheral aperture.
    const DISPLAY: Option<DisplayFirmware> = Some(DisplayFirmware {
        mailbox: MmioWindow {
            base: BCM2837.mmio.mailbox_base,
            size: BCM2837.mmio.mailbox_grant_size,
        },
        framebuffer_limit: BCM2837.mmio.peripheral_base,
    });
    const SDHCI: Option<SdhciFacts> = Some(SdhciFacts {
        base: BCM2837.mmio.sdhci_base,
        word_access_only: BCM2837.sdhci.word_access_only,
        minimum_write_spacing_us: BCM2837.sdhci.minimum_write_spacing_us,
        pinmux_gpio_base: Some(BCM2837.mmio.gpio_base),
    });
    const BOARD_PINMUX_GPIO_BASE: Option<usize> = Some(BCM2837.mmio.gpio_base);
}
