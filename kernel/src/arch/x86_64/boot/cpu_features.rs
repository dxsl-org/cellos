// SPDX-License-Identifier: MPL-2.0
//! x86_64 CPU features: the hardware-virtualization vendor (CPUID) and root
//! operation. The arch-neutral flags and accessors are `crate::cpu_features`.

use core::sync::atomic::{AtomicU8, Ordering};

/// x86 hardware-virtualization vendor detected via CPUID (Tier 3 x86 VMM).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum X86Virt {
    /// AMD SVM (`CPUID.8000_0001:ECX[2]`) — first backend (QEMU TCG emulates it).
    Svm,
    /// Intel VT-x (`CPUID.1:ECX[5]`) — second backend (KVM/real-HW lane only).
    Vmx,
}

/// 0 = none, 1 = SVM, 2 = VMX — written once by `detect()`.
static X86_VIRT_KIND: AtomicU8 = AtomicU8::new(0);

/// The x86_64 part of `crate::cpu_features::detect`.
pub(crate) fn detect(dtb: usize) {
    let _ = dtb;
    // Latch the x86 hardware-virt vendor (SVM preferred — the TCG-testable
    // backend; VMX only when SVM is absent, i.e. genuine Intel).
    {
        if hal::svm::supported() {
            X86_VIRT_KIND.store(1, Ordering::Relaxed);
        } else if hal::vmx::supported() {
            X86_VIRT_KIND.store(2, Ordering::Relaxed);
        }
    }
}

/// The verdict of a board EL2 monitor: aarch64 only.
pub(crate) fn el2_monitor_verified() -> Option<bool> {
    None
}

/// Is this kernel running under a hypervisor? (`CPUID.1:ECX[31]`)
///
/// Nested SVM/VMX is not guaranteed even when the vendor bits are advertised:
/// `VMXON` under a hypervisor that does not grant nested operation **faults**
/// (`#GP`) rather than reporting failure, and root operation is an optional
/// capability. The boot path consults this before attempting it.
pub(crate) fn hypervisor_present() -> bool {
    core::arch::x86_64::__cpuid(1).ecx & (1 << 31) != 0
}

/// The x86 virt vendor CPUID advertised, if any.  `None` before `detect()`.
pub(crate) fn x86_virt_kind() -> Option<X86Virt> {
    match X86_VIRT_KIND.load(Ordering::Relaxed) {
        1 => Some(X86Virt::Svm),
        2 => Some(X86Virt::Vmx),
        _ => None,
    }
}

/// Enter the CPUID-selected backend's x86 root operation on the CURRENT CPU.
///
/// Root operation is per-CPU state (`EFER.SVME` + `VM_HSAVE_PA`, or
/// `CR4.VMXE` + `VMXON`), so every CPU gets its own freshly allocated HSAVE /
/// VMXON frame. The frame is never freed: root operation lasts until reset, and
/// it is never mapped into any guest NPT (P02 invariant). The BSP calls this
/// before [`latch_x86_root_active`]; while
/// [`has_x86_virt`](crate::cpu_features::has_x86_virt) is set, each AP calls it
/// before going online, and an AP that is refused stays offline.
///
/// `Err` names the refusal for the caller's log line.
pub(crate) fn enter_x86_root_on_current_cpu() -> Result<X86Virt, &'static str> {
    let kind = x86_virt_kind().ok_or("not supported by CPU")?;
    let frame = {
        let mut guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
        guard
            .as_mut()
            .expect("Frame allocator not initialized")
            .allocate_frame()
    };
    match kind {
        X86Virt::Svm => {
            let pa = frame.ok_or("OOM allocating HSAVE frame")?;
            // SAFETY: the HSAVE frame is freshly allocated, exclusively owned
            // by this CPU and never freed (see above).
            unsafe { hal::svm::enable(pa as u64) }
                .map_err(|_| "SVM present but firmware-locked")?;
        }
        X86Virt::Vmx => {
            let pa = frame.ok_or("OOM allocating VMXON frame")?;
            let va = crate::memory::frame::phys_to_virt(pa) as *mut u32;
            // SAFETY: pa/va name the same freshly allocated, exclusively owned
            // frame (HHDM mapping), never freed (see above).
            unsafe { hal::vmx::enter_root(pa as u64, va) }
                .map_err(|_| "VMX present but unavailable")?;
        }
    }
    Ok(kind)
}

/// Record that the BSP successfully entered x86 root operation.
///
/// Called by kmain after [`enter_x86_root_on_current_cpu`] returned `Ok` —
/// the HypervisorCap gate reads
/// [`has_x86_virt`](crate::cpu_features::has_x86_virt). APs that cannot enter
/// the same root operation never go online, so the flag holds on every online
/// CPU.
pub(crate) fn latch_x86_root_active() {
    crate::cpu_features::HAS_X86_VIRT.store(true, Ordering::Relaxed);
}
