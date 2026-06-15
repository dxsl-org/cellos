//! Emulated ARM PL011 UART for the guest console.
//!
//! The guest PL011 lives at IPA 0x09000000 (GPA in Stage-2 unmapped = traps here).
//! Only the registers Linux actually touches during early console output are emulated;
//! all others return a safe default.

use ostd::io::print;

/// PL011 base IPA in the guest address space.
pub const PL011_BASE_IPA: u64 = 0x0900_0000;
pub const PL011_SIZE: u64     = 0x1000; // 4 KiB MMIO window

/// PL011 register offsets (byte addresses from base).
mod reg {
    pub const UARTDR:   u64 = 0x00; // Data register: TX byte on write
    pub const UARTRSR:  u64 = 0x04; // Receive status / error clear
    pub const UARTFR:   u64 = 0x18; // Flag register: read → TX empty/ready bits
    pub const UARTIBRD: u64 = 0x24; // Integer baud rate
    pub const UARTFBRD: u64 = 0x28; // Fractional baud rate
    pub const UARTLCR:  u64 = 0x2C; // Line control (8N1)
    pub const UARTCR:   u64 = 0x30; // Control: UARTEN|TXE|RXE
    pub const UARTIFLS: u64 = 0x34; // FIFO level select
    pub const UARTIMSC: u64 = 0x38; // Interrupt mask
    pub const UARTRIS:  u64 = 0x3C; // Raw interrupt status
    pub const UARTMIS:  u64 = 0x40; // Masked interrupt status
    pub const UARTICR:  u64 = 0x44; // Interrupt clear
    pub const UARTDMACR: u64 = 0x48; // DMA control
}

/// UARTFR bits.
const FR_TXFF: u64 = 1 << 5; // TX FIFO full
const FR_RXFE: u64 = 1 << 4; // RX FIFO empty
const FR_TXFE: u64 = 1 << 7; // TX FIFO empty (all data shifted out)
const FR_BUSY: u64 = 1 << 3; // UART busy

/// Minimal PL011 state.
pub struct Pl011 {
    cr: u64,    // control register shadow
    lcr: u64,   // line control register shadow
    imsc: u64,  // interrupt mask shadow
}

impl Pl011 {
    pub const fn new() -> Self {
        Self { cr: 0x300, lcr: 0, imsc: 0 }
    }

    /// Handle a guest MMIO write to `offset` (relative to PL011_BASE_IPA) with `val`.
    pub fn write(&mut self, offset: u64, val: u64) {
        match offset {
            reg::UARTDR => {
                // Forward TX byte to ViCell serial output.
                let byte = (val & 0xFF) as u8;
                let buf = [byte];
                if let Ok(s) = core::str::from_utf8(&buf) { print(s); }
            }
            reg::UARTCR   => { self.cr   = val; }
            reg::UARTLCR  => { self.lcr  = val; }
            reg::UARTIMSC => { self.imsc = val; }
            reg::UARTICR  => { /* clear interrupts — no interrupt delivery yet */ }
            _ => { /* ignore: IBRD, FBRD, IFLS, DMACR etc. */ }
        }
    }

    /// Handle a guest MMIO read from `offset`; returns the register value.
    pub fn read(&self, offset: u64) -> u64 {
        match offset {
            reg::UARTFR => {
                // TX always ready (we forward bytes synchronously), RX FIFO empty.
                FR_TXFE | FR_RXFE
            }
            reg::UARTCR   => self.cr,
            reg::UARTLCR  => self.lcr,
            reg::UARTIMSC => self.imsc,
            reg::UARTRIS  => 0, // no raw interrupts
            reg::UARTMIS  => 0, // no masked interrupts
            _ => 0,
        }
    }

    /// True if `ipa` falls within this device's MMIO window.
    pub fn owns(ipa: u64) -> bool {
        ipa >= PL011_BASE_IPA && ipa < PL011_BASE_IPA + PL011_SIZE
    }
}
