// SPDX-License-Identifier: MPL-2.0
//! aarch64 CPU features: the EL2 boot regime and the board's EL2 monitor. The
//! arch-neutral flags and accessors are `crate::cpu_features`.

use core::sync::atomic::Ordering;

/// The aarch64 part of `crate::cpu_features::detect`.
pub(crate) fn detect(dtb: usize) {
    let _ = dtb;
    // Latch EL2 boot status (ARM64 only); no-op on other arches.
    if hal::aarch64::el2::is_el2() {
        crate::cpu_features::HAS_EL2.store(true, Ordering::Relaxed);
    }
}

/// The verdict of the board's EL2 monitor (Pi's EL1-host monitor: HVC init
/// and boot-time guest smoke), or `None` when the board has none.
pub(crate) fn el2_monitor_verified() -> Option<bool> {
    use hal::backend::{ArmEl2Monitor, El2Monitor};
    if El2Monitor::PRESENT {
        return Some(El2Monitor::is_verified());
    }
    None
}
