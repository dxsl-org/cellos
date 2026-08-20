#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

#[cfg(any(feature = "runtime", test))]
mod controller;
#[cfg(any(feature = "runtime", test))]
mod registers;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime")]
use controller::{BcmBscCore, MmioBus};
#[cfg(feature = "runtime")]
use hal_i2c::{I2cError, ViI2c};
#[cfg(feature = "runtime")]
use hal_soc_bcm27xx::BCM2837;
#[cfg(feature = "runtime")]
use ostd::mmio::request_region;
#[cfg(feature = "runtime")]
use types::ViError;

#[cfg(feature = "runtime")]
pub struct BcmBscI2c {
    core: BcmBscCore<MmioBus>,
}

#[cfg(feature = "runtime")]
impl BcmBscI2c {
    pub fn open() -> Result<Self, ViError> {
        let mmio = request_region(BCM2837.mmio.bsc1_base, BCM2837.mmio.bsc1_grant_size)?;
        Ok(Self {
            core: BcmBscCore::new(MmioBus::new(mmio)),
        })
    }
}

#[cfg(feature = "runtime")]
impl ViI2c for BcmBscI2c {
    type Error = I2cError;

    fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        self.core.write(addr, bytes)
    }

    fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), Self::Error> {
        self.core.read(addr, buf)
    }

    fn write_read(&mut self, addr: u8, wr: &[u8], rd: &mut [u8]) -> Result<(), Self::Error> {
        self.core.write_read(addr, wr, rd)
    }
}
