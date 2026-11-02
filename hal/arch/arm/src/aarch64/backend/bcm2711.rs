//! BCM2711 (Raspberry Pi 4) backend: PL011 console at the profile's UART0,
//! GIC-400, generic-timer tick, and PSCI secondaries.

use hal_soc_bcm27xx::BCM2711;

use super::{ArmIrqController, ArmPlatform, ArmTimer, NoIommu, SecondaryStart};
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
