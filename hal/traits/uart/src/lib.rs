#![no_std]

use types::HalResult;

/// A simple Serial/UART interface
pub trait SerialPort {
    /// Initialize the serial port (baud rate, etc.)
    fn init(&mut self) -> HalResult<()>;

    /// Write a single byte
    fn send(&mut self, data: u8) -> HalResult<()>;

    /// Read a single byte (blocking or polling)
    fn receive(&mut self) -> HalResult<u8>;
}

/// Helper to write strings
pub trait SerialWrite: SerialPort {
    fn write_str(&mut self, s: &str) -> HalResult<()> {
        for byte in s.bytes() {
            self.send(byte)?;
        }
        Ok(())
    }
}

impl<T: SerialPort> SerialWrite for T {}
