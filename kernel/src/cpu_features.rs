//! CPU feature detection — parsed from the firmware-provided device tree.
//!
//! Call `detect(dtb)` once at kernel boot before any Cell is spawned.
//! All other callers use the read-only `has_*()` accessors. The probes are
//! per architecture (`crate::arch::boot::cpu_features`).

use core::sync::atomic::{AtomicBool, Ordering};

pub(crate) static HAS_H_EXT: AtomicBool = AtomicBool::new(false);

/// QEMU virt EL2-host is latched by detect(); Pi's EL1-host monitor becomes
/// capable only after its HVC init AND boot-time guest smoke succeed.
pub(crate) static HAS_EL2: AtomicBool = AtomicBool::new(false);

/// Latched by kmain AFTER root operation is actually entered (EFER.SVME set /
/// VMXON succeeded).  The HypervisorCap gate keys off THIS, not raw CPUID —
/// firmware may advertise the feature but lock it off.
pub(crate) static HAS_X86_VIRT: AtomicBool = AtomicBool::new(false);

/// Probe the device tree for CPU feature flags.
///
/// Must be called once at kernel boot before any Cell is spawned.
pub(crate) fn detect(dtb: usize) {
    crate::arch::boot::cpu_features::detect(dtb);
}

/// Returns `true` if this kernel entered x86 root operation (SVM/VMX) at boot.
///
/// Always `false` on non-x86_64 targets and on CPUs where the feature is
/// absent or firmware-locked.
pub(crate) fn has_x86_virt() -> bool {
    HAS_X86_VIRT.load(Ordering::Relaxed)
}

/// True only after entering actual EL2 host mode or validating the Pi monitor
/// with an HVC/WFI/MMIO/virtual IRQ guest. Firmware that starts Pi at EL1 fails
/// closed even if the CPU advertises EL2 in its ID registers.
pub(crate) fn has_el2() -> bool {
    if let Some(verified) = crate::arch::boot::cpu_features::el2_monitor_verified() {
        return verified;
    }
    HAS_EL2.load(Ordering::Relaxed)
}

/// Returns `true` if the RISC-V H-extension (hypervisor) is present.
///
/// Always `false` on non-riscv64 targets.
pub(crate) fn has_h_ext() -> bool {
    HAS_H_EXT.load(Ordering::Relaxed)
}
