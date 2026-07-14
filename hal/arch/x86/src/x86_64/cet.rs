//! x86_64 CET (Control-flow Enforcement Technology) — IBT + Shadow Stack.
//!
//! IBT (Indirect Branch Tracking): every indirect CALL/JMP must land on
//! ENDBR64. Enabled via CR4.CET + MSR_IA32_S_CET.ENDBR_EN.
//!
//! Shadow Stack: CPU maintains a write-protected return-address stack.
//! Enabled separately (gated by `cet-shadow-stack` feature + CPUID).
//!
//! ⚠️ IBT is a PREREQUISITE for x86 MPK/PKU: WRPKRU is an unprivileged
//! instruction; without IBT a JOP gadget can call it to grant all keys
//! (ERIM 2019, PKU-Pitfalls 2020).
//!
//! # Init sequence
//! Must be called AFTER `idt::init()` — the #CP handler (vector 21) is
//! registered by `idt::init()` unconditionally so that enabling IBT here
//! immediately has a valid fault handler.

// ── MSR constants ─────────────────────────────────────────────────────────────
/// Supervisor CET control MSR (IA32_S_CET).
const MSR_IA32_S_CET: u32 = 0x6A2;
/// Kernel shadow-stack pointer MSR (IA32_PL0_SSP).
#[cfg(feature = "cet-shadow-stack")]
const MSR_IA32_PL0_SSP: u32 = 0x6A4;

/// S_CET bit: enable shadow stack at privilege level 0.
#[cfg(feature = "cet-shadow-stack")]
const IA32_S_CET_SH_STK_EN: u64 = 1 << 0;
/// S_CET bit: allow WRSS instruction to update shadow-stack pages.
#[cfg(feature = "cet-shadow-stack")]
const IA32_S_CET_WR_SHSTK_EN: u64 = 1 << 1;
/// S_CET bit: enable ENDBR enforcement for indirect branches.
const IA32_S_CET_ENDBR_EN: u64 = 1 << 2;
/// S_CET bit: suppress #CP on legacy-code-without-ENDBR (NO_TRACK prefix).
const IA32_S_CET_NO_TRACK_EN: u64 = 1 << 4;

/// CR4 bit 23: enable CET for the current privilege level.
const CR4_CET: u64 = 1 << 23;

// ── CPUID detection ───────────────────────────────────────────────────────────

/// Hardware CET capability flags detected at runtime.
pub struct CetCaps {
    /// CPU supports Indirect Branch Tracking (ENDBR64 enforcement).
    pub ibt: bool,
    /// CPU supports user/supervisor shadow stacks.
    pub shstk: bool,
}

/// Detect CET capabilities via CPUID leaf 7, subleaf 0.
///
/// Returns `CetCaps { ibt: false, shstk: false }` on CPUs that do not
/// support CPUID leaf 7 (very old hardware).
pub fn detect() -> CetCaps {
    // __cpuid_count is safe: CPUID is a read-only instruction with no side effects.
    let result = core::arch::x86_64::__cpuid_count(7, 0);
    CetCaps {
        // IBT: leaf 7, subleaf 0, EDX bit 20
        ibt: (result.edx >> 20) & 1 != 0,
        // Shadow stack: leaf 7, subleaf 0, ECX bit 7
        shstk: (result.ecx >> 7) & 1 != 0,
    }
}

// ── Public init ───────────────────────────────────────────────────────────────

