// SPDX-License-Identifier: MPL-2.0
//! 32-bit CPU features: none are probed. The arch-neutral flags and accessors
//! are `crate::cpu_features`.

/// The 32-bit part of `crate::cpu_features::detect`: nothing to probe.
pub(crate) fn detect(dtb: usize) {
    let _ = dtb;
}

/// The verdict of a board EL2 monitor: aarch64 only.
pub(crate) fn el2_monitor_verified() -> Option<bool> {
    None
}
