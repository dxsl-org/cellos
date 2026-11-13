//! Two-pass transaction orchestration: entry points, the shared `execute`
//! body, and the pinned Domain commit. The Sas commit lives in `sas_lease`.

use super::guard::commit_guarded;
use super::page_probe::{probe_sas, stage_domain, user_leaf_allows};
use super::range::{CopyError, CopyView, Direction, UserReadSlice, UserWriteSlice};
use super::sas_lease::commit_sas;
use crate::memory::address_space::{AddressSpace, CopyReader};
use crate::memory::frame::phys_to_virt;
use crate::memory::paging::PAGE_SIZE;

/// Checked copy from user memory into `dst`. On ANY failure `dst` is left
/// untouched: the probe pass moves no bytes and the commit pass only starts
/// after every page of `src` was proven present and readable.
///
/// Requires `src.len() <= dst.len()`; a caller passing a short destination has
/// misdescribed the transaction and gets the same recoverable error.
pub(crate) fn copy_from_user(
    view: &CopyView,
    src: UserReadSlice,
    dst: &mut [u8],
) -> Result<(), CopyError> {
    if src.len() > dst.len() {
        return Err(CopyError::InvalidAddress);
    }
    execute(
        view,
        Direction::FromUser,
        src.ptr(),
        dst.as_mut_ptr(),
        src.len(),
    )
}

/// Checked copy from kernel `src` into user memory. On ANY failure the user
/// destination is untouched: the commit pass only starts after every page of
/// `dst` was proven present and writable.
pub(crate) fn copy_to_user(
    view: &CopyView,
    dst: UserWriteSlice,
    src: &[u8],
) -> Result<(), CopyError> {
    if dst.len() > src.len() {
        return Err(CopyError::InvalidAddress);
    }
    execute(
        view,
        Direction::ToUser,
        dst.ptr(),
        src.as_ptr().cast_mut(),
        dst.len(),
    )
}
/// Probe a destination without moving any bytes.
///
/// The caller may use this before an irreversible operation and then repeat the
/// full copy under its own short-lived ownership lease.
pub(crate) fn probe_writable(view: &CopyView, dst: UserWriteSlice) -> Result<(), CopyError> {
    if dst.len() == 0 {
        return Ok(());
    }
    match view {
        CopyView::Domain(arc) => {
            if !arc.is_live() {
                return Err(CopyError::InvalidAddress);
            }
            drop(stage_domain(arc, Direction::ToUser, dst.ptr(), dst.len())?);
            Ok(())
        }
        CopyView::Sas => probe_sas(dst.ptr(), dst.len(), Direction::ToUser),
    }
}

/// Shared transaction body. `other` is the kernel-side buffer pointer; the
/// user-side pointer comes from the validated slice.
fn execute(
    view: &CopyView,
    direction: Direction,
    user_ptr: usize,
    other: *mut u8,
    len: usize,
) -> Result<(), CopyError> {
    if len == 0 {
        return Ok(());
    }
    match view {
        CopyView::Domain(arc) => {
            // Reject Dying before the pin (the double-check inside
            // acquire_copy_reader covers the post-pin edge).
            if !arc.is_live() {
                return Err(CopyError::InvalidAddress);
            }
            let staged = stage_domain(arc, direction, user_ptr, len)?;
            staged.commit(other)
        }
        CopyView::Sas => {
            probe_sas(user_ptr, len, direction)?;
            commit_sas(direction, user_ptr, other, len)
        }
    }
}

/// A probed, pinned user range awaiting its byte-copy commit.
pub(crate) struct PinnedCopy<'a> {
    pub(super) arc: &'a AddressSpace,
    /// Held purely for its drain side effect: while this lease exists no
    /// revocation path may tear down a PTE of the pinned root.
    #[allow(dead_code)]
    pub(super) reader: CopyReader<'a>,
    pub(super) direction: Direction,
    pub(super) user_ptr: usize,
    pub(super) len: usize,
}

impl PinnedCopy<'_> {
    /// Run the commit pass. `other` is the kernel-side buffer.
    ///
    /// FAULT-FREEDOM PROOF AT THE COPY SITE: the reader pin blocks every
    /// revocation path (`unmap_private_page`, `unmap_grant_page`) until it
    /// drains. Every page is re-walked directly from `(user_ptr, len)` under
    /// that pin with zero heap allocation.
    pub(crate) fn commit(self, other: *mut u8) -> Result<(), CopyError> {
        let root_pa = self.arc.root_ppn() << 12;
        if crate::hal::paging::current_root() == Some(root_pa) {
            let (src, dst) = if self.direction == Direction::FromUser {
                (self.user_ptr as *const u8, other)
            } else {
                (other as *const u8, self.user_ptr as *mut u8)
            };
            return commit_guarded(src, dst, self.len, self.user_ptr, self.user_ptr + self.len);
        }
        let end = self.user_ptr + self.len;
        let mut page = self.user_ptr & !(PAGE_SIZE - 1);
        let mut result = Ok(());
        while page < end {
            let lo = page.max(self.user_ptr);
            let hi = (page + PAGE_SIZE).min(end);
            let chunk = hi - lo;
            // SAFETY: the reader pin keeps the private root and its tables
            // live until this commit returns.
            let Some(leaf) = (unsafe { crate::hal::paging::leaf(root_pa, page) }) else {
                return Err(CopyError::InvalidAddress);
            };
            if !user_leaf_allows(&leaf, self.direction) {
                return Err(CopyError::InvalidAddress);
            }
            let offset = lo - page;
            let alias = phys_to_virt(leaf.pa + offset);
            let (src, dst): (*const u8, *mut u8) = if self.direction == Direction::FromUser {
                (alias as *const u8, other.wrapping_add(lo - self.user_ptr))
            } else {
                (other.wrapping_add(lo - self.user_ptr), alias as *mut u8)
            };
            if commit_guarded(src, dst, chunk, alias, alias + chunk).is_err() {
                result = Err(CopyError::InvalidAddress);
                break;
            }
            page += PAGE_SIZE;
        }
        result
    }
}
