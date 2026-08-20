#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

#[cfg(any(feature = "runtime", test))]
mod controller;
#[cfg(any(feature = "runtime", test))]
mod registers;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime")]
use controller::{BcmSpiCore, MmioBus};
#[cfg(feature = "runtime")]
use hal_soc_bcm27xx::BCM2837;
#[cfg(feature = "runtime")]
use hal_spi::{SpiError, ViSpi};
#[cfg(feature = "runtime")]
use ostd::mmio::request_region;
#[cfg(feature = "runtime")]
use types::ViError;

/// Polling-only BCM SPI0 master, Mode 0, native CS0 baseline.
#[cfg(feature = "runtime")]
pub struct BcmSpi0 {
    core: BcmSpiCore<MmioBus>,
}

#[cfg(feature = "runtime")]
impl BcmSpi0 {
    pub fn open() -> Result<Self, ViError> {
        let mmio = request_region(BCM2837.mmio.spi0_base, BCM2837.mmio.spi0_grant_size)?;
        Ok(Self {
            core: BcmSpiCore::new(MmioBus::new(mmio)),
        })
    }
}

#[cfg(feature = "runtime")]
impl ViSpi for BcmSpi0 {
    type Error = SpiError;

    fn cs_select(&mut self) -> Result<(), Self::Error> {
        self.core.cs_select()
    }

    fn cs_deselect(&mut self) -> Result<(), Self::Error> {
        self.core.cs_deselect()
    }

    fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> Result<(), Self::Error> {
        self.core.transfer(tx, rx)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.core.write(bytes)
    }
}

#[cfg(feature = "runtime")]
impl Drop for BcmSpi0 {
    fn drop(&mut self) {
        let _ = self.core.cs_deselect();
    }
}
