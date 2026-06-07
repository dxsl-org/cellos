#![no_std]

/// I2C bus error kinds.
#[derive(Debug, PartialEq)]
pub enum I2cError {
    /// Slave did not acknowledge the address byte.
    NackAddress,
    /// Slave did not acknowledge a data byte.
    NackData,
    /// GPIO or bus-state error (SDA/SCL stuck, driver fault).
    BusError,
}

/// Synchronous I2C master trait.
///
/// # Contract
/// - `addr` is the 7-bit device address (bits 6:0); the R/W bit is managed internally.
/// - Implementations must generate a STOP condition on error to release the bus.
/// - All methods are synchronous (no async boundary) so `&[u8]` / `&mut [u8]` are valid.
pub trait ViI2c {
    type Error: core::fmt::Debug;

    /// Write `bytes` to the device at `addr`.
    fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), Self::Error>;

    /// Read into `buf` from the device at `addr`.
    fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), Self::Error>;

    /// Write `wr`, then read `rd` with a repeated START (combined transaction).
    fn write_read(&mut self, addr: u8, wr: &[u8], rd: &mut [u8]) -> Result<(), Self::Error>;
}
