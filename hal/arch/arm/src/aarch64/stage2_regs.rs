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
//! | VS    | [19] | 0 (8-bit VMIDs) |
//! | RES1  | [31] | 1 |
//!
//! Computed: `0x80023558`
//!
//! ## VTTBR_EL2
//! bits[63:56] = RES0 (`VTCR_EL2.VS` = 0)
//! bits[55:48] = VMID (≥ 1; host is VMID 0; see [`VmidPool`])
//! bits[47:1]  = BADDR (root PA[47:13] for 8 KB-aligned root)
//!
//! ## HCR_EL2.VM (bit 0)
//! Setting this bit enables Stage-2 translation for EL0/EL1.
//! Must be cleared before tearing down the Stage-2 table.

/// VTCR_EL2 value for 40-bit IPA, 4 KB granule, Level-1 start (SL0=1),
/// Normal-WB-WA Inner+Outer-shareable, bit[31]=RES1.
///
/// T0SZ=24 | SL0=1 | IRGN0=01 | ORGN0=01 | SH0=11 | PS=010 | VS=0 | RES1
#[cfg(target_arch = "aarch64")]
const VTCR_VALUE: u64 = 0x8002_3558;

/// `VTCR_EL2.VS`: 16-bit VMIDs. [`VTCR_VALUE`] leaves it clear, which is what
/// [`VMID_COUNT`] assumes (not every core implements 16-bit VMIDs).
#[cfg(target_arch = "aarch64")]
const VTCR_VS: u64 = 1 << 19;
#[cfg(target_arch = "aarch64")]
const _: () = assert!(VTCR_VALUE & VTCR_VS == 0);

/// VMIDs the Stage-2 configuration tells apart. With `VTCR_EL2.VS` clear a
/// VMID is `VTTBR_EL2[55:48]`: VMIDs 256 apart share TLB tags (256 aliases the
/// host's VMID 0), and a VMID above 255 sets RES0 bits of `VTTBR_EL2`.
pub const VMID_COUNT: usize = 1 << 8;

/// The guest VMIDs in use, all below [`VMID_COUNT`]. VMID 0 is the host's and
/// is never handed out, so 255 VMs can hold one at a time.
///
/// A released VMID is handed out again, so its holder must have invalidated
/// every TLB entry tagged with it, on every CPU, before [`VmidPool::release`],
/// and no CPU may still name it in `VTTBR_EL2`. `monitor_enable_stage2`
/// invalidates the VMID again when its next VM binds it.
pub struct VmidPool {
    used: [core::sync::atomic::AtomicU64; VMID_COUNT / 64],
}

impl VmidPool {
    /// A pool with every guest VMID free.
    pub const fn new() -> Self {
        use core::sync::atomic::AtomicU64;
        Self {
            used: [
                AtomicU64::new(1), // VMID 0: the host
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }

    /// Take the lowest free VMID; `None` when all 255 are in use.
    pub fn allocate(&self) -> Option<u16> {
        use core::sync::atomic::Ordering;
        for (index, word) in self.used.iter().enumerate() {
            let mut current = word.load(Ordering::Relaxed);
            while current != u64::MAX {
                let bit = (!current).trailing_zeros();
                match word.compare_exchange_weak(
                    current,
                    current | (1 << bit),
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return Some((index * 64) as u16 + bit as u16),
                    Err(seen) => current = seen,
                }
            }
        }
        None
    }

    /// Return `vmid` to the pool once no TLB holds an entry tagged with it
    /// (see [`VmidPool`]).
    pub fn release(&self, vmid: u16) {
        debug_assert!(vmid != 0, "VMID 0 is the host's");
        let mask = 1u64 << (vmid % 64);
        let previous = self.used[usize::from(vmid) / 64]
            .fetch_and(!mask, core::sync::atomic::Ordering::Release);
        debug_assert!(previous & mask != 0, "VMID {vmid} released while free");
    }

    /// Guest VMIDs free at the moment of the read; a snapshot, not a claim.
    pub fn free_count(&self) -> usize {
        self.used
            .iter()
            .map(|word| {
                word.load(core::sync::atomic::Ordering::Relaxed)
                    .count_zeros() as usize
            })
            .sum()
    }
}

impl Default for VmidPool {
    fn default() -> Self {
        Self::new()
    }
}

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
/// - `vmid` must be ≥ 1 (VMID 0 is reserved for the EL2 host) and below
///   [`VMID_COUNT`].
/// - This function must NOT be called while a vCPU is running (VTTBR races).
#[cfg(target_arch = "aarch64")]
pub unsafe fn monitor_enable_stage2(vmid: u16, root_pa: u64) {
    debug_assert_eq!(root_pa % (2 * 4096), 0, "S2 root must be 8 KB-aligned");
    debug_assert!(vmid >= 1, "VMID 0 is reserved for the EL2 host");
    debug_assert!(
        usize::from(vmid) < VMID_COUNT,
        "VMID {vmid} is wider than 8 bits"
    );

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
    debug_assert!(
        usize::from(vmid) < VMID_COUNT,
        "VMID {vmid} is wider than 8 bits"
    );
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
/// reserved for the EL2 host) and below [`VMID_COUNT`], the Stage-2 page table
/// must be fully populated and flushed to RAM, and no vCPU may be running
/// (VTTBR race).
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

#[cfg(test)]
mod tests {
    use super::{VmidPool, VMID_COUNT};

    #[test]
    fn vmids_stay_within_eight_bits_and_exhaustion_refuses() {
        let pool = VmidPool::new();
        for expected in 1..VMID_COUNT as u16 {
            assert_eq!(pool.allocate(), Some(expected));
        }
        assert_eq!(
            pool.allocate(),
            None,
            "VMID 256 would alias the host's VMID 0"
        );
        assert_eq!(pool.allocate(), None);
    }

    #[test]
    fn a_released_vmid_is_recycled_and_no_other() {
        let pool = VmidPool::new();
        while pool.allocate().is_some() {}
        pool.release(70);
        pool.release(3);
        assert_eq!(pool.allocate(), Some(3));
        assert_eq!(pool.allocate(), Some(70));
        assert_eq!(pool.allocate(), None);
    }

    #[test]
    fn free_count_tracks_allocate_and_release() {
        let pool = VmidPool::new();
        assert_eq!(pool.free_count(), VMID_COUNT - 1, "VMID 0 is the host's");
        let vmid = pool.allocate().expect("free");
        assert_eq!(pool.free_count(), VMID_COUNT - 2);
        pool.release(vmid);
        assert_eq!(pool.free_count(), VMID_COUNT - 1);
        while pool.allocate().is_some() {}
        assert_eq!(pool.free_count(), 0);
    }

    #[test]
    fn concurrent_holders_never_share_a_vmid() {
        extern crate std;
        use std::{sync::Arc, thread, vec::Vec};
        let pool = Arc::new(VmidPool::new());
        let threads: Vec<_> = (0..5)
            .map(|_| {
                let pool = Arc::clone(&pool);
                thread::spawn(move || {
                    let mut held = Vec::new();
                    while let Some(vmid) = pool.allocate() {
                        held.push(vmid);
                    }
                    held
                })
            })
            .collect();
        let mut seen = [false; VMID_COUNT];
        for thread in threads {
            for vmid in thread.join().unwrap() {
                assert!(!seen[usize::from(vmid)], "VMID {vmid} handed out twice");
                seen[usize::from(vmid)] = true;
            }
        }
        assert!(!seen[0] && seen[1..].iter().all(|&held| held));
    }
}
