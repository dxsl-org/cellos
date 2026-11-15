//! ARM64 Stage-2 EL2 system-register helpers. Pi's EL1 host invokes the
//! retained EL2 monitor via HVC; QEMU virt's EL2 host writes registers directly.
//! A page-table update must be visible before any TLBI or guest entry.
//!
//! # Register layout
//!
//! ## VTCR_EL2 (40-bit IPA, 4 KB granule, SL0=1 start at Level 1)
//! | Field | Bits | Value |
//! |-------|------|-------|
//! | T0SZ  | [5:0] | 24 (IPA = 2^40 = 1 TiB) |
//! | SL0   | [7:6] | 0b01 (start at L1) |
//! | IRGN0 | [9:8] | 0b01 (Normal WB-WA inner) |
//! | ORGN0 | [11:10] | 0b01 (Normal WB-WA outer) |
//! | SH0   | [13:12] | 0b11 (Inner-shareable) |
//! | PS    | [18:16] | 0b010 (40-bit PA size) |
//! | RES1  | [31] | 1 |
//!
//! Computed: `0x80023558`
//!
//! ## VTTBR_EL2
//! bits[63:48] = VMID (≥ 1; host is VMID 0)
//! bits[47:1]  = BADDR (root PA[47:13] for 8 KB-aligned root)
//!
//! ## HCR_EL2.VM (bit 0)
//! Setting this bit enables Stage-2 translation for EL0/EL1.
//! Must be cleared before tearing down the Stage-2 table.

/// VTCR_EL2 value for 40-bit IPA, 4 KB granule, Level-1 start (SL0=1),
/// Normal-WB-WA Inner+Outer-shareable, bit[31]=RES1.
///
/// T0SZ=24 | SL0=1 | IRGN0=01 | ORGN0=01 | SH0=11 | PS=010 | RES1
#[cfg(target_arch = "aarch64")]
const VTCR_VALUE: u64 = 0x8002_3558;

#[cfg(target_arch = "aarch64")]
use super::backend::{ArmEl2Monitor, El2Monitor};

/// Arm Stage-2 translation for the given `vmid` and root page-table at `root_pa`.
///
/// Programs VTCR_EL2 and VTTBR_EL2 and issues a full Stage-2 TLB invalidation.
/// It does NOT set HCR_EL2.VM: the host's Cells keep running at EL0 after this
/// returns, and with VM=1 their EL1&0 stage-1 walks are themselves Stage-2
/// translated through the guest map — the first post-syscall instruction fetch
/// then walks garbage and the Cell dies on a wild PC (observed as an
/// instruction abort at return-address | 0x06000000 on QEMU 8.2). VM is owned
/// exclusively by the world-switch: `vcpu_enter_guest` sets it on guest entry
/// and `vt_vcpu_trap` clears it on every guest exit (vcpu.rs "CRITICAL" note).
///
/// # Safety
/// - Must be called with an active EL2 host, or the Pi EL1 monitor initialized.
/// - `root_pa` must be 8 KB-aligned (concatenated 2 × L1 tables, VTCR.SL0=1).
/// - The Stage-2 page table must be fully populated; the entry `dsb ishst`
///   publishes WB descriptor stores to the Inner-shareable walkers.
/// - `vmid` must be ≥ 1 (VMID 0 is reserved for the EL2 host).
/// - This function must NOT be called while a vCPU is running (VTTBR races).
#[cfg(target_arch = "aarch64")]
pub unsafe fn monitor_enable_stage2(vmid: u16, root_pa: u64) {
    debug_assert_eq!(root_pa % (2 * 4096), 0, "S2 root must be 8 KB-aligned");
    debug_assert!(vmid >= 1, "VMID 0 is reserved for the EL2 host");

    let vttbr = ((vmid as u64) << 48) | root_pa;

    // SAFETY: caller guarantees EL2 context and well-formed root PA.
    unsafe {
        core::arch::asm!(
            // Stage-2 descriptors were populated by EL1 (Pi) or EL2 (virt).
            // Complete their WB stores before programming the walk and TLBI.
            "dsb ishst",
            // 1. Program VTCR_EL2 (S2 translation parameters).
            "msr vtcr_el2,  {vtcr}",
            // 2. Program VTTBR_EL2 (VMID + S2 root PA).
            "msr vttbr_el2, {vttbr}",
            "isb",
            // 3. Invalidate all Stage-1 + Stage-2 TLB entries for this VMID.
            //    HCR_EL2.VM stays 0 — vcpu_enter_guest sets it per guest entry.
            "tlbi vmalls12e1is",
            "dsb ish",
            "isb",
            vtcr  = in(reg) VTCR_VALUE,
            vttbr = in(reg) vttbr,
            options(nostack),
        );
    }
}

