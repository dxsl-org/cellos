//! GICv2 Virtual CPU Interface Control (GICH) MMIO register access for EL2 VMM.
//!
//! The GICH base comes from the board (`ArmResources::VGIC`); boards without a
//! GICv2 virtualization interface never reach this module. Functions that touch
//! GICH are `unsafe` — the caller must be at EL2 with GICH MMIO accessible. The
//! list-register planning ([`plan_entry_lrs`], [`sort_exit_lrs`]) and the timer
//! line ([`vtimer_line_asserted`]) are pure, so they compile and are tested on
//! the host.
//!
//! # GICH_LR encoding for a Group 0 Pending virtual IRQ
//! `[30]=Group1=0, [29:28]=Pending=0b01, [27:23]=Priority=0, [9:0]=VIntID`
//! → value = `0x1000_0000 | intid`
//!
//! Group 0, because a GICv2 guest enables its CPU interface with `GICC_CTLR = 1`:
//! on the virtual interface (`GICV_CTLR`) bit 0 is EnableGrp0, so a Group 1 list
//! register is never signalled to a Linux guest. KVM loads GICv2 guests' LRs as
//! Group 0 for the same reason.
//!
//! # GICV Stage-2 passthrough
//! The caller maps the guest's GICC IPA onto the board's GICV window read-write
//! in the Stage-2 table so guest GICC reads/writes hit real GICV hardware
//! without causing a VM exit. The guest must be able to write it: its
//! `GICC_CTLR`/`GICC_PMR` enable the interface and `GICC_EOIR` completes each
//! interrupt. Those writes only reach the running vCPU's state in GICH
//! (`GICH_VMCR`, `GICH_APR`, the LRs), which [`CpuIfState`] saves on every exit
//! and restores on every entry, so no other vCPU sees them.

const GICH_HCR: usize = 0x000;
const GICH_VTR: usize = 0x004;
const GICH_VMCR: usize = 0x008;
const GICH_ELRSR0: usize = 0x030;
const GICH_APR: usize = 0x0F0;
const GICH_LR: usize = 0x100; // LR[n] at +n*4

const LR_PENDING: u32 = 1 << 28;
const LR_ACTIVE: u32 = 1 << 29;
const LR_VINTID: u32 = 0x3FF;
/// GIC-400 GICH_VMCR reset value: interface disabled, VPMR=0, VBPR0=2, VBPR1=3.
const VMCR_RESET: u32 = 0x004C_0000;

/// Maximum list registers to use; QEMU GICv2 has 4.
pub const MAX_LRS: usize = 4;

#[inline]
unsafe fn rd32(addr: usize) -> u32 {
    // SAFETY: caller guarantees EL2 + valid MMIO address.
    core::ptr::read_volatile(addr as *const u32)
}

#[inline]
unsafe fn wr32(addr: usize, val: u32) {
    // SAFETY: caller guarantees EL2 + valid MMIO address.
    core::ptr::write_volatile(addr as *mut u32, val);
}

/// Enable GICH (set GICH_HCR.EN = 1).
///
/// # Safety
/// Caller must be at EL2; the GICH window at `gich` must be mapped.
pub unsafe fn enable(gich: usize) {
    let cur = rd32(gich + GICH_HCR);
    wr32(gich + GICH_HCR, cur | 1);
}

/// Return the number of list registers (GICH_VTR[4:0] + 1), capped at MAX_LRS.
///
/// # Safety
/// Caller must be at EL2; the GICH window at `gich` must be mapped.
pub unsafe fn lr_count(gich: usize) -> usize {
    let vtr = rd32(gich + GICH_VTR);
    (((vtr & 0x1F) + 1) as usize).min(MAX_LRS)
}

/// The guest's virtual timer line: `CNTV_CTL` ENABLE=1 and IMASK=0 (ISTATUS is
/// ignored: it is a snapshot from the last exit), and the guest's virtual count
/// `now` has reached `CNTV_CVAL` (unsigned compare, as the architecture
/// defines the timer condition).
pub fn vtimer_line_asserted(ctl: u64, cval: u64, now: u64) -> bool {
    ctl & 0b11 == 0b01 && now >= cval
}

/// Plan the list registers for one guest entry, using at most `n_lrs`.
///
/// The `active` LRs saved at the last exit come first, verbatim. Then pending
/// INTIDs from `next_pending` fill the free slots as Group 0 Pending; one that
/// matches an LR already planned sets that LR's Pending bit instead, because
/// two LRs must never carry the same virtual INTID. Pending INTIDs are pulled
/// only while a slot is free, so the rest stay in the caller's pending set.
/// Returns the LR values and how many are in use.
pub fn plan_entry_lrs(
    active: &[u32],
    n_lrs: usize,
    mut next_pending: impl FnMut() -> Option<u32>,
) -> ([u32; MAX_LRS], usize) {
    let n_lrs = n_lrs.min(MAX_LRS);
    let mut lrs = [0u32; MAX_LRS];
    let mut used = active.len().min(n_lrs);
    lrs[..used].copy_from_slice(&active[..used]);
    while used < n_lrs {
        let Some(intid) = next_pending() else {
            break;
        };
        let intid = intid & LR_VINTID;
        if let Some(lr) = lrs[..used].iter_mut().find(|lr| **lr & LR_VINTID == intid) {
            *lr |= LR_PENDING;
        } else {
            lrs[used] = LR_PENDING | intid;
            used += 1;
        }
    }
    (lrs, used)
}

