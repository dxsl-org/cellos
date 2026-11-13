// SPDX-License-Identifier: MPL-2.0
//! `sdhci`: the board backend's SDHCI controller, for `crate::dev::mmc`.

use crate::dev::mmc::sdhci::SdhciAccessPolicy;
use crate::dev::mmc::SdhciConfig;

/// The board backend's SDHCI controller, if it has one.
pub(crate) fn selected_config() -> Option<SdhciConfig> {
    if let Some(sdhci) =
        <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::SDHCI
    {
        return Some(SdhciConfig {
            base: sdhci.base,
            policy: SdhciAccessPolicy {
                word_access_only: sdhci.word_access_only,
                minimum_write_spacing_us: sdhci.minimum_write_spacing_us,
            },
        });
    }
    None
}

/// Route the SD pins to the controller when the board backend names a pinmux
/// GPIO block.
pub(crate) fn apply_pinmux() {
    if let Some(gpio_base) =
        <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::SDHCI
            .and_then(|sdhci| sdhci.pinmux_gpio_base)
    {
        crate::arch::dev::mmc::pinmux_bcm::apply(gpio_base, crate::board::active().wiring);
    }
}
