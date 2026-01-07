//! Minimal 16550 UART Driver for QEMU RISC-V Virt
//! 
//! Used for kernel logging and early debug output.
//! Base Address: 0x10000000

use core::fmt;
use crate::sync::Spinlock;

/// UART Registers (offset from base)
const RHR: usize = 0; // Receive Holding Register (read)
const THR: usize = 0; // Transmit Holding Register (write)
const IER: usize = 1; // Interrupt Enable Register
const FCR: usize = 2; // FIFO Control Register
const ISR: usize = 2; // Interrupt Status Register
const LCR: usize = 3; // Line Control Register
const LSR: usize = 5; // Line Status Register

/// Line Status Flags
const LSR_RX_READY: u8 = 1 << 0;
const LSR_TX_EMPTY: u8 = 1 << 5;

#[allow(non_camel_case_types)]
pub struct viUART {
    base_addr: usize,
}

impl viUART {
    /// Create a new viUART instance (unsafe because base_addr must be valid)
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self { base_addr }
    }

    /// Initialize the UART
    pub fn init(&mut self) {
        unsafe {
            let ptr = self.base_addr as *mut u8;
            
            // Disable interrupts
            ptr.add(IER).write_volatile(0x00);
            
            // Enable FIFO
            ptr.add(FCR).write_volatile(0x01);
            
            // Set 8-bit mode (Word Length Select bits 0 and 1)
            ptr.add(LCR).write_volatile(0x03);
            
            // Enable interrupts (Receive Data Available) - Optional for polling
            ptr.add(IER).write_volatile(0x01);
        }
    }

    /// Write a single byte
    pub fn write_byte(&mut self, byte: u8) {
        unsafe {
            let ptr = self.base_addr as *mut u8;
            
            // Wait for TX FIFO to be empty
            while (ptr.add(LSR).read_volatile() & LSR_TX_EMPTY) == 0 {}
            
            // Write byte
            ptr.add(THR).write_volatile(byte);
        }
    }
}

impl fmt::Write for viUART {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

// Global Serial Instance protected by Spinlock
pub static SERIAL: Spinlock<viUART> = Spinlock::new(unsafe { viUART::new(0x10_000_000) });

struct StackWrite<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

impl<'a> StackWrite<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, offset: 0 }
    }
    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.offset]
    }
}

impl<'a> fmt::Write for StackWrite<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if self.offset + len > self.buf.len() {
            return Err(fmt::Error);
        }
        self.buf[self.offset..self.offset + len].copy_from_slice(bytes);
        self.offset += len;
        Ok(())
    }
}

// Logger integration
struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            use fmt::Write;
            
            // 1. Write to Serial
            {
                let mut serial = SERIAL.lock();
                let _ = write!(serial, "[{:>5}] {}\n", record.level(), record.args());
            }

            // 2. Write to Framebuffer Console - DISABLED for stability debugging
            /*
            let mut buf = [0u8; 256];
            let mut wrapper = StackWrite::new(&mut buf);
            // We ignore errors here. If it's too long, it's truncated or partially written.
            let _ = write!(wrapper, "[{}] {}\n", record.level(), record.args());
            if let Ok(s) = core::str::from_utf8(wrapper.as_bytes()) {
                if !s.is_empty() {
                    crate::task::drivers::fb_console::FramebufferConsole::write_str(s);
                }
            }
            */
        }
    }

    fn flush(&self) {}
}

static LOGGER: SimpleLogger = SimpleLogger;

pub fn init() {
    SERIAL.lock().init();
    
    // Initialize Logger
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info));
}
