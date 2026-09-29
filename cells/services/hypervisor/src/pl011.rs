//! Emulated ARM PL011 UART for the guest console.
//!
//! The guest PL011 lives at IPA 0x09000000 (GPA in Stage-2 unmapped = traps here).
//! Only the registers Linux actually touches during early console output are emulated;
//! all others return a safe default.

use ostd::io::print;

/// PL011 base IPA in the guest address space.
pub const PL011_BASE_IPA: u64 = 0x0900_0000;
pub const PL011_SIZE: u64 = 0x1000; // 4 KiB MMIO window

/// PL011 register offsets (byte addresses from base).
///
/// Registers below that are never matched in `write`/`read` (RSR, IBRD, FBRD,
/// IFLS, DMACR) document the full PL011 map so the next person extending this
/// emulation to baud/FIFO-level trapping doesn't have to re-derive the offsets.
mod reg {
    pub const UARTDR: u64 = 0x00; // Data register: TX byte on write
    #[allow(dead_code)] // documents full register map; not trapped (RX status unsupported)
    pub const UARTRSR: u64 = 0x04; // Receive status / error clear
    pub const UARTFR: u64 = 0x18; // Flag register: read → TX empty/ready bits
    #[allow(dead_code)] // documents full register map; baud rate not modeled (bytes forwarded synchronously)
    pub const UARTIBRD: u64 = 0x24; // Integer baud rate
    #[allow(dead_code)] // documents full register map; baud rate not modeled (bytes forwarded synchronously)
    pub const UARTFBRD: u64 = 0x28; // Fractional baud rate
    pub const UARTLCR: u64 = 0x2C; // Line control (8N1)
    pub const UARTCR: u64 = 0x30; // Control: UARTEN|TXE|RXE
    #[allow(dead_code)] // documents full register map; FIFO level select not modeled (no interrupt delivery yet)
    pub const UARTIFLS: u64 = 0x34; // FIFO level select
    pub const UARTIMSC: u64 = 0x38; // Interrupt mask
    pub const UARTRIS: u64 = 0x3C; // Raw interrupt status
    pub const UARTMIS: u64 = 0x40; // Masked interrupt status
    pub const UARTICR: u64 = 0x44; // Interrupt clear
    #[allow(dead_code)] // documents full register map; DMA not modeled
    pub const UARTDMACR: u64 = 0x48; // DMA control
}

/// UARTFR bits.
#[allow(dead_code)] // documents the real PL011 flag bit; TX-full backpressure not modeled (synchronous forwarding)
const FR_TXFF: u64 = 1 << 5; // TX FIFO full
const FR_RXFE: u64 = 1 << 4; // RX FIFO empty
const FR_TXFE: u64 = 1 << 7; // TX FIFO empty (all data shifted out)
#[allow(dead_code)] // documents the real PL011 flag bit; busy-state not modeled (synchronous forwarding)
const FR_BUSY: u64 = 1 << 3; // UART busy

// ARM PrimeCell PL011 peripheral/cell ID (QEMU's standard ARM variant).
// Linux's AMBA bus reads 0xfe0..0xffc before binding the ttyAMA driver.
const PL011_ID: [u8; 8] = [0x11, 0x10, 0x14, 0x00, 0x0d, 0xf0, 0x05, 0xb1];
pub const PL011_SPI: u32 = 33; // DTB SPI 1 + GIC's SPI base 32
const INT_RX: u64 = 1 << 4;
const INT_TX: u64 = 1 << 5;
const RX_CAPACITY: usize = 256;

/// Minimal PL011 state with a bounded host-to-guest serial RX queue.
pub struct Pl011 {
    cr: u64,
    lcr: u64,
    imsc: u64,
    tx_irq: bool,
    rx: [u8; RX_CAPACITY],
    rx_head: usize,
    rx_len: usize,
}

impl Pl011 {
    pub const fn new() -> Self {
        Self {
            cr: 0x300,
            lcr: 0,
            imsc: 0,
            tx_irq: false,
            rx: [0; RX_CAPACITY],
            rx_head: 0,
            rx_len: 0,
        }
    }