/// Disable Stage-2 translation (clear HCR_EL2.VM=0), flush the S2 TLB for the
/// VMID in this CPU's `VTTBR_EL2`, then clear `VTTBR_EL2` so this CPU names
/// no Stage-2 root.
///
/// Call during VM teardown before freeing the Stage-2 page table frames.
///
/// # Safety
/// Must be called from EL2 with no vCPU currently running.
#[cfg(target_arch = "aarch64")]
pub unsafe fn monitor_disable_stage2() {
    // SAFETY: EL2 context; clearing HCR_EL2.VM disables guest translation.
    unsafe {
        core::arch::asm!(
            "mrs {tmp}, hcr_el2",
            "bic {tmp}, {tmp}, #1",   // clear VM bit
            "msr hcr_el2, {tmp}",
            "isb",
            "tlbi vmalls12e1is",
            "dsb ish",
            "msr vttbr_el2, xzr",
            "isb",
            tmp = out(reg) _,
            options(nomem, nostack),
        );
    }
}

/// Point this CPU's Stage-2 walk at `vmid`'s root for the guest entry that
/// follows: `VTCR_EL2` and `VTTBR_EL2` only. `VTTBR_EL2` is per CPU, and a vCPU
/// may enter on any CPU, so every entry programs it; without this a vCPU ran
/// against whichever root that CPU last named. No TLB invalidation: entries
/// are tagged by VMID, and the VMID's stale entries were invalidated when its
/// VM was created ([`monitor_enable_stage2`]).
///
/// # Safety
/// Must be called from EL2 with no vCPU running on this CPU, under the same
/// conditions as [`monitor_enable_stage2`].
#[cfg(target_arch = "aarch64")]
pub unsafe fn monitor_load_stage2(vmid: u16, root_pa: u64) {
    debug_assert_eq!(root_pa % (2 * 4096), 0, "S2 root must be 8 KB-aligned");
    debug_assert!(vmid >= 1, "VMID 0 is reserved for the EL2 host");
    let vttbr = ((vmid as u64) << 48) | root_pa;
    // SAFETY: caller guarantees EL2 context and a live, well-formed root.
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "msr vtcr_el2,  {vtcr}",
            "msr vttbr_el2, {vttbr}",
            "isb",
            vtcr  = in(reg) VTCR_VALUE,
            vttbr = in(reg) vttbr,
            options(nostack),
        );
    }
}

/// Clear this CPU's `VTTBR_EL2`, so it names no Stage-2 root once its guest
/// has exited: a root freed later is then named by no CPU.
///
/// # Safety
/// Must be called from EL2 with no vCPU running on this CPU (`HCR_EL2.VM`
/// clear).
#[cfg(target_arch = "aarch64")]
pub unsafe fn monitor_clear_stage2_root() {
    // SAFETY: EL2 context; VTTBR_EL2 is unused while HCR_EL2.VM is clear.
    unsafe {
        core::arch::asm!("msr vttbr_el2, xzr", "isb", options(nomem, nostack));
    }
}

