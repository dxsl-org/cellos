#![no_std]
#![forbid(unsafe_code)]

//! Bit-bang I2C master over two GPIO pins.
//!
//! Generic over any `G: ViGpio`. Concrete use:
//! `BitBangI2c::<Pl061Gpio>::new(gpio)` — calls `Pl061Gpio::open()` in the caller.
//!
//! SDA open-drain emulation with `ViGpio`:
//! - "Drive low" → `set_direction(SDA, Output) + write_pin(SDA, false)`
//! - "Release / float high" → `set_direction(SDA, Input)` (pull-up → reads 1 in QEMU)
//!
//! QEMU note: without a real I2C slave, SDA stays 1 — every address NACKs.
//! The caller should detect `NackAddress` and fall back to synthetic data.

use hal_gpio::{PinDir, ViGpio};
use hal_i2c::{I2cError, ViI2c};

const SCL: u8 = 0; // clock pin
const SDA: u8 = 1; // data pin

// Busy-wait loops per I2C half-period. QEMU TCG timing is not precise;
// this value is chosen for protocol correctness, not exact frequency.
const HALF_PERIOD: usize = 50;

/// Bit-bang I2C master backed by a `ViGpio` implementation.
pub struct BitBangI2c<G: ViGpio> {
    gpio: G,
}

impl<G: ViGpio> BitBangI2c<G> {
    /// Take ownership of `gpio` and prepare it as an I2C master.
    ///
    /// The caller is responsible for opening/acquiring `gpio` before passing it here.
    pub fn new(gpio: G) -> Self {
        Self { gpio }
    }

    /// Release the underlying GPIO resource (e.g. back to the cell owner).
    pub fn into_gpio(self) -> G {
        self.gpio
    }

    fn delay(&self) {
        for _ in 0..HALF_PERIOD {
            core::hint::spin_loop();
        }
    }

    // ── SDA helpers (open-drain emulation) ──────────────────────────────────

    fn sda_low(&mut self) -> Result<(), I2cError> {
        self.gpio
            .set_direction(SDA, PinDir::Output)
            .map_err(|_| I2cError::BusError)?;
        self.gpio
            .write_pin(SDA, false)
            .map_err(|_| I2cError::BusError)
    }

    fn sda_release(&mut self) -> Result<(), I2cError> {
        // Input mode → SDA floats high via pull-up; slave can pull low for ACK.
        self.gpio
            .set_direction(SDA, PinDir::Input)
            .map_err(|_| I2cError::BusError)
    }

    fn read_sda(&self) -> Result<bool, I2cError> {
        self.gpio.read_pin(SDA).map_err(|_| I2cError::BusError)
    }

    // ── SCL helpers (master always drives clock) ─────────────────────────────

    fn scl_high(&mut self) -> Result<(), I2cError> {
        self.gpio
            .write_pin(SCL, true)
            .map_err(|_| I2cError::BusError)
    }

    fn scl_low(&mut self) -> Result<(), I2cError> {
        self.gpio
            .write_pin(SCL, false)
            .map_err(|_| I2cError::BusError)
    }

    // ── Bus conditions ───────────────────────────────────────────────────────

    fn start(&mut self) -> Result<(), I2cError> {
        // START: SDA 1→0 while SCL is high.
        self.sda_release()?;
        self.scl_high()?;
        self.delay();
        self.sda_low()?;
        self.delay();
        self.scl_low()?;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), I2cError> {
        // STOP: SDA 0→1 while SCL is high.
        self.sda_low()?;
        self.delay();
        self.scl_high()?;
        self.delay();
        self.sda_release()?;
        self.delay();
        Ok(())
    }

    // ── Bit-level I/O ────────────────────────────────────────────────────────

    fn send_bit(&mut self, bit: bool) -> Result<(), I2cError> {
        if bit {
            self.sda_release()?;
        } else {
            self.sda_low()?;
        }
        self.delay();
        self.scl_high()?;
        self.delay();
        self.scl_low()?;
        Ok(())
    }

    fn recv_bit(&mut self) -> Result<bool, I2cError> {
        self.sda_release()?; // let slave drive SDA
        self.delay();
        self.scl_high()?;
        let bit = self.read_sda()?;
        self.delay();
        self.scl_low()?;
        Ok(bit)
    }

    // ── Byte-level I/O ───────────────────────────────────────────────────────

    /// Send one byte MSB-first. Returns `true` if slave ACKed (SDA low in ACK slot).
    fn send_byte(&mut self, byte: u8) -> Result<bool, I2cError> {
        for i in (0..8).rev() {
            self.send_bit((byte >> i) & 1 != 0)?;
        }
        // ACK slot: master releases SDA, slave pulls low to ACK.
        let nack = self.recv_bit()?;
        Ok(!nack)
    }

    /// Receive one byte MSB-first. `ack`: true sends ACK (more bytes coming), false sends NACK.
    fn recv_byte(&mut self, ack: bool) -> Result<u8, I2cError> {
        let mut byte = 0u8;
        for _ in 0..8 {
            byte = (byte << 1) | (self.recv_bit()? as u8);
        }
        // ACK/NACK: master drives SDA.
        if ack {
            self.sda_low()?;
        } else {
            self.sda_release()?;
        }
        self.delay();
        self.scl_high()?;
        self.delay();
        self.scl_low()?;
        Ok(byte)
    }

    fn send_addr(&mut self, addr: u8, read: bool) -> Result<bool, I2cError> {
        self.send_byte((addr << 1) | read as u8)
    }
}

impl<G: ViGpio> ViI2c for BitBangI2c<G> {
    type Error = I2cError;

    fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), I2cError> {
        // SCL must be Output throughout; set once before START.
        self.gpio
            .set_direction(SCL, PinDir::Output)
            .map_err(|_| I2cError::BusError)?;
        self.start()?;
        if !self.send_addr(addr, false)? {
            let _ = self.stop();
            return Err(I2cError::NackAddress);
        }
        for &b in bytes {
            if !self.send_byte(b)? {
                let _ = self.stop();
                return Err(I2cError::NackData);
            }
        }
        self.stop()
    }

    fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), I2cError> {
        self.gpio
            .set_direction(SCL, PinDir::Output)
            .map_err(|_| I2cError::BusError)?;
        self.start()?;
        if !self.send_addr(addr, true)? {
            let _ = self.stop();
            return Err(I2cError::NackAddress);
        }
        let last = buf.len().saturating_sub(1);
        for (i, slot) in buf.iter_mut().enumerate() {
            *slot = self.recv_byte(i < last)?; // ACK all but final byte
        }
        self.stop()
    }

    fn write_read(&mut self, addr: u8, wr: &[u8], rd: &mut [u8]) -> Result<(), I2cError> {
        self.gpio
            .set_direction(SCL, PinDir::Output)
            .map_err(|_| I2cError::BusError)?;
        // Write phase
        self.start()?;
        if !self.send_addr(addr, false)? {
            let _ = self.stop();
            return Err(I2cError::NackAddress);
        }
        for &b in wr {
            if !self.send_byte(b)? {
                let _ = self.stop();
                return Err(I2cError::NackData);
            }
        }
        // Repeated START → read phase
        self.start()?;
        if !self.send_addr(addr, true)? {
            let _ = self.stop();
            return Err(I2cError::NackAddress);
        }
        let last = rd.len().saturating_sub(1);
        for (i, slot) in rd.iter_mut().enumerate() {
            *slot = self.recv_byte(i < last)?;
        }
        self.stop()
    }
}
