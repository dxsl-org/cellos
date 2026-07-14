//! x86_64 PKU (Protection Keys for Userspace) — domain-based page access control.
//!
//! PKU assigns 4-bit keys to PTE bits [62:59] and uses the PKRU register (32 bits,
//! 2 bits per key) to gate ring-3 access per key domain. PKRU = 0 = all-access
//! (kernel default).
//!
//! ⚠️ PKU is only secure when forward-edge CFI (CET-IBT) is active.
//! Without IBT a JOP gadget can call WRPKRU to grant all keys
//! (ERIM 2019, PKU-Pitfalls 2020). `init()` checks `cet::detect().ibt` and
//! refuses enforcement if IBT is off.
//!
//! Key model:
//!   Key 0 — kernel / trusted-core (PKRU=0, all access)
//!   Key 1 — standard Tier-1 Rust cells
//!   Key 2 — Tier-1b C/FFI cells (mlibc, DOOM)
//!   Key 15 — PKS supervisor key (Intel Ice Lake+ only, behind `pks` feature)
//!
//! # WRPKRU contract
//! WRPKRU reads PKRU from EAX, requires ECX=0 and EDX=0. It does NOT clobber
//! any register but the caller must zero ECX before calling, which destroys
//! the prior value in RCX. See `syscall.rs` exit path for the reload sequence.

/// Exported as `ViCell_pku_active` for direct asm reference in `__trap_exit` and
/// `syscall_entry`. Set to 1 only AFTER PKU is fully initialised and IBT is confirmed.
/// The asm paths (`syscall.rs` / `boot.rs`) test this byte before executing `wrpkru`
/// to avoid #UD on CPUs that do not support PKU.
///
/// `#[used]` keeps the symbol alive in release builds (no other Rust code reads it;
/// only asm addresses it by name). `#[export_name]` controls the linker symbol name.
///
/// # Safety
/// Written exactly once at boot from a single-threaded init path (`init()`), then
/// treated as read-only by all asm callers. No concurrent Rust reference exists after boot.
#[used]
#[export_name = "ViCell_pku_active"]
pub static mut PKU_ACTIVE: u8 = 0;

/// Hardware PKU/PKS capability flags detected at runtime.
pub struct PkuCaps {
    /// CPU supports PKU (Protection Keys for Userspace) — CPUID.(7,0).ECX[3].
    pub pku: bool,
    /// CPU supports PKS (Protection Keys for Supervisor, Intel Ice Lake+) — CPUID.(7,0).ECX[31].
    pub pks: bool,
}

/// Detect PKU/PKS capabilities via CPUID leaf 7, subleaf 0.
pub fn detect() -> PkuCaps {
    // CPUID leaf 7, subleaf 0: ECX[3]=PKU, ECX[31]=PKS.
    // LLVM reserves rbx as a frame-pointer on x86_64, so we must push/pop it
    // manually around CPUID rather than using an `out("ebx")` constraint.
    let ecx: u32;
    // SAFETY: CPUID is always safe from Ring-0; no side effects on memory.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "pop rbx",
            inout("eax") 7u32 => _,
            inout("ecx") 0u32 => ecx,
            out("edx") _,
            options(nostack)
        );
    }
    PkuCaps {
        pku: (ecx >> 3) & 1 != 0,
        pks: (ecx >> 31) & 1 != 0,
    }
}