/// Invalidate all Stage-1 + Stage-2 TLB entries for all VMIDs (broadcast).
///
/// Use after bulk Stage-2 table changes or during VM teardown. `TLBI ALLE1IS`
/// covers every VMID of the EL1&0 regime; `TLBI VMALLS12E1IS` would cover only
/// the VMID in this CPU's `VTTBR_EL2`, which outside a guest run names no VM.
///
/// # Safety
/// Must be called from EL2.
#[cfg(target_arch = "aarch64")]
pub unsafe fn monitor_s2_tlb_flush_all() {
    // SAFETY: EL2 broadcast TLB invalidation; no memory mutation.
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi alle1is",
            "dsb ish",
            "isb",
            options(nostack),
        );
    }
}

/// Invalidate the Stage-2 TLB entry for a single guest IPA.
///
/// Encodes `ipa >> 12` into the `TLBI IPAS2E1IS` operand as required by the
/// ARM architecture (D8-7204: Xt = IPA[47:12]).  Also issues `TLBI VMALLE1IS`
/// to flush any Stage-1 entries cached during the S2 walk.
///
/// # Safety
/// Must be called from EL2 after updating the Stage-2 descriptor for `ipa`.
#[cfg(target_arch = "aarch64")]
pub unsafe fn monitor_s2_tlb_flush_ipa(ipa: u64) {
    let encoded = ipa >> 12; // TLBI IPAS2E1IS encodes IPA[47:12] in Xt
                             // SAFETY: EL2 TLB maintenance; encoded IPA operand is correct per ARM DDI.
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi ipas2e1is, {x}",  // invalidate Stage-2 entry for this IPA
            "dsb ish",
            "tlbi vmalle1is",       // flush Stage-1 entries cached during S2 walk
            "dsb ish",
            "isb",
            x = in(reg) encoded,
            options(nostack),
        );
    }
}

/// This CPU's `VTTBR_EL2` (VMID and Stage-2 root); 0 when it names no root.
///
/// # Safety
/// Must be called from EL2 (an EL2 host; the Pi kernel runs at EL1).
#[cfg(target_arch = "aarch64")]
pub unsafe fn monitor_read_vttbr() -> u64 {
    let vttbr: u64;
    // SAFETY: EL2 context; reading VTTBR_EL2 has no side effects.
    unsafe {
        core::arch::asm!("mrs {v}, vttbr_el2", v = out(reg) vttbr, options(nomem, nostack));
    }
    vttbr
}

/// Bind an S2 root through the private Pi HVC gateway or directly at an EL2 host.
///
/// # Safety
/// Must be called with an active EL2 host, or after the Pi EL1 monitor
/// initialized. `root_pa` must be 8 KB-aligned, `vmid` must be ≥ 1 (VMID 0 is
/// reserved for the EL2 host), the Stage-2 page table must be fully populated
/// and flushed to RAM, and no vCPU may be running (VTTBR race).
#[cfg(target_arch = "aarch64")]
pub unsafe fn enable_stage2(vmid: u16, root_pa: u64) {
    if El2Monitor::PRESENT {
        assert!(El2Monitor::is_ready(), "EL2 monitor unavailable");
        assert_eq!(El2Monitor::gateway(1, vmid as u64, root_pa, 0, 0), 0);
    } else {
        unsafe {
            monitor_enable_stage2(vmid, root_pa);
        }
    }
}

/// Program this CPU's `VTTBR_EL2` for one guest entry on an EL2 host
/// ([`monitor_load_stage2`]). The Pi monitor does the same inside its run op,
/// so this is for EL2 hosts only.
///
/// # Safety
/// As [`monitor_load_stage2`]; never called through the Pi gateway.
#[cfg(target_arch = "aarch64")]
pub unsafe fn load_stage2(vmid: u16, root_pa: u64) {
    debug_assert!(
        !El2Monitor::PRESENT,
        "the Pi monitor loads VTTBR_EL2 itself"
    );
    unsafe {
        monitor_load_stage2(vmid, root_pa);
    }
}

