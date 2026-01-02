//! Console abstractions (UART/Serial)
//! 
//! Crucial for debugging and early kernel logging.

use crate::HalResult;

/// A simple byte-oriented output stream (e.g., UART TX).
pub trait Write {
    /// Write a single byte to the console.
    fn write_byte(&mut self, byte: u8);

    /// Write a string slice to the console.
    fn write_str(&mut self, s: &str) {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
    }
}

/// A simple byte-oriented input stream (e.g., UART RX).
pub trait Read {
    /// Read a single byte. Returns error if buffer empty or hardware fail.
    fn read_byte(&mut self) -> HalResult<u8>;
}

/// Combined Console Interface
pub trait Console: Write + Read {}