/// Compute the PKRU value for a cell running in `key` domain.
///
/// Allows key 0 (trusted-core / shared read-only pages) and `key` (own domain).
/// Denies all other keys (AD=1, WD=0 — access disabled, writes allowed if AD=0,
/// but AD already gates access so WD is irrelevant here).
///
/// PKRU bit layout: bits [2k] = AD (access disable), bits [2k+1] = WD (write disable)
/// for protection key k. AD=1 means ring-3 may NOT access pages with that key.
///
/// # Returns
/// - `0` for key 0 (trusted-core: all keys accessible — kernel default).
/// - Otherwise: `0x5555_5555` (all-deny) with bits for key 0 and `key` cleared.
pub fn pkru_for_key(key: u8) -> u32 {
    if key == 0 {
        return 0; // trusted-core: all access (PKRU=0)
    }
    // All-deny baseline: AD=1, WD=0 for every key → pattern 0b01 per key pair.
    // 16 keys × 2 bits = 32 bits; all pairs = 0x5555_5555.
    let all_deny: u32 = 0x5555_5555;
    // Clear the 2-bit slot for key 0 (bits [1:0]) — allow trusted-core pages.
    let allow_key0: u32 = !(0b11u32);
    // Clear the 2-bit slot for the cell's own key — allow own pages.
    let allow_own: u32 = !(0b11u32 << (key as u32 * 2));
    all_deny & allow_key0 & allow_own
}

/// Enable PKU if available AND CET-IBT is already on.
///
/// Preconditions:
/// - Called from Ring-0.
/// - `cet::init_kernel_cet()` has already run (IBT must be active first).
///
/// Refuses enforcement if CET-IBT is off — WRPKRU without IBT is bypassable
/// via JOP gadgets (ERIM 2019).
///
/// CR4.PKE (bit 22) is set here; `PKU_ACTIVE` is armed last so no asm path
/// runs `wrpkru` until the feature is fully configured.
pub fn init() {
    let caps = detect();
    if !caps.pku {
        log_str("[INFO] PKU: unavailable\n");
        return;
    }

    // Require IBT as a mandatory prerequisite.
    if !super::cet::detect().ibt {
        log_str("[WARN] PKU: skipped — CET-IBT not available (prerequisite)\n");
        return;
    }

    // Enable PKU in CR4: bit 22 = PKE (Protection Key Enable).
    // SAFETY: CR4 write from Ring-0 is a standard privileged operation.
    unsafe {
        let mut cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        cr4 |= 1u64 << 22; // PKE
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
    }

    // Kernel stays at PKRU=0 (all-access). No wrpkru needed here; the kernel
    // runs at Ring-0 and PKRU only restricts Ring-3 accesses. Ring-3 cells get
    // their pku_value set at spawn and restored on every ring-3 re-entry by the
    // asm paths in syscall.rs and boot.rs.

    // Arm the asm guard last — guarantees no ring-3 path executes wrpkru before
    // CR4.PKE is set (which would trap with #GP on CPUs without PKU).
    // SAFETY: PKU_ACTIVE is only written here (single-core init path), then
    // treated as read-only. No concurrent Rust references exist.
    unsafe {
        PKU_ACTIVE = 1;
    }

    log_str("[INFO] PKU: enabled (key 0=trusted key 1=cell key 2=ffi)\n");

    #[cfg(feature = "pks")]
    {
        if caps.pks {
            init_pks();
        } else {
            log_str("[INFO] PKS: unavailable\n");
        }
    }
}

/// Enable PKS (supervisor protection keys) via MSR IA32_PKRS.
///
/// PKS is Intel Ice Lake+ only (AMD lacks it). Gated by the `pks` cargo feature.
#[cfg(feature = "pks")]
fn init_pks() {
    // IA32_PKRS MSR: supervisor key-rights register (analogous to PKRU for Ring-0).
    const MSR_IA32_PKRS: u32 = 0x6E1;
    // PKRS=0 = all-access for the kernel (no supervisor key restrictions by default).
    let lo: u32 = 0;
    let hi: u32 = 0;
    // SAFETY: wrmsr from Ring-0 to a valid MSR; MSR_IA32_PKRS is safe to write.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") MSR_IA32_PKRS,
            in("eax") lo,
            in("edx") hi,
            options(nomem, nostack)
        );
    }
    log_str("[INFO] PKS: enabled (supervisor keys)\n");
}

fn log_str(s: &str) {
    for c in s.bytes() {
        super::uart_16550::putchar(c);
    }
}
