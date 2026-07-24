//! Emulated 16550 UART (COM1 @ 0x3F8) for the x86 guest console.
//!
//! The guest talks to the serial port with `in`/`out` on ports 0x3F8–0x3FF,
//! which trap out as SVM IOIO exits. TX bytes are forwarded to the ViCell
//! console synchronously, so the line is always reported tx-ready
//! (`LSR.THRE|TEMT`). RX: the run loop drains host keystrokes (kernel UART
//! ring via `sys_read(0)`) into [`Uart16550::push_rx`]; `LSR.DR` + the IIR
//! RDA source turn them into a guest IRQ4 (see [`Uart16550::irq_pending`] —
//! delivery/EOI is the run loop + PIC model's job, this type only reports
//! whether an interrupt condition exists).

extern crate alloc;

use alloc::collections::VecDeque;
use ostd::io::print;

/// COM1 base port; the 8-register window is `[BASE, BASE+8)`.
pub const UART_BASE: u16 = 0x3F8;
const UART_TOP: u16 = 0x3F8 + 8;

// Register offsets from BASE (DLAB in LCR bit 7 re-tasks offsets 0/1).
const THR_RBR_DLL: u16 = 0; // write THR / read RBR (DLAB=0); divisor low (DLAB=1)
const IER_DLM: u16 = 1; //     IER (DLAB=0); divisor high (DLAB=1)
const IIR_FCR: u16 = 2; //     read IIR / write FCR
const LCR: u16 = 3;
const MCR: u16 = 4;
const LSR: u16 = 5;
const MSR: u16 = 6;
const SCR: u16 = 7;

const LCR_DLAB: u8 = 1 << 7;
const LSR_DR: u8 = 1 << 0; //   receive data ready
const LSR_THRE: u8 = 1 << 5; // transmit holding register empty
const LSR_TEMT: u8 = 1 << 6; // transmitter empty
const IER_RDA: u8 = 1 << 0; //  enable received-data-available interrupt
const IER_THRE: u8 = 1 << 1; // enable transmitter-empty interrupt
const IIR_NO_INT: u8 = 0x01; // no interrupt pending
const IIR_THRE: u8 = 0x02; //   transmitter-empty interrupt pending
const IIR_RDA: u8 = 0x04; //    received-data-available interrupt pending
const FCR_ENABLE: u8 = 1 << 0;
const IIR_FIFO_ON: u8 = 0xC0; // "FIFOs enabled" marker bits in IIR
const MSR_DEFAULT: u8 = 0xB0; // DCD | DSR | CTS asserted (modem lines up)

/// Bound the RX FIFO so a runaway host input stream cannot grow the cell heap;
/// 16550-style overflow policy: newest bytes are dropped.
const RX_FIFO_CAP: usize = 256;

/// Minimal 16550 register state + RX FIFO.
pub struct Uart16550 {
    ier: u8,
    fcr: u8,
    lcr: u8,
    mcr: u8,
    scr: u8,
    dll: u8,
    dlm: u8,
    rx: VecDeque<u8>,
    /// THR-empty edge latch: set when the (always instantly empty) transmitter
    /// "goes empty" — on THR write or on enabling `IER_THRE` — and cleared by
    /// the IIR read that reports it, per the 16550 contract. Without the edge
    /// latch a level model would interrupt-storm the guest.
    thre_latch: bool,
}

impl Uart16550 {
    pub fn new() -> Self {
        Self {
            ier: 0,
            fcr: 0,
            lcr: 0,
            mcr: 0,
            scr: 0,
            dll: 0,
            dlm: 0,
            rx: VecDeque::new(),
            thre_latch: false,
        }
    }

    /// True if `port` addresses this UART.
    pub fn owns(port: u16) -> bool {
        (UART_BASE..UART_TOP).contains(&port)
    }

    #[inline]
    fn dlab(&self) -> bool {
        self.lcr & LCR_DLAB != 0
    }

    /// Queue one host keystroke for the guest (drops when the FIFO is full).
    pub fn push_rx(&mut self, byte: u8) {
        if self.rx.len() < RX_FIFO_CAP {
            self.rx.push_back(byte);
        }
    }

    /// True when an enabled interrupt source is asserted — the run loop
    /// injects the PIC's IRQ4 vector while this holds (level-triggered; the
    /// guest handler drains RBR / reads IIR, dropping the condition).
    pub fn irq_pending(&self) -> bool {
        (self.ier & IER_RDA != 0 && !self.rx.is_empty())
            || (self.ier & IER_THRE != 0 && self.thre_latch)
    }

    /// Handle a guest `out` to `port` with the low byte of `val`.
    pub fn write(&mut self, port: u16, val: u32) {
        let byte = (val & 0xFF) as u8;
        match port - UART_BASE {
            THR_RBR_DLL => {
                if self.dlab() {
                    self.dll = byte;
                } else {
                    forward(byte);
                    // Synchronous TX: the holding register is empty again the
                    // moment the write completes.
                    self.thre_latch = self.ier & IER_THRE != 0;
                }
            }
            IER_DLM => {
                if self.dlab() {
                    self.dlm = byte;
                } else {
                    // 8250 contract: enabling THRI with an already-empty THR
                    // fires the interrupt immediately (Linux relies on this to
                    // kick the TX path).
                    if byte & IER_THRE != 0 && self.ier & IER_THRE == 0 {
                        self.thre_latch = true;
                    }
                    self.ier = byte;
                }
            }
            IIR_FCR => self.fcr = byte,
            LCR => self.lcr = byte,
            MCR => self.mcr = byte,
            SCR => self.scr = byte,
            _ => {} // LSR/MSR are read-only
        }
    }

    /// Handle a guest `in` from `port`; returns the register value.
    pub fn read(&mut self, port: u16) -> u32 {
        let v: u8 = match port - UART_BASE {
            THR_RBR_DLL => {
                if self.dlab() {
                    self.dll
                } else {
                    self.rx.pop_front().unwrap_or(0)
                }
            }
            IER_DLM => {
                if self.dlab() {
                    self.dlm
                } else {
                    self.ier
                }
            }
            IIR_FCR => {
                let fifo = if self.fcr & FCR_ENABLE != 0 {
                    IIR_FIFO_ON
                } else {
                    0
                };
                // Priority per 16550: RDA above THRE; reading IIR clears a
                // reported THRE condition (and only that).
                if self.ier & IER_RDA != 0 && !self.rx.is_empty() {
                    fifo | IIR_RDA
                } else if self.ier & IER_THRE != 0 && self.thre_latch {
                    self.thre_latch = false;
                    fifo | IIR_THRE
                } else {
                    fifo | IIR_NO_INT
                }
            }
            LCR => self.lcr,
            MCR => self.mcr,
            LSR => {
                let dr = if self.rx.is_empty() { 0 } else { LSR_DR };
                LSR_THRE | LSR_TEMT | dr
            }
            MSR => MSR_DEFAULT,
            SCR => self.scr,
            _ => 0,
        };
        v as u32
    }
}

/// Forward one TX byte to the ViCell console (ASCII only; non-UTF-8 dropped —
/// the guest console stream is text, matching the ARM PL011 model).
fn forward(byte: u8) {
    let buf = [byte];
    if let Ok(s) = core::str::from_utf8(&buf) {
        print(s);
    }
}
