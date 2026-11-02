//! BCM2711 (Raspberry Pi 4) backend: PL011 console at the profile's UART0.

use super::{ArmPlatform, NoIommu};
use crate::aarch64::uart_pl011;

/// Raspberry Pi 4 (BCM2711).
pub struct Bcm2711;

impl ArmPlatform for Bcm2711 {
    type Iommu = NoIommu;

    const PL011_BASE: usize = hal_soc_bcm27xx::BCM2711.mmio.uart_base;
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
    fn diag_probe(_byte: u8) {}
}
