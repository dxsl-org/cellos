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
    let mut table_pa = root_pa;
    for level in 0..3usize {
        let index = (va >> (30 - 9 * level)) & 0x1FF;
        // SAFETY: page-table frames are owned by the address space and stay
        // resident for the walk's lifetime; reads are plain loads through the
        // kernel linear alias.
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

/// Physical root of the currently resident Sv39 table, if paging is active.
pub(crate) fn current_satp_root() -> Option<usize> {
    let satp: usize;
    // SAFETY: CSR reads have no side effects.
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
/// share one map and the kernel dereferences through SUM=1, so any live
/// mapping with the required R/W right qualifies — the U bit is deliberately
/// NOT required here. Kernel-image rejection for Sas therefore reduces to the
/// canonical bound plus whatever the walk refuses; private-root views enforce
/// the strict U + R/W ledger contract above.
///
/// Documented retained legacy exposure: Sas identity-maps RAM, so a wild
/// pointer inside the identity-mapped RAM range passes this probe by design —
/// only VAs with no live Sv39 leaf (true holes between mapped regions) are
/// rejected before any byte moves. Concurrent-unmap residual is legacy Sas
/// semantics, converted to a recoverable error by the guard.
pub(super) fn probe_sas(ptr: usize, len: usize, direction: Direction) -> Result<(), CopyError> {
    let end = ptr.checked_add(len).ok_or(CopyError::InvalidAddress)?;
    let Some(root_pa) = current_satp_root() else {
        return Err(CopyError::InvalidAddress);
    };
    let need = if direction == Direction::ToUser {
        SV39_WRITE | SV39_USER
    } else {
        SV39_READ | SV39_USER
    };
    let mut page = ptr & !(PAGE_SIZE - 1);
    while page < end {
        let (bits, _) = sv39_leaf(root_pa, page).ok_or(CopyError::InvalidAddress)?;
        if bits & (SV39_VALID | need) != SV39_VALID | need {
            return Err(CopyError::InvalidAddress);
        }
        page += PAGE_SIZE;
    }
    Ok(())
}

/// TEST HOOK: stage (probe + pin) a domain copy without committing it, so a
/// fixture can inject a protocol violation between the two passes.
#[cfg(feature = "test-hooks")]
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
