// SPDX-License-Identifier: MPL-2.0
//! `sdhci`: the SoC's SDHCI controller (VisionFive 2), for `crate::dev::mmc`.

#[cfg(feature = "board-vf2")]
use crate::dev::mmc::sdhci::SdhciAccessPolicy;
use crate::dev::mmc::SdhciConfig;

/// The SoC's SDHCI controller, if the board has one.
pub(crate) fn selected_config() -> Option<SdhciConfig> {
    #[cfg(feature = "board-vf2")]
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

/// No SD pinmux to apply.
pub(crate) fn apply_pinmux() {}
