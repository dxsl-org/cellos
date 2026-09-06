use super::core::{CardInfo, MmcCore};
use super::regs::*;
use super::sdhci::SdhciAccessPolicy;
use hal_traits_mmc::{CardType, ViMmcHost};
use types::{ViError, ViResult};

fn single_block_transfer_mode(is_read: bool) -> u16 {
    TM_BLKCNT_EN | if is_read { TM_DATA_READ } else { 0 }
}

/// SD card block device (SDHC/SDXC block-addressed, or SDSC byte-addressed).
/// State is owned here; caller must hold a `Spinlock` guard (via `MMC_DEVICE`)
/// before calling `read_sector` / `write_sector`.
pub struct SdBlock {
    pub(super) core: MmcCore,
    pub(super) info: CardInfo,
}

impl SdBlock {
    /// Probe the SDHCI controller at `sdhci_base` for an SD card.
    ///
    /// Returns `Err(NotFound)` if the controller is absent or the card is eMMC.
    ///
    /// # Safety
    /// `sdhci_base` must be a valid kernel-mapped MMIO address for the SDHCI register block.
    pub unsafe fn probe(sdhci_base: usize, policy: SdhciAccessPolicy) -> ViResult<Self> {
        // SAFETY: forwarded from caller contract.
        let mut core = MmcCore::new(sdhci_base, policy);
        let info = core.init_card()?;
        if info.card_type == CardType::Emmc {
            return Err(ViError::NotFound);
        }
        log::info!(
            "[sd] SD card probed: {} sectors (~{} MiB), block_addr={}",
            info.sector_count,
            info.sector_count / 2048,
            info.is_block_addressed
        );
        Ok(Self { core, info })
    }

    /// Compute the CMD17/CMD24 argument.
    /// SDHC/SDXC is block-addressed; SDSC is byte-addressed (sector × 512).
    #[inline]
    fn cmd_arg(&self, sector: u64) -> u32 {
        if self.info.is_block_addressed {
            sector as u32
        } else {
            (sector.saturating_mul(512)) as u32
        }
    }

    pub fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        let arg = self.cmd_arg(sector);
        self.core
            .host
            .setup_data_transfer(0x0200, 1, single_block_transfer_mode(true));
        let cmd = hal_traits_mmc::MmcCmd {
            index: 17,
            arg,
            resp_type: hal_traits_mmc::RespType::R1,
            has_data: true,
        };
        self.core.host.send_cmd(cmd)?;
        match self.core.host.read_block(buf) {
            Ok(()) => Ok(()),
            Err(e) => {
                log::warn!("[sd] block read failed at sector {}, retrying at 12.5 MHz", sector);
                let _ = self.core.host.set_clock_hz(12_500_000);
                self.core
                    .host
                    .setup_data_transfer(0x0200, 1, single_block_transfer_mode(true));
                let retry_cmd = hal_traits_mmc::MmcCmd {
                    index: 17,
                    arg,
                    resp_type: hal_traits_mmc::RespType::R1,
                    has_data: true,
                };
                if self.core.host.send_cmd(retry_cmd).is_ok()
                    && self.core.host.read_block(buf).is_ok()
                {
                    log::info!("[sd] block read recovered at 12.5 MHz");
                    return Ok(());
                }
                Err(e)
            }
        }
    }

    pub fn write_sector(&mut self, sector: u64, buf: &[u8]) -> ViResult<()> {
        let arg = self.cmd_arg(sector);
        self.core
            .host
            .setup_data_transfer(0x0200, 1, single_block_transfer_mode(false));
        let cmd = hal_traits_mmc::MmcCmd {
            index: 24,
            arg,
            resp_type: hal_traits_mmc::RespType::R1,
            has_data: true,
        };
        self.core.host.send_cmd(cmd)?;
        self.core.host.write_block(buf)
    }

    pub fn sector_count(&self) -> u64 {
        self.info.sector_count
    }

    pub fn flush(&mut self) -> ViResult<()> {
        self.core.wait_ready_and_flush(self.info.rca)
    }
}

impl Drop for SdBlock {
    fn drop(&mut self) {
        // SdhciController::drop powers off the card slot on teardown.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_block_read_enables_block_count_and_direction() {
        let mode = single_block_transfer_mode(true);

        assert_eq!(mode, TM_BLKCNT_EN | TM_DATA_READ);
        assert_eq!(mode & (TM_DMA_EN | TM_MULTI_BLK), 0);
    }

    #[test]
    fn single_block_write_enables_block_count_without_read_direction() {
        let mode = single_block_transfer_mode(false);

        assert_eq!(mode, TM_BLKCNT_EN);
        assert_eq!(mode & (TM_DMA_EN | TM_DATA_READ | TM_MULTI_BLK), 0);
    }
}
