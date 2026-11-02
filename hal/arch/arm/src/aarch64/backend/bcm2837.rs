//! BCM2837 (Raspberry Pi 3) backend: mini UART (AUX UART1) console with
//! interrupt-driven RX and single-byte boot probes.

use super::{ArmPlatform, NoIommu};
use crate::aarch64::uart_bcm_mini;

/// Raspberry Pi 3 (BCM2837).
pub struct Bcm2837;

impl ArmPlatform for Bcm2837 {
    type Iommu = NoIommu;

    /// The console is the mini UART, so no RPi3 path routes output through the
    /// PL011 driver; it keeps the QEMU `virt` address it has always had here.
    const PL011_BASE: usize = hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.base;
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
    fn diag_probe(byte: u8) {
        uart_bcm_mini::probe_put(byte);
    }
}
