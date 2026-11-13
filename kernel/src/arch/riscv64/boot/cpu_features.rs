// SPDX-License-Identifier: MPL-2.0
//! riscv64 CPU features: the H-extension, from the firmware device tree. The
//! arch-neutral flags and accessors are `crate::cpu_features`.

use core::sync::atomic::Ordering;

/// The riscv64 part of `crate::cpu_features::detect`.
pub(crate) fn detect(dtb: usize) {
    detect_riscv(dtb);
}

/// The verdict of a board EL2 monitor: aarch64 only.
pub(crate) fn el2_monitor_verified() -> Option<bool> {
    None
}

fn detect_riscv(dtb: usize) {
    if dtb == 0 {
        return;
    }
    // SAFETY: dtb is the FDT pointer handed to the kernel by OpenSBI firmware.
    // fdt::Fdt::from_ptr verifies the FDT magic number before reading any further.
    let fdt = match unsafe { fdt::Fdt::from_ptr(dtb as *const u8) } {
        Ok(f) => f,
        Err(_) => return,
    };
    for cpu in fdt.cpus() {
        // Prefer the newer property; fall back to the legacy ISA string.
        //
        // `riscv,isa-extensions` is a DT stringlist: NUL-separated tokens packed
        // into one byte blob.  `as_str()` strips the trailing NUL; splitting on
        // '\0' produces the individual extension names (e.g. "h", "smstateen").
        let from_ext_list = cpu
            .property("riscv,isa-extensions")
            .and_then(|p| p.as_str())
            .map(|s| s.split('\0').any(|ext| ext == "h"));

        let from_isa_str = cpu
            .property("riscv,isa")
            .and_then(|p| p.as_str())
            .map(isa_string_has_h);

        if from_ext_list.or(from_isa_str).unwrap_or(false) {
            crate::cpu_features::HAS_H_EXT.store(true, Ordering::Relaxed);
            return;
        }
    }
}

/// Returns `true` if the `riscv,isa` string encodes the 'h' extension.
///
/// Scans past the `rv32`/`rv64` prefix, then iterates the single-letter extension
/// zone.  Digits and `p` in version suffixes (e.g. `i2p1`) are skipped — NOT
/// treated as terminators — so `"rv64i2p1mafdch"` correctly detects 'h'.
/// Stops at `_` (start of the multi-char extension zone).
fn isa_string_has_h(isa: &str) -> bool {
    let after_prefix = if isa.len() >= 4 {
        &isa[4..]
    } else {
        return false;
    };
    for c in after_prefix.chars() {
        match c {
            'h' => return true,
            '_' => return false, // multi-char zone; no more single-letter exts
            'a'..='z' | '0'..='9' => {} // extension letter or version component
            _ => return false,
        }
    }
    false
}