/// Sort the LRs read back after an exit.
///
/// Empty LRs (`elrsr` bit set) are skipped. Active and Pending+Active LRs are
/// kept verbatim for the next entry, because the guest is still handling them.
/// Pending-only LRs go back through `repend`, except for level-sensitive
/// INTIDs (`is_level`): the caller re-samples those lines at entry, so a line
/// the guest has since deasserted is not delivered again. For the same reason
/// a kept Active level LR loses its Pending bit. Returns the kept Active LRs
/// and their count.
pub fn sort_exit_lrs(
    lrs: &[u32],
    elrsr: u32,
    is_level: impl Fn(u32) -> bool,
    mut repend: impl FnMut(u32),
) -> ([u32; MAX_LRS], usize) {
    let mut active = [0u32; MAX_LRS];
    let mut n_active = 0;
    for (n, &lr) in lrs.iter().enumerate().take(MAX_LRS) {
        if (elrsr >> n) & 1 != 0 {
            continue;
        }
        let intid = lr & LR_VINTID;
        if lr & LR_ACTIVE != 0 {
            active[n_active] = if is_level(intid) {
                lr & !LR_PENDING
            } else {
                lr
            };
            n_active += 1;
        } else if lr & LR_PENDING != 0 && !is_level(intid) {
            repend(intid);
        }
    }
    (active, n_active)
}

/// One vCPU's virtual CPU interface state. It lives in GICH only while that
/// vCPU runs: [`CpuIfState::load`] installs it before entry and
/// [`CpuIfState::save`] takes it back after exit.
#[derive(Clone, Copy)]
pub struct CpuIfState {
    /// Guest `GICV_CTLR`/`PMR`/`BPR` view (GICH_VMCR).
    vmcr: u32,
    /// Guest active priorities (GICH_APR).
    apr: u32,
    /// LRs that still held an Active (or Pending+Active) interrupt at exit.
    /// Reloading them as plain Pending would deliver an interrupt the guest is
    /// still handling a second time.
    active: [u32; MAX_LRS],
    n_active: usize,
    /// Implemented LRs (GICH_VTR), read at the first `load`; 0 until then.
    n_lrs: usize,
}

impl Default for CpuIfState {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuIfState {
    /// Interface state of a vCPU that has not run yet.
    pub const fn new() -> Self {
        Self {
            vmcr: VMCR_RESET,
            apr: 0,
            active: [0; MAX_LRS],
            n_active: 0,
            n_lrs: 0,
        }
    }

    /// Install this vCPU's interface state in the GICH window at `gich`, with
    /// LRs planned by [`plan_entry_lrs`]; every other implemented LR is
    /// cleared. Returns the number of LRs in use, which the matching
    /// [`CpuIfState::save`] needs.
    ///
    /// Precondition: INTIDs from `next_pending` are ≤ 1019 (validated by the
    /// syscall layer, m3).
    ///
    /// # Safety
    /// Caller must be at EL2 with the GICH window at `gich` mapped and no vCPU
    /// running.
    pub unsafe fn load(&mut self, gich: usize, next_pending: impl FnMut() -> Option<u32>) -> usize {
        if self.n_lrs == 0 {
            self.n_lrs = lr_count(gich);
        }
        let (lrs, used) = plan_entry_lrs(&self.active[..self.n_active], self.n_lrs, next_pending);
        self.n_active = 0;
        wr32(gich + GICH_VMCR, self.vmcr);
        wr32(gich + GICH_APR, self.apr);
        for (n, lr) in lrs.iter().enumerate().take(self.n_lrs) {
            wr32(gich + GICH_LR + n * 4, if n < used { *lr } else { 0 });
        }
        used
    }

    /// Take this vCPU's interface state back from the GICH window at `gich`
    /// after exit and clear the `used` LRs; the LRs are sorted by
    /// [`sort_exit_lrs`] with `is_level` and `repend`.
    ///
    /// # Safety
    /// Caller must be at EL2 with the GICH window at `gich` mapped and no vCPU
    /// running; `used` is the value the matching `load` returned.
    pub unsafe fn save(
        &mut self,
        gich: usize,
        used: usize,
        is_level: impl Fn(u32) -> bool,
        repend: impl FnMut(u32),
    ) {
        let used = used.min(self.n_lrs);
        let elrsr = rd32(gich + GICH_ELRSR0);
        let mut lrs = [0u32; MAX_LRS];
        for (n, lr) in lrs.iter_mut().enumerate().take(used) {
            *lr = rd32(gich + GICH_LR + n * 4);
            wr32(gich + GICH_LR + n * 4, 0);
        }
        (self.active, self.n_active) = sort_exit_lrs(&lrs[..used], elrsr, is_level, repend);
        self.vmcr = rd32(gich + GICH_VMCR);
        self.apr = rd32(gich + GICH_APR);
    }
}

#[cfg(test)]
#[path = "vgic_tests.rs"]
mod tests;
