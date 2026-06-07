//! CPU feature detection — parsed from the firmware-provided device tree.
//!
//! Call `detect(dtb)` once at kernel boot before any Cell is spawned.
//! All other callers use the read-only `has_*()` accessors.

use core::sync::atomic::{AtomicBool, Ordering};

static HAS_H_EXT: AtomicBool = AtomicBool::new(false);

/// Probe the device tree for CPU feature flags.
///
/// Must be called once at kernel boot before any Cell is spawned.
/// No-op (and safe) on non-riscv64 targets.
pub(crate) fn detect(dtb: usize) {
    #[cfg(target_arch = "riscv64")]
    detect_riscv(dtb);
    #[cfg(not(target_arch = "riscv64"))]
    let _ = dtb;
}

/// Returns `true` if the RISC-V H-extension (hypervisor) is present.
///
/// Always `false` on non-riscv64 targets.
pub(crate) fn has_h_ext() -> bool {
    HAS_H_EXT.load(Ordering::Relaxed)
}

#[cfg(target_arch = "riscv64")]
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
            HAS_H_EXT.store(true, Ordering::Relaxed);
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
#[cfg(target_arch = "riscv64")]
fn isa_string_has_h(isa: &str) -> bool {
    let after_prefix = if isa.len() >= 4 { &isa[4..] } else { return false };
    for c in after_prefix.chars() {
        match c {
            'h' => return true,
            '_' => return false,             // multi-char zone; no more single-letter exts
            'a'..='z' | '0'..='9' => {}     // extension letter or version component
            _ => return false,
        }
    }
    false
}
