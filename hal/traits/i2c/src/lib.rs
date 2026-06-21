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

impl embedded_hal::i2c::Error for I2cError {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        match self {
            I2cError::NackAddress => embedded_hal::i2c::ErrorKind::NoAcknowledge(
                embedded_hal::i2c::NoAcknowledgeSource::Address,
            ),
            I2cError::NackData => embedded_hal::i2c::ErrorKind::NoAcknowledge(
                embedded_hal::i2c::NoAcknowledgeSource::Data,
            ),
            I2cError::BusError => embedded_hal::i2c::ErrorKind::Bus,
        }
    }
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

/// Adapter that lets any [`ViI2c`] implementor be used as an [`embedded_hal::i2c::I2c`].
///
/// # Usage
/// ```no_run
/// use hal_i2c::I2cAdapter;
/// let adapter = I2cAdapter(my_vi_i2c_driver);
/// let sensor = some_sensor_crate::Sensor::new(adapter); // expects embedded-hal I2c
/// ```
///
/// The constraint `T::Error: embedded_hal::i2c::Error` is satisfied for any driver
/// using [`I2cError`] as its error type.
pub struct I2cAdapter<T: ViI2c>(pub T);

impl<T: ViI2c> embedded_hal::i2c::ErrorType for I2cAdapter<T>
where
    T::Error: embedded_hal::i2c::Error,
{
    type Error = T::Error;
}

impl<T: ViI2c> embedded_hal::i2c::I2c for I2cAdapter<T>
where
    T::Error: embedded_hal::i2c::Error,
{
    fn read(&mut self, address: u8, read: &mut [u8]) -> Result<(), T::Error> {
        self.0.read(address, read)
    }

    fn write(&mut self, address: u8, write: &[u8]) -> Result<(), T::Error> {
        self.0.write(address, write)
    }

    fn write_read(&mut self, address: u8, write: &[u8], read: &mut [u8]) -> Result<(), T::Error> {
        self.0.write_read(address, write, read)
    }

    /// Executes operations sequentially.
    ///
    /// **Limitation**: each `ViI2c::write` / `read` call issues a full START + STOP.
    /// Consecutive operations therefore produce STOP + START between them, **not** a
    /// repeated START. For the common `[Write reg_addr][Read data]` sensor pattern,
    /// use `I2cAdapter::write_read` directly, which maps to `ViI2c::write_read` and
    /// guarantees the repeated START that most sensors require to latch the register
    /// pointer before the read phase.
    ///
    /// For multi-op patterns that require true repeated START, implement `ViI2c::write_read`
    /// for the specific sequence instead of relying on `transaction`.
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), T::Error> {
        use embedded_hal::i2c::Operation;
        // Fast path: [Write, Read] → write_read() for correct repeated-START semantics.
        if let [Operation::Write(wr), Operation::Read(rd)] = operations {
            return self.0.write_read(address, wr, rd);
        }
        for op in operations.iter_mut() {
            match op {
                Operation::Read(buf) => self.0.read(address, buf)?,
                Operation::Write(bytes) => self.0.write(address, bytes)?,
            }
        }
        Ok(())
    }
}
