pub mod core;
pub mod emmc;
#[cfg(all(
    target_arch = "aarch64",
    any(feature = "board-rpi3", feature = "board-rpi4")
))]
mod pinmux_bcm;
pub mod regs;
pub mod sd;
pub mod sdhci;

use crate::sync::Spinlock;
use api::block::ViBlockDevice;
use emmc::EmmcBlock;
use sd::SdBlock;
use sdhci::SdhciAccessPolicy;
use types::{ViError, ViResult};

#[derive(Clone, Copy)]
struct SdhciConfig {
    base: usize,
    policy: SdhciAccessPolicy,
}

fn selected_sdhci_config() -> Option<SdhciConfig> {
    #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
    {
        let soc = hal_soc_bcm27xx::BCM2837;
        return Some(SdhciConfig {
            base: soc.mmio.sdhci_base,
            policy: SdhciAccessPolicy {
                word_access_only: soc.sdhci.word_access_only,
                minimum_write_spacing_us: soc.sdhci.minimum_write_spacing_us,
            },
        });
    }
    #[cfg(all(
        target_arch = "aarch64",
        feature = "board-rpi4",
        not(feature = "board-rpi3")
    ))]
    {
        let soc = hal_soc_bcm27xx::BCM2711;
        return Some(SdhciConfig {
            base: soc.mmio.sdhci_base,
            policy: SdhciAccessPolicy {
                word_access_only: soc.sdhci.word_access_only,
                minimum_write_spacing_us: soc.sdhci.minimum_write_spacing_us,
            },
        });
    }
    #[cfg(all(target_arch = "riscv64", feature = "board-vf2"))]
    {
        let soc = hal_soc_riscv::JH7110.sdhci.expect("JH7110 SDHCI profile");
        return Some(SdhciConfig {
            base: soc.base,
            policy: SdhciAccessPolicy {
                word_access_only: soc.word_access_only,
                minimum_write_spacing_us: soc.minimum_write_spacing_us,
            },
        });
    }
    #[allow(unreachable_code)]
    None
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

static MMC_DEVICE: Spinlock<Option<MmcDevice>> = Spinlock::new(None);

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

    #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
    pinmux_bcm::apply(
        hal_soc_bcm27xx::BCM2837.mmio.gpio_base,
        crate::board::selected_rpi3().wiring,
    );
    #[cfg(all(
        target_arch = "aarch64",
        feature = "board-rpi4",
        not(feature = "board-rpi3")
    ))]
    pinmux_bcm::apply(
        hal_soc_bcm27xx::BCM2711.mmio.gpio_base,
        crate::board::selected_rpi4().wiring,
    );

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

/// Force-release the `MMC_DEVICE` lock from the fault/panic path.
///
/// # Safety
/// Single-hart, interrupts disabled, called only from the fault/panic handler.
pub unsafe fn force_unlock_locks() {
    MMC_DEVICE.force_unlock();
}
