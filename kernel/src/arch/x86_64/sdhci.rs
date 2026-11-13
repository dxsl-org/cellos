// SPDX-License-Identifier: MPL-2.0
//! `sdhci`: no SDHCI controller on this target.

use crate::dev::mmc::SdhciConfig;

/// No SDHCI controller.
pub(crate) fn selected_config() -> Option<SdhciConfig> {
    None
}

/// No SD pinmux to apply.
pub(crate) fn apply_pinmux() {}
