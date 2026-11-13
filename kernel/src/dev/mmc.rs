pub mod core;
pub mod emmc;
pub mod regs;
pub mod sd;
pub mod sdhci;

use crate::sync::Spinlock;
use abi::block::ViBlockDevice;
use emmc::EmmcBlock;
use sd::SdBlock;
use sdhci::SdhciAccessPolicy;
use types::{ViError, ViResult};

/// The board's SDHCI controller and its access policy (`crate::arch::sdhci`).
#[derive(Clone, Copy)]
pub(crate) struct SdhciConfig {
    pub(crate) base: usize,
    pub(crate) policy: SdhciAccessPolicy,
}

fn selected_sdhci_config() -> Option<SdhciConfig> {
    crate::arch::sdhci::selected_config()
}

// ---------------------------------------------------------------------------
// Device enum — holds either eMMC or SD (runtime probe selection).
// ---------------------------------------------------------------------------

enum MmcDevice {
    Emmc(EmmcBlock),
    Sd(SdBlock),
}

impl MmcDevice {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        match self {
            Self::Emmc(d) => d.read_sector(sector, buf),
            Self::Sd(d) => d.read_sector(sector, buf),
        }
    }
    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> ViResult<()> {
        match self {
            Self::Emmc(d) => d.write_sector(sector, buf),
            Self::Sd(d) => d.write_sector(sector, buf),
        }
    }
    fn sector_count(&self) -> u64 {
        match self {
            Self::Emmc(d) => d.sector_count(),
            Self::Sd(d) => d.sector_count(),
        }
    }
    fn flush(&mut self) -> ViResult<()> {
        match self {
            Self::Emmc(d) => d.flush(),
            Self::Sd(d) => d.flush(),
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static MMC_DEVICE: Spinlock<Option<MmcDevice>> =
    Spinlock::ranked(None, crate::sync::rank::MMC_DEVICE);

/// Zero-sized struct; implements [`ViBlockDevice`] by locking [`MMC_DEVICE`].
pub struct MmcBlock;

impl ViBlockDevice for MmcBlock {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        MMC_DEVICE
            .lock()
            .as_mut()
            .ok_or(ViError::NotFound)?
            .read_sector(sector, buf)
    }
    fn write_sector(&self, sector: u64, buf: &[u8]) -> ViResult<()> {
        MMC_DEVICE
            .lock()
            .as_mut()
            .ok_or(ViError::NotFound)?
            .write_sector(sector, buf)
    }
    fn sector_count(&self) -> u64 {
        MMC_DEVICE
            .lock()
            .as_ref()
            .map(|d| d.sector_count())
            .unwrap_or(0)
    }
    fn sector_size(&self) -> usize {
        512
    }
    fn flush(&self) -> ViResult<()> {
        MMC_DEVICE.lock().as_mut().ok_or(ViError::NotFound)?.flush()
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Probe the SDHCI controller for an eMMC or SD card.
///
/// No-op when no board feature is selected (QEMU keeps using VirtIO).
pub fn init_driver() {
    let Some(config) = selected_sdhci_config() else {
        log::debug!("[mmc] no board configured — skipping SDHCI probe");
        return;
    };

    crate::arch::sdhci::apply_pinmux();

    // Try eMMC first, then SD card.
    // SAFETY: config.base is the SoC-profile MMIO address for the selected board.
    // The MMIO region must be mapped before calling init_driver().
    log::info!("[mmc-diag] probe=eMMC");
    let emmc = unsafe { EmmcBlock::probe(config.base, config.policy) };
    match emmc {
        Ok(dev) => {
            *MMC_DEVICE.lock() = Some(MmcDevice::Emmc(dev));
            log::info!("[mmc] eMMC probed at 0x{:x}", config.base);
            return;
        }
        Err(e) => log::debug!("[mmc] eMMC probe failed ({:?}), trying SD...", e),
    }

    log::info!("[mmc-diag] probe=SD");
    let sd = unsafe { SdBlock::probe(config.base, config.policy) };
    match sd {
        Ok(dev) => {
            *MMC_DEVICE.lock() = Some(MmcDevice::Sd(dev));
            log::info!("[mmc] SD card probed at 0x{:x}", config.base);
        }
        Err(e) => log::warn!("[mmc] no card found at 0x{:x}: {:?}", config.base, e),
    }
}

/// Returns `true` when an MMC/SD card was successfully probed.
pub fn is_present() -> bool {
    MMC_DEVICE.lock().is_some()
}
