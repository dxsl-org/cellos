//! Ledger + live-Sv39-PTE validation for probed user ranges. Both the
//! private-root (Domain) and shared-root (Sas) probe passes live here; no
//! byte ever moves in this module.

use super::copy::PinnedCopy;
use super::range::{CopyError, Direction};
use crate::memory::address_space::AddressSpace;
use crate::memory::paging::{Flags, PAGE_SIZE};

pub(super) const SV39_VALID: usize = 1 << 0;
pub(super) const SV39_READ: usize = 1 << 1;
pub(super) const SV39_WRITE: usize = 1 << 2;
pub(super) const SV39_USER: usize = 1 << 4;

/// Walk one Sv39 root and return `(PTE flag bits, resolved physical address)`
/// for the leaf translating `va`. Reads page-table memory through the kernel
/// linear mapping, so the walk itself can never take a user page fault.
pub(crate) fn sv39_leaf(root_pa: usize, va: usize) -> Option<(usize, usize)> {
    #[cfg(target_arch = "riscv64")]
    {
        let mut table_pa = root_pa;
        for level in 0..3usize {
            let index = (va >> (30 - 9 * level)) & 0x1FF;
            let pte = unsafe {
                core::ptr::read_volatile(
                    crate::memory::frame::phys_to_virt(table_pa + index * 8) as *const u64
                )
            } as usize;
            if pte & SV39_VALID == 0 {
                return None;
            }
            if pte & (SV39_READ | SV39_WRITE | (1 << 3)) != 0 {
                let shift = 12 + 9 * (2 - level);
                let off_mask = (1usize << shift) - 1;
                let pa = (((pte >> 10) & ((1usize << 44) - 1)) << 12) | (va & off_mask);
                return Some((pte & 0xFF, pa));
            }
            table_pa = ((pte >> 10) & ((1usize << 44) - 1)) << 12;
        }
        None
    }
    #[cfg(target_arch = "aarch64")]
    {
        let table =
            unsafe { &*(crate::memory::frame::phys_to_virt(root_pa) as *const hal::PageTable) };
        let leaf = table.leaf_entry(va)?;
        let pa = (leaf as usize) & 0x0000_ffff_ffff_f000;
        let mut bits = 0usize;
        if leaf & 1 != 0 {
            bits |= SV39_VALID;
        }
        if leaf & (1 << 6) != 0 {
            bits |= SV39_USER;
        }
        bits |= SV39_READ;
        if leaf & (1 << 7) == 0 {
            bits |= SV39_WRITE;
        }
        Some((bits, pa))
    }
    #[cfg(target_arch = "x86_64")]
    {
        let mut table_pa = root_pa;
        for level in 0..4usize {
            let shift = 39 - 9 * level;
            let index = (va >> shift) & 0x1FF;
            let pte = unsafe {
                core::ptr::read_volatile(
                    crate::memory::frame::phys_to_virt(table_pa + index * 8) as *const u64
                )
            } as usize;
            if pte & 1 == 0 {
                return None;
            }
            if level == 3 || pte & (1 << 7) != 0 {
                let pa = pte & 0x000f_ffff_ffff_f000;
                let mut bits = SV39_VALID | SV39_READ;
                if pte & (1 << 1) != 0 {
                    bits |= SV39_WRITE;
                }
                if pte & (1 << 2) != 0 {
                    bits |= SV39_USER;
                }
                return Some((bits, pa));
            }
            table_pa = pte & 0x000f_ffff_ffff_f000;
        }
        None
    }
    #[cfg(not(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )))]
    None
}

/// Physical root of the currently resident domain table, if paging is active.
pub(crate) fn current_satp_root() -> Option<usize> {
    #[cfg(target_arch = "riscv64")]
    {
        let satp: usize;
        unsafe {
            core::arch::asm!(
                "csrr {satp}, satp",
                satp = out(reg) satp,
                options(nostack, nomem)
            );
        }
        if (satp >> 60) != 8 {
            return None;
        }
        Some((satp & ((1usize << 44) - 1)) << 12)
    }
    #[cfg(target_arch = "aarch64")]
    {
        let ttbr0: usize;
        unsafe {
            core::arch::asm!(
                "mrs {ttbr0}, ttbr0_el1",
                ttbr0 = out(reg) ttbr0,
                options(nostack, nomem)
            );
        }
        Some(ttbr0 & 0x0000_ffff_ffff_f000)
    }
    #[cfg(target_arch = "x86_64")]
    {
        let cr3: usize;
        unsafe {
            core::arch::asm!(
                "mov {cr3}, cr3",
                cr3 = out(reg) cr3,
                options(nostack, nomem)
            );
        }
        Some(cr3 & !0xFFF)
    }
    #[cfg(not(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )))]
    None
}

