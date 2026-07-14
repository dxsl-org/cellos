#![no_std]
#![forbid(unsafe_code)]

//! Bit-bang SPI master over four GPIO pins.
//!
//! Generic over any `G: ViGpio`. Concrete use:
//! `BitBangSpi::<Pl061Gpio>::new(gpio)` — caller opens the GPIO device first.
//!
//! Pin assignments (must not overlap I2C pins 0/1):
//! - MOSI = pin 2 (Master Out Slave In)
//! - MISO = pin 3 (Master In Slave Out)
//! - SCK  = pin 4 (clock)
//! - CS   = pin 5 (chip-select, active-low)
//!
//! **SPI Mode 0 (CPOL=0, CPHA=0):**
//! - Clock idles low; data is captured on the rising edge and shifted out on the falling edge.
//! - Bytes are transferred MSB-first.
//!
//! **QEMU note:** the PL061 GPIO has no MOSI→MISO loopback; MISO floats at 0.
//! The `transfer()` return value will read all-zeros in simulation. Use `write()` to
//! assert the TX path, which works correctly with QEMU MMIO.

use hal_gpio::{PinDir, ViGpio};
use hal_spi::{SpiError, ViSpi};

/// MOSI pin index (Master Out Slave In). Must not overlap SCL=0 / SDA=1.
const MOSI: u8 = 2;
/// MISO pin index (Master In Slave Out).
const MISO: u8 = 3;
/// SCK pin index (clock).
const SCK: u8 = 4;
/// CS pin index (chip-select, active-low).
const CS: u8 = 5;

/// Busy-wait half-period in spin loops. QEMU TCG timing is not cycle-accurate;
/// this value ensures protocol correctness, not exact SPI frequency.
const HALF_PERIOD: usize = 50;

/// Bit-bang SPI master backed by a `ViGpio` implementation.
pub struct BitBangSpi<G: ViGpio> {
    gpio: G,
}

impl<G: ViGpio> BitBangSpi<G> {
    /// Take ownership of `gpio` and prepare it as an SPI master.
    ///
    /// The caller is responsible for opening / acquiring `gpio` before passing it here.
    /// Pin directions are configured lazily on the first `transfer` or `write` call.
    pub fn new(gpio: G) -> Self {
        Self { gpio }
    }

    /// Release the underlying GPIO resource back to the caller.
    pub fn into_gpio(self) -> G {
        self.gpio
    }

    fn delay(&self) {
        for _ in 0..HALF_PERIOD {
            core::hint::spin_loop();
        }
    }

    // ── Pin helpers ──────────────────────────────────────────────────────────

    /// Configure MOSI/SCK/CS as outputs and MISO as input.
    ///
    /// Must be called at the start of every transaction so that open-drain
    /// effects from prior operations don't linger.
    fn setup_pins(&mut self) -> Result<(), SpiError> {
        self.gpio
            .set_direction(MOSI, PinDir::Output)
            .map_err(|_| SpiError::BusError)?;
        self.gpio
            .set_direction(SCK, PinDir::Output)
            .map_err(|_| SpiError::BusError)?;
        self.gpio
            .set_direction(CS, PinDir::Output)
            .map_err(|_| SpiError::BusError)?;
        self.gpio
            .set_direction(MISO, PinDir::Input)
            .map_err(|_| SpiError::BusError)?;
        // Idle state: SCK low, CS deasserted (high).
        self.gpio
            .write_pin(SCK, false)
            .map_err(|_| SpiError::BusError)?;
        self.gpio
            .write_pin(CS, true)
            .map_err(|_| SpiError::BusError)?;
        Ok(())
    }

    fn cs_low(&mut self) -> Result<(), SpiError> {
        self.gpio
            .write_pin(CS, false)
            .map_err(|_| SpiError::BusError)
    }

    fn cs_high(&mut self) -> Result<(), SpiError> {
        self.gpio
            .write_pin(CS, true)
            .map_err(|_| SpiError::BusError)
    }

    fn sck_high(&mut self) -> Result<(), SpiError> {
        self.gpio
            .write_pin(SCK, true)
            .map_err(|_| SpiError::BusError)
    }

    fn sck_low(&mut self) -> Result<(), SpiError> {
        self.gpio
            .write_pin(SCK, false)
            .map_err(|_| SpiError::BusError)
    }

    fn mosi_set(&mut self, high: bool) -> Result<(), SpiError> {
        self.gpio
            .write_pin(MOSI, high)
            .map_err(|_| SpiError::BusError)
    }

    fn read_miso(&self) -> Result<bool, SpiError> {
        self.gpio.read_pin(MISO).map_err(|_| SpiError::BusError)
    }

    // ── Byte transfer ────────────────────────────────────────────────────────

    /// Transfer one byte full-duplex (Mode 0, MSB-first).
    ///
    /// Returns the byte clocked in from MISO.
    /// On QEMU, MISO floats at 0, so the return will always be `0x00`.
    fn xfer_byte(&mut self, out: u8) -> Result<u8, SpiError> {
        let mut received = 0u8;
        for i in (0..8).rev() {
            // Set MOSI before rising edge (Mode 0: data valid on rising edge).
            self.mosi_set((out >> i) & 1 != 0)?;
            self.delay();
            // Rising edge: slave samples MOSI, master prepares to sample MISO.
            self.sck_high()?;
            self.delay();
            // Sample MISO on rising edge (CPHA=0).
            let bit = self.read_miso()? as u8;
            received = (received << 1) | bit;
            // Falling edge: shift register advances on next cycle.
            self.sck_low()?;
        }
        Ok(received)
    }
}

impl<G: ViGpio> ViSpi for BitBangSpi<G> {
    type Error = SpiError;

    fn cs_select(&mut self) -> Result<(), SpiError> {
        self.cs_low()
    }

    fn cs_deselect(&mut self) -> Result<(), SpiError> {
        self.cs_high()
    }

    fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> Result<(), SpiError> {
        self.setup_pins()?;
        self.cs_low()?;

        let len = tx.len().max(rx.len());
        for i in 0..len {
            let out = if i < tx.len() { tx[i] } else { 0x00 };
            match self.xfer_byte(out) {
                Ok(byte) => {
                    if i < rx.len() {
                        rx[i] = byte;
                    }
                }
                Err(e) => {
                    // Best-effort CS deassert on error; ignore secondary fault.
                    let _ = self.cs_high();
                    return Err(e);
                }
            }
        }

        self.cs_high()?;
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), SpiError> {
        self.setup_pins()?;
        self.cs_low()?;

        for &b in bytes {
            match self.xfer_byte(b) {
                Ok(_) => {}
                Err(e) => {
                    // Best-effort CS deassert on error; ignore secondary fault.
                    let _ = self.cs_high();
                    return Err(e);
                }
            }
        }

        self.cs_high()?;
        Ok(())
    }
}
