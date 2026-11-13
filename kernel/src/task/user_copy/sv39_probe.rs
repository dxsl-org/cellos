//! Ledger + live-Sv39-PTE validation for probed user ranges. Both the
//! private-root (Domain) and shared-root (Sas) probe passes live here; no
//! byte ever moves in this module.

use super::copy::PinnedCopy;
use super::range::{CopyError, Direction};
use crate::memory::address_space::AddressSpace;
use crate::memory::paging::{Flags, PAGE_SIZE};

pub(crate) const SV39_VALID: usize = 1 << 0;
pub(crate) const SV39_READ: usize = 1 << 1;
pub(crate) const SV39_WRITE: usize = 1 << 2;
pub(crate) const SV39_USER: usize = 1 << 4;

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
    let satp_is_domain = crate::arch::page_probe::current_root() == Some(root_pa);
    while page < end {
        let (ledger_flags, ledger_pa) =
            arc.page_proof_for(page).ok_or(CopyError::InvalidAddress)?;
        // Strict U + R/W contract for private roots.
        if ledger_flags.bits() & (Flags::USER | need_flag) != Flags::USER | need_flag {
            return Err(CopyError::InvalidAddress);
        }
        if !satp_is_domain {
            let (pte_bits, pte_pa) =
                crate::arch::page_probe::leaf(root_pa, page).ok_or(CopyError::InvalidAddress)?;
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
    let Some(root_pa) = crate::arch::page_probe::current_root() else {
        return Err(CopyError::InvalidAddress);
    };
    sas_span_live(ptr, len, direction, |page| {
        crate::arch::page_probe::leaf(root_pa, page).map(|(bits, _)| bits)
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