    pub fn push_rx(&mut self, byte: u8) {
        if self.rx_len < RX_CAPACITY {
            self.rx[(self.rx_head + self.rx_len) % RX_CAPACITY] = byte;
            self.rx_len += 1;
        }
    }

    fn raw_interrupts(&self) -> u64 {
        (if self.rx_len != 0 { INT_RX } else { 0 }) | (if self.tx_irq { INT_TX } else { 0 })
    }

    pub fn irq_pending(&self) -> bool {
        self.raw_interrupts() & self.imsc != 0
    }

    /// Handle a guest MMIO write to `offset` (relative to PL011_BASE_IPA) with `val`.
    pub fn write(&mut self, offset: u64, val: u64) {
        match offset {
            reg::UARTDR => {
                // Forward TX byte to ViCell serial output.
                let byte = (val & 0xFF) as u8;
                let buf = [byte];
                if let Ok(s) = core::str::from_utf8(&buf) {
                    print(s);
                }
                self.tx_irq = true;
            }
            reg::UARTCR => {
                self.cr = val;
            }
            reg::UARTLCR => {
                self.lcr = val;
            }
            reg::UARTIMSC => {
                self.imsc = val;
            }
            reg::UARTICR => self.tx_irq &= val & INT_TX == 0,
            _ => { /* ignore: IBRD, FBRD, IFLS, DMACR etc. */ }
        }
    }

    /// Handle a guest MMIO read from `offset`; returns the register value.
    pub fn read(&mut self, offset: u64) -> u64 {
        if (0xfe0..=0xffc).contains(&offset) && offset & 3 == 0 {
            return PL011_ID[((offset - 0xfe0) / 4) as usize] as u64;
        }
        match offset {
            reg::UARTDR => {
                if self.rx_len == 0 {
                    return 0;
                }
                let value = self.rx[self.rx_head] as u64;
                self.rx_head = (self.rx_head + 1) % RX_CAPACITY;
                self.rx_len -= 1;
                value
            }
            reg::UARTFR => FR_TXFE | if self.rx_len == 0 { FR_RXFE } else { 0 },
            reg::UARTCR => self.cr,
            reg::UARTLCR => self.lcr,
            reg::UARTIMSC => self.imsc,
            reg::UARTRIS => self.raw_interrupts(),
            reg::UARTMIS => self.raw_interrupts() & self.imsc,
            _ => 0,
        }
    }

    /// True if `ipa` falls within this device's MMIO window.
    pub fn owns(ipa: u64) -> bool {
        (PL011_BASE_IPA..PL011_BASE_IPA + PL011_SIZE).contains(&ipa)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primecell_id_identifies_uart_to_amba_bus() {
        let mut uart = Pl011::new();
        for (index, value) in PL011_ID.iter().enumerate() {
            assert_eq!(uart.read(0xfe0 + (index * 4) as u64), u64::from(*value));
        }
    }

    #[test]
    fn rx_irq_tracks_bounded_fifo_through_wrap_and_drain() {
        let mut uart = Pl011::new();
        uart.write(reg::UARTIMSC, INT_RX);
        for byte in 0..RX_CAPACITY {
            uart.push_rx(byte as u8);
        }
        uart.push_rx(0xee); // Full FIFO drops the newest input.
        for byte in 0..RX_CAPACITY / 2 {
            assert_eq!(uart.read(reg::UARTDR), byte as u64);
        }
        for byte in RX_CAPACITY..RX_CAPACITY + RX_CAPACITY / 2 {
            uart.push_rx(byte as u8);
        }
        assert!(uart.irq_pending());
        assert_eq!(uart.read(reg::UARTFR) & FR_RXFE, 0);
        for byte in RX_CAPACITY / 2..RX_CAPACITY + RX_CAPACITY / 2 {
            assert_eq!(uart.read(reg::UARTDR), byte as u8 as u64);
        }
        assert!(!uart.irq_pending());
        assert_ne!(uart.read(reg::UARTFR) & FR_RXFE, 0);
    }
}
