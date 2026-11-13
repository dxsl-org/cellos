//! Ledger + live-leaf validation for probed user ranges. Both the
//! private-root (Domain) and shared-root (Sas) probe passes live here; no
//! byte ever moves in this module.

use super::copy::PinnedCopy;
use super::range::{CopyError, Direction};
use crate::memory::address_space::AddressSpace;
use crate::memory::paging::{Flags, PAGE_SIZE};
use hal::Leaf;

/// Whether `leaf` is a valid user leaf with the right `direction` needs.
pub(super) fn user_leaf_allows(leaf: &Leaf, direction: Direction) -> bool {
    let right = if direction == Direction::ToUser {
        leaf.write
    } else {
        leaf.read
    };
    leaf.valid && leaf.user && right
}

/// Probe every page of `[ptr, ptr+len)` against BOTH the mapping ledger and
/// the live leaves of the private root under an acquired reader pin.
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

    // 2. Validate every page under the held pin.
    let mut page = ptr & !(PAGE_SIZE - 1);
    let root_is_domain = crate::hal::paging::current_root() == Some(root_pa);
    while page < end {
        let (ledger_flags, ledger_pa) =
            arc.page_proof_for(page).ok_or(CopyError::InvalidAddress)?;
        // Strict U + R/W contract for private roots.
        if ledger_flags.bits() & (Flags::USER | need_flag) != Flags::USER | need_flag {
            return Err(CopyError::InvalidAddress);
        }
        if !root_is_domain {
            // SAFETY: the reader pin keeps the private root and its tables
            // live for the whole walk.
            let leaf = unsafe { crate::hal::paging::leaf(root_pa, page) }
                .ok_or(CopyError::InvalidAddress)?;
            if !user_leaf_allows(&leaf, direction) {
                return Err(CopyError::InvalidAddress);
            }
            if leaf.pa != ledger_pa {
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
    let Some(root_pa) = crate::hal::paging::current_root() else {
        return Err(CopyError::InvalidAddress);
    };
    sas_span_live(ptr, len, direction, |page| {
        // SAFETY: the live root of this hart; its tables outlive the probe.
        unsafe { crate::hal::paging::leaf(root_pa, page) }
    })
}

/// Require every page of `[ptr, ptr+len)` to resolve to a valid user leaf
/// with the direction's R/W right. `leaf` returns the leaf for one page VA. A
/// retired page (unmapped, or re-mapped kernel-only) fails here.
pub(super) fn sas_span_live(
    ptr: usize,
    len: usize,
    direction: Direction,
    leaf: impl Fn(usize) -> Option<Leaf>,
) -> Result<(), CopyError> {
    let end = ptr.checked_add(len).ok_or(CopyError::InvalidAddress)?;
    let mut page = ptr & !(PAGE_SIZE - 1);
    while page < end {
        let leaf = leaf(page).ok_or(CopyError::InvalidAddress)?;
        if !user_leaf_allows(&leaf, direction) {
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
