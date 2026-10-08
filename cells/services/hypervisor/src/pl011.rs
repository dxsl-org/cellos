//! Emulated ARM PL011 UART for the guest console.
//!
//! The guest PL011 lives at IPA 0x09000000 (GPA in Stage-2 unmapped = traps here).
//! Early console and the regular ttyAMA driver share the trapped MMIO state.
//! Unsupported registers read as zero; TX is forwarded synchronously.

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
    #[allow(dead_code)] // documents the receive error register (no errors modeled)
    pub const UARTRSR: u64 = 0x04; // Receive status / error clear
    pub const UARTFR: u64 = 0x18; // Flag register: read → TX empty/ready bits
    #[allow(dead_code)] // documents full register map; baud rate not modeled (bytes forwarded synchronously)
    pub const UARTIBRD: u64 = 0x24; // Integer baud rate
    #[allow(dead_code)] // documents full register map; baud rate not modeled (bytes forwarded synchronously)
    pub const UARTFBRD: u64 = 0x28; // Fractional baud rate
    pub const UARTLCR: u64 = 0x2C; // Line control (8N1)
    pub const UARTCR: u64 = 0x30; // Control: UARTEN|TXE|RXE
    #[allow(dead_code)] // documents FIFO thresholds (RX interrupt follows nonempty input)
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
    /// TX-side filter state for `ESC [ 6 n`, the cursor-position *request*.
    tx_state: u8,
    tx_hold: [u8; 3],
    tx_hold_len: u8,
    /// RX-side filter state for `ESC [ … R`, the terminal's reply to it.
    rx_state: u8,
    rx_hold: [u8; 12],
    rx_hold_len: u8,
    /// How many queries were suppressed and how many stale replies dropped.
    queries_suppressed: u32,
    replies_dropped: u32,
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
            tx_state: 0,
            tx_hold: [0; 3],
            tx_hold_len: 0,
            rx_state: 0,
            rx_hold: [0; 12],
            rx_hold_len: 0,
            queries_suppressed: 0,
            replies_dropped: 0,
        }
    }

    pub fn push_rx(&mut self, byte: u8) {
        if self.rx_len < RX_CAPACITY {
            self.rx[(self.rx_head + self.rx_len) % RX_CAPACITY] = byte;
            self.rx_len += 1;
        }
    }

    /// Feed one byte arriving from the **host terminal** into the guest.
    ///
    /// Drops the terminal's reply to a cursor-position query (`ESC [ … R`).
    /// The guest is far too slow to use that reply — every console byte traps
    /// through Stage-2 — so by the time it arrives the shell has stopped waiting
    /// and reads it as command text: the board showed `~ # [71;11Runame -a` and
    /// `/bin/sh: [71: not found` for a typed `uname -a`. Anything else (arrow
    /// keys, function keys, plain text) passes through byte-for-byte, and a
    /// sequence that turns out not to be a reply is flushed rather than eaten.
    pub fn push_host_rx(&mut self, byte: u8) {
        const MAX_PARAMS: u8 = 12;
        let flush = |s: &mut Self, extra: Option<u8>| {
            for i in 0..s.rx_hold_len as usize {
                s.push_rx(s.rx_hold[i]);
            }
            s.rx_hold_len = 0;
            if let Some(b) = extra {
                s.push_rx(b);
            }
            s.rx_state = 0;
        };
        match self.rx_state {
            0 if byte == 0x1b => {
                self.rx_hold[0] = byte;
                self.rx_hold_len = 1;
                self.rx_state = 1;
            }
            0 => self.push_rx(byte),
            1 if byte == b'[' => {
                self.rx_hold[1] = byte;
                self.rx_hold_len = 2;
                self.rx_state = 2;
            }
            1 => flush(self, Some(byte)),
            2 if byte.is_ascii_digit() || byte == b';' || byte == b'?' => {
                self.rx_hold[2] = byte;
                self.rx_hold_len = 3;
                self.rx_state = 3;
            }
            2 => flush(self, Some(byte)),
            3 if byte.is_ascii_digit() || byte == b';' => {
                if self.rx_hold_len >= MAX_PARAMS {
                    flush(self, Some(byte));
                } else {
                    self.rx_hold[self.rx_hold_len as usize] = byte;
                    self.rx_hold_len += 1;
                }
            }
            3 if byte == b'R' => {
                // The reply itself: dropped, and counted so a guest that keeps
                // asking is visible.
                self.rx_hold_len = 0;
                self.rx_state = 0;
                self.replies_dropped = self.replies_dropped.wrapping_add(1);
                if self.replies_dropped % 8 == 1 {
                    ostd::io::print("[hv] dropped ");
                    ostd::io::print_usize(self.replies_dropped as usize);
                    ostd::io::println(
                        " terminal reply(ies): the guest console cannot use them in time",
                    );
                }
            }
            _ => flush(self, Some(byte)),
        }
    }

    /// Feed one guest TX byte through the console filter, appending what should
    /// reach the terminal to `out`. Returns how many bytes that is.
    ///
    /// Suppresses `ESC [ 6 n` — the cursor-position **request** — so the host
    /// terminal never produces the reply the guest cannot use in time. Every
    /// other byte, including other escape sequences, is forwarded unchanged; a
    /// prefix that turns out not to match is flushed.
    pub fn filter_tx(&mut self, byte: u8, out: &mut [u8; 4]) -> usize {
        match self.tx_state {
            0 if byte == 0x1b => {
                self.tx_hold[0] = byte;
                self.tx_hold_len = 1;
                self.tx_state = 1;
                0
            }
            0 => {
                out[0] = byte;
                1
            }
            1 if byte == b'[' => {
                self.tx_hold[1] = byte;
                self.tx_hold_len = 2;
                self.tx_state = 2;
                0
            }
            2 if byte == b'6' => {
                self.tx_hold[2] = byte;
                self.tx_hold_len = 3;
                self.tx_state = 3;
                0
            }
            3 if byte == b'n' => {
                self.tx_hold_len = 0;
                self.tx_state = 0;
                self.queries_suppressed = self.queries_suppressed.wrapping_add(1);
                if self.queries_suppressed % 8 == 1 {
                    ostd::io::print("[hv] suppressed ");
                    ostd::io::print_usize(self.queries_suppressed as usize);
                    ostd::io::println(
                        " cursor-position quer(ies): this console does not answer them",
                    );
                }
                0
            }
            _ => {
                let mut n = 0usize;
                for i in 0..self.tx_hold_len as usize {
                    out[n] = self.tx_hold[i];
                    n += 1;
                }
                out[n] = byte;
                n += 1;
                self.tx_hold_len = 0;
                self.tx_state = 0;
                n
            }
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
                // Forward TX bytes to the serial console, minus the sequences
                // this console cannot answer (see `filter_tx`).
                let mut out = [0u8; 4];
                let n = self.filter_tx((val & 0xFF) as u8, &mut out);
                if let Ok(s) = core::str::from_utf8(&out[..n]) {
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

#[cfg(test)]
mod console_filter_tests {
    use super::Pl011;

    fn tx_bytes(pl011: &mut Pl011, input: &[u8]) -> alloc::vec::Vec<u8> {
        let mut out = alloc::vec::Vec::new();
        for &b in input {
            let mut buf = [0u8; 4];
            let n = pl011.filter_tx(b, &mut buf);
            out.extend_from_slice(&buf[..n]);
        }
        out
    }

    fn rx_bytes(pl011: &mut Pl011, input: &[u8]) -> alloc::vec::Vec<u8> {
        for &b in input {
            pl011.push_host_rx(b);
        }
        let mut out = alloc::vec::Vec::new();
        while let byte = pl011.read(0x00) as u8 {
            // UARTDR reads back 0 when the ring is empty; the filter never feeds
            // a NUL, so this is unambiguous for the test's inputs.
            if byte == 0 {
                break;
            }
            out.push(byte);
        }
        out
    }

    /// The guest asks where the cursor is; this console never answers, so the
    /// request must not reach the terminal that would reply late.
    #[test]
    fn cursor_position_request_never_reaches_the_terminal() {
        let mut pl011 = Pl011::new();
        assert!(tx_bytes(&mut pl011, b"\x1b[6n").is_empty());
        assert_eq!(tx_bytes(&mut pl011, b"ok"), b"ok");
    }

    /// Everything else — including other escape sequences — is forwarded
    /// byte-for-byte, and a prefix that stops matching is flushed, not eaten.
    #[test]
    fn other_sequences_and_text_pass_through_tx() {
        let mut pl011 = Pl011::new();
        assert_eq!(tx_bytes(&mut pl011, b"\x1b[A"), b"\x1b[A");
        assert_eq!(tx_bytes(&mut pl011, b"\x1b[6x"), b"\x1b[6x");
        assert_eq!(tx_bytes(&mut pl011, b"hi"), b"hi");
    }

    /// The terminal's reply to that query arrives after the guest stopped
    /// waiting, so it must not be read as a command.
    #[test]
    fn stale_cursor_position_reply_is_dropped() {
        let mut pl011 = Pl011::new();
        assert_eq!(rx_bytes(&mut pl011, b"\x1b[71;11R"), b"");
        assert_eq!(rx_bytes(&mut pl011, b"a"), b"a");
    }

    /// Arrow keys are escape sequences too, and they are not replies.
    #[test]
    fn arrow_keys_pass_through_rx() {
        let mut pl011 = Pl011::new();
        assert_eq!(rx_bytes(&mut pl011, b"\x1b[A"), b"\x1b[A");
    }
}