/// Probe every page of `[ptr, ptr+len)` against BOTH the mapping ledger and
/// the live Sv39 PTEs of the private root under an acquired reader pin.
/// Zero heap allocations: pages are validated and committed directly from `(ptr, len)`.
pub(super) fn stage_domain<'a>(
    arc: &'a AddressSpace,
    direction: Direction,
    ptr: usize,
    len: usize,
) -> Result<PinnedCopy<'a>, CopyError> {
    // 1. Acquire the reader pin FIRST: revoke paths must wait for us to drain
    // before they may remove a single PTE or reuse frames.
    let reader = arc
        .acquire_copy_reader()
        .map_err(|_| CopyError::InvalidAddress)?;

    let end = ptr.checked_add(len).ok_or(CopyError::InvalidAddress)?;
    let root_pa = arc.root_ppn() << 12;
    let need_flag = if direction == Direction::ToUser {
        Flags::WRITE
    } else {
        Flags::READ
    };
    let need_pte = if direction == Direction::ToUser {
        SV39_WRITE
    } else {
        SV39_READ
    };

    // 2. Validate every page under the held pin.
    let mut page = ptr & !(PAGE_SIZE - 1);
    let satp_is_domain = current_satp_root() == Some(root_pa);
    while page < end {
        let (ledger_flags, ledger_pa) =
            arc.page_proof_for(page).ok_or(CopyError::InvalidAddress)?;
        // Strict U + R/W contract for private roots.
        if ledger_flags.bits() & (Flags::USER | need_flag) != Flags::USER | need_flag {
            return Err(CopyError::InvalidAddress);
        }
        if !satp_is_domain {
            let (pte_bits, pte_pa) = sv39_leaf(root_pa, page).ok_or(CopyError::InvalidAddress)?;
            if pte_bits & (SV39_VALID | SV39_USER | need_pte) != SV39_VALID | SV39_USER | need_pte {
                return Err(CopyError::InvalidAddress);
            }
            if pte_pa != ledger_pa {
                return Err(CopyError::InvalidAddress);
            }
        }
        page += PAGE_SIZE;
    }

    Ok(PinnedCopy {
        arc,
        reader,
        direction,
        user_ptr: ptr,
        len,
    })
}

/// Probe `[ptr, ptr+len)` against the currently resident root.
///
/// Sas RETAINS today's shared-address-space semantics: cells and the kernel
/// share one map, so any live USER mapping with the required R/W right
/// qualifies — ownership is not checked. Private-root views enforce the strict
/// ledger contract above.
///
/// Documented retained legacy exposure: Sas identity-maps RAM, so a wild
/// pointer at another cell's USER page passes this probe by design. This probe
/// is NOT the authority for the commit: a page may be retired between probe
/// and commit, and a retired page is re-mapped kernel-RWX — writable by the
/// kernel without any fault — before its frame is reused. The commit therefore
/// re-validates every page inside its leased window (`sas_lease`), which is
/// what keeps a racing release from receiving bytes after the frame is freed.
pub(super) fn probe_sas(ptr: usize, len: usize, direction: Direction) -> Result<(), CopyError> {
    let Some(root_pa) = current_satp_root() else {
        return Err(CopyError::InvalidAddress);
    };
    sas_span_live(ptr, len, direction, |page| {
        sv39_leaf(root_pa, page).map(|(bits, _)| bits)
    })
}

/// Require every page of `[ptr, ptr+len)` to resolve VALID | USER plus the
/// direction's R/W right. `leaf` returns the PTE bits for one page VA. A
/// retired page (unmapped, or re-mapped kernel-only) fails here.
pub(super) fn sas_span_live(
    ptr: usize,
    len: usize,
    direction: Direction,
    leaf: impl Fn(usize) -> Option<usize>,
) -> Result<(), CopyError> {
    let end = ptr.checked_add(len).ok_or(CopyError::InvalidAddress)?;
    let need = if direction == Direction::ToUser {
        SV39_VALID | SV39_WRITE | SV39_USER
    } else {
        SV39_VALID | SV39_READ | SV39_USER
    };
    let mut page = ptr & !(PAGE_SIZE - 1);
    while page < end {
        let bits = leaf(page).ok_or(CopyError::InvalidAddress)?;
        if bits & need != need {
            return Err(CopyError::InvalidAddress);
        }
        page += PAGE_SIZE;
    }
    Ok(())
}

/// TEST HOOK: stage (probe + pin) a domain copy without committing it, so a
/// fixture can inject a protocol violation between the two passes.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) fn stage_domain_for_test<'a>(
    arc: &'a AddressSpace,
    ptr: usize,
    len: usize,
    write: bool,
) -> Result<PinnedCopy<'a>, CopyError> {
    let direction = if write {
        Direction::ToUser
    } else {
        Direction::FromUser
    };
    stage_domain(arc, direction, ptr, len)
}