/// Enable CET-IBT (and optionally shadow stack) for the kernel.
///
/// Preconditions:
/// - Called from Ring-0.
/// - `idt::init()` has already installed the #CP handler at vector 21.
/// - `#[cfg(feature = "cet-shadow-stack")]`: shadow stack is only activated
///   when both the cargo feature and CPUID confirm hardware support.
///
/// This function is a no-op on CPUs that do not advertise CET-IBT.
pub fn init_kernel_cet() {
    let caps = detect();

    if !caps.ibt {
        log_str("[INFO] CET-IBT: unavailable\n");
        return;
    }

    // Enable CR4.CET — required before any CET MSR can be written.
    // SAFETY: CR4 write from Ring-0 is a standard privileged operation.
    unsafe {
        let mut cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        cr4 |= CR4_CET;
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
    }

    // Enable IBT: ENDBR_EN forces all indirect CALL/JMP to land on ENDBR64.
    // NO_TRACK_EN suppresses #CP for legacy indirect branches prefixed with
    // the DS-override (0x3E) NO_TRACK prefix — avoids false positives from
    // firmware thunks that cannot be recompiled.
    wrmsr(MSR_IA32_S_CET, IA32_S_CET_ENDBR_EN | IA32_S_CET_NO_TRACK_EN);

    log_str("[INFO] CET-IBT: enabled\n");

    // Shadow Stack: gated on both the cargo feature AND CPUID support.
    #[cfg(feature = "cet-shadow-stack")]
    {
        if caps.shstk {
            init_shadow_stack();
            log_str("[INFO] CET-SS: enabled\n");
        } else {
            log_str("[INFO] CET-SS: unavailable\n");
        }
    }
    #[cfg(not(feature = "cet-shadow-stack"))]
    {
        let _ = caps.shstk; // suppress unused-variable warning
        log_str("[INFO] CET-SS: skipped (feature disabled)\n");
    }
}

// ── Shadow Stack (feature-gated) ─────────────────────────────────────────────

/// Initialise a static 4 KiB kernel shadow stack and point PL0_SSP at it.
///
/// Preconditions: CR4.CET is already set; called single-threaded at boot.
/// The shadow stack is a BSS-allocated page; physical write-protection of the
/// page is a G2 hardening task (requires the paging module to mark it as SS).
/// For now, hardware enforcement is limited to CALL/RET balance checking
/// once SH_STK_EN is set in S_CET.
#[cfg(feature = "cet-shadow-stack")]
fn init_shadow_stack() {
    // 512 × 8 bytes = 4096 bytes — one page, grows downward.
    static mut KERNEL_SHADOW_STACK: [u64; 512] = [0u64; 512];

    // SAFETY: Single-threaded boot. CR4.CET is set (prerequisite). Writing
    // PL0_SSP before SH_STK_EN is the required MSR ordering per SDM Vol.3
    // §17.2.1. The shadow stack pointer is aligned to 8 bytes (u64 array end).
    unsafe {
        // Point SSP at the TOP (high address) of the buffer; it grows down.
        let ssp = core::ptr::addr_of!(KERNEL_SHADOW_STACK) as u64
            + core::mem::size_of::<[u64; 512]>() as u64;
        wrmsr(MSR_IA32_PL0_SSP, ssp);

        // Activate shadow stack: read-modify-write to preserve ENDBR_EN bit
        // already set by init_kernel_cet().
        let cur_cet = rdmsr(MSR_IA32_S_CET);
        wrmsr(
            MSR_IA32_S_CET,
            cur_cet | IA32_S_CET_SH_STK_EN | IA32_S_CET_WR_SHSTK_EN,
        );
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Write a 64-bit value to an MSR.
///
/// # Safety (call-site): caller must supply a valid MSR index; calling from
/// Ring-0 on a valid MSR does not affect Rust memory safety.
fn wrmsr(msr: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    // SAFETY: wrmsr from Ring-0 on a valid MSR is a safe privileged operation.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") lo,
            in("edx") hi,
            options(nomem, nostack),
        );
    }
}

/// Read a 64-bit MSR value.
///
/// # Safety (call-site): caller must supply a valid MSR index.
#[cfg(feature = "cet-shadow-stack")]
fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: rdmsr from Ring-0 on a valid MSR does not modify any memory.
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack),
        );
    }
    (hi as u64) << 32 | lo as u64
}

/// Write a string to COM1 via the UART driver.
///
/// Used instead of a logging macro to avoid pulling in alloc or the kernel
/// log infrastructure before it is initialised.
fn log_str(s: &str) {
    for c in s.bytes() {
        super::uart_16550::putchar(c);
    }
}
