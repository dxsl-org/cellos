//! CPU feature detection — parsed from the firmware-provided device tree.
//!
//! Call `detect(dtb)` once at kernel boot before any Cell is spawned.
//! All other callers use the read-only `has_*()` accessors.

#[cfg(target_arch = "x86_64")]
use core::sync::atomic::AtomicU8;
use core::sync::atomic::{AtomicBool, Ordering};

static HAS_H_EXT: AtomicBool = AtomicBool::new(false);

/// Latched at boot by `detect()` if the kernel entered at EL2 (ARM64,
/// QEMU `virtualization=on`).  Always `false` on non-aarch64 targets.
static HAS_EL2: AtomicBool = AtomicBool::new(false);

/// x86 hardware-virtualization vendor detected via CPUID (Tier 3b x86 VMM).
#[cfg(target_arch = "x86_64")]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum X86Virt {
    /// AMD SVM (`CPUID.8000_0001:ECX[2]`) — first backend (QEMU TCG emulates it).
    Svm,
    /// Intel VT-x (`CPUID.1:ECX[5]`) — second backend (KVM/real-HW lane only).
    Vmx,
}

/// 0 = none, 1 = SVM, 2 = VMX — written once by `detect()`.
#[cfg(target_arch = "x86_64")]
static X86_VIRT_KIND: AtomicU8 = AtomicU8::new(0);

/// Latched by kmain AFTER root operation is actually entered (EFER.SVME set /
/// VMXON succeeded).  The HypervisorCap gate keys off THIS, not raw CPUID —
/// firmware may advertise the feature but lock it off.
static HAS_X86_VIRT: AtomicBool = AtomicBool::new(false);

/// Probe the device tree for CPU feature flags.
///
/// Must be called once at kernel boot before any Cell is spawned.
/// No-op (and safe) on non-riscv64 targets.
pub(crate) fn detect(dtb: usize) {
    #[cfg(target_arch = "riscv64")]
    detect_riscv(dtb);
    #[cfg(not(target_arch = "riscv64"))]
    let _ = dtb;
    // Latch EL2 boot status (ARM64 only); no-op on other arches.
    #[cfg(target_arch = "aarch64")]
    if hal::aarch64::el2::is_el2() {
        HAS_EL2.store(true, Ordering::Relaxed);
    }
    // Latch the x86 hardware-virt vendor (SVM preferred — the TCG-testable
    // backend; VMX only when SVM is absent, i.e. genuine Intel).
    #[cfg(target_arch = "x86_64")]
    {
        if hal::svm::supported() {
            X86_VIRT_KIND.store(1, Ordering::Relaxed);
        } else if hal::vmx::supported() {
            X86_VIRT_KIND.store(2, Ordering::Relaxed);
        }
    }
}

/// The x86 virt vendor CPUID advertised, if any.  `None` before `detect()`.
#[cfg(target_arch = "x86_64")]
pub(crate) fn x86_virt_kind() -> Option<X86Virt> {
    match X86_VIRT_KIND.load(Ordering::Relaxed) {
        1 => Some(X86Virt::Svm),
        2 => Some(X86Virt::Vmx),
        _ => None,
    }
}

/// Record that this CPU successfully entered x86 root operation.
///
/// Called by kmain after `hal::svm::enable()` / `hal::vmx::enter_root()`
/// returned `Ok` — the HypervisorCap gate reads [`has_x86_virt`].
#[cfg(target_arch = "x86_64")]
pub(crate) fn latch_x86_root_active() {
    HAS_X86_VIRT.store(true, Ordering::Relaxed);
}

/// Returns `true` if this kernel entered x86 root operation (SVM/VMX) at boot.
///
/// Always `false` on non-x86_64 targets and on CPUs where the feature is
/// absent or firmware-locked.
pub(crate) fn has_x86_virt() -> bool {
    HAS_X86_VIRT.load(Ordering::Relaxed)
}

/// Returns `true` if the kernel booted at EL2 (ARM64, QEMU `virtualization=on`).
///
/// Always `false` on non-aarch64 targets.
pub(crate) fn has_el2() -> bool {
    HAS_EL2.load(Ordering::Relaxed)
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