/// Clear this CPU's `VTTBR_EL2` through the private Pi HVC gateway or
/// directly at an EL2 host ([`monitor_clear_stage2_root`]).
///
/// # Safety
/// Must be called with an active EL2 host, or after the Pi EL1 monitor
/// initialized, and with no vCPU running on this CPU.
#[cfg(target_arch = "aarch64")]
pub unsafe fn clear_stage2_root() {
    if El2Monitor::PRESENT {
        assert!(El2Monitor::is_ready(), "EL2 monitor unavailable");
        assert_eq!(El2Monitor::gateway(7, 0, 0, 0, 0), 0);
    } else {
        unsafe {
            monitor_clear_stage2_root();
        }
    }
}

/// Unbind the S2 root through the private Pi HVC gateway or directly at an EL2 host.
///
/// # Safety
/// Must be called with an active EL2 host, or after the Pi EL1 monitor
/// initialized, and with no vCPU running (clearing `HCR_EL2.VM` under a live
/// guest would drop its translation).
#[cfg(target_arch = "aarch64")]
pub unsafe fn disable_stage2() {
    if El2Monitor::PRESENT {
        assert!(El2Monitor::is_ready(), "EL2 monitor unavailable");
        assert_eq!(El2Monitor::gateway(2, 0, 0, 0, 0), 0);
    } else {
        unsafe {
            monitor_disable_stage2();
        }
    }
}

/// Invalidate every Stage-2 entry through the private Pi HVC gateway or
/// directly at an EL2 host.
///
/// # Safety
/// Must be called with an active EL2 host, or after the Pi EL1 monitor
/// initialized.
#[cfg(target_arch = "aarch64")]
pub unsafe fn s2_tlb_flush_all() {
    if El2Monitor::PRESENT {
        assert!(El2Monitor::is_ready(), "EL2 monitor unavailable");
        assert_eq!(El2Monitor::gateway(3, 0, 0, 0, 0), 0);
    } else {
        unsafe {
            monitor_s2_tlb_flush_all();
        }
    }
}

/// Invalidate the Stage-2 entry for one guest IPA through the private Pi HVC
/// gateway or directly at an EL2 host.
///
/// # Safety
/// Must be called with an active EL2 host, or after the Pi EL1 monitor
/// initialized, after updating the Stage-2 descriptor for `ipa`.
#[cfg(target_arch = "aarch64")]
pub unsafe fn s2_tlb_flush_ipa(ipa: u64) {
    if El2Monitor::PRESENT {
        assert!(El2Monitor::is_ready(), "EL2 monitor unavailable");
        assert_eq!(El2Monitor::gateway(4, ipa, 0, 0, 0), 0);
    } else {
        unsafe {
            monitor_s2_tlb_flush_ipa(ipa);
        }
    }
}

// ── Non-AArch64 stubs (keeps workspace compiling on RISC-V / x86_64) ────────
// `unsafe` mirrors the aarch64 signatures so call sites are identical.

/// No-op stub for non-AArch64 targets.
///
/// # Safety
/// Always safe — does nothing; kept `unsafe` to match the aarch64 signature.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn enable_stage2(_vmid: u16, _root_pa: u64) {}

/// No-op stub for non-AArch64 targets.
///
/// # Safety
/// Always safe — does nothing; kept `unsafe` to match the aarch64 signature.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn load_stage2(_vmid: u16, _root_pa: u64) {}

/// No-op stub for non-AArch64 targets.
///
/// # Safety
/// Always safe — does nothing; kept `unsafe` to match the aarch64 signature.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn clear_stage2_root() {}

/// No-op stub for non-AArch64 targets.
///
/// # Safety
/// Always safe — does nothing; kept `unsafe` to match the aarch64 signature.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn disable_stage2() {}

/// No-op stub for non-AArch64 targets.
///
/// # Safety
/// Always safe — does nothing; kept `unsafe` to match the aarch64 signature.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn s2_tlb_flush_all() {}

/// No-op stub for non-AArch64 targets.
///
/// # Safety
/// Always safe — does nothing; kept `unsafe` to match the aarch64 signature.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn s2_tlb_flush_ipa(_ipa: u64) {}
