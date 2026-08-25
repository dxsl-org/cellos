//! Multi-destination scatter transaction orchestration.

use super::copy::commit_sas;
use super::range::{CopyError, CopyView, Direction, UserWriteSlice};
use super::sv39_probe::{probe_sas, stage_domain};
use alloc::vec::Vec;

/// Checked copy from kernel buffers into multiple user destinations.
///
/// Multi-destination write transaction: every destination is probed and staged
/// (under the domain reader pin for Domain views, retaining all pins across the
/// whole transaction, or probed against the active root for Sas views) BEFORE
/// any byte is committed to user memory. If any destination fails validation or
/// staging, NO user buffer is touched.
pub(crate) fn copy_to_user_scatter(
    view: &CopyView,
    writes: &[(UserWriteSlice, &[u8])],
) -> Result<(), CopyError> {
    for (dst, src) in writes {
        if dst.len() > src.len() {
            return Err(CopyError::InvalidAddress);
        }
    }
    match view {
        CopyView::Domain(arc) => {
            if !arc.is_live() {
                return Err(CopyError::InvalidAddress);
            }
            // 1. Stage and pin EVERY destination range. Retaining `staged` keeps
            // all CopyReader pins held so no concurrent revocation can tear down
            // mappings while we probe remaining ranges or commit bytes.
            let mut staged = Vec::new();
            if staged.try_reserve_exact(writes.len()).is_err() {
                return Err(CopyError::InvalidAddress);
            }
            for (dst, _) in writes {
                if dst.len() > 0 {
                    let pin = stage_domain(arc, Direction::ToUser, dst.ptr(), dst.len())?;
                    staged.push(pin);
                }
            }
            // 2. Commit every staged destination. Every range was validated and
            // is pinned against revocation.
            let mut pin_iter = staged.into_iter();
            for (dst, src) in writes {
                if dst.len() > 0 {
                    if let Some(pin) = pin_iter.next() {
                        pin.commit(src.as_ptr().cast_mut())?;
                    }
                }
            }
            Ok(())
        }
        CopyView::Sas => {
            // 1. Probe every destination range before committing any.
            for (dst, _) in writes {
                if dst.len() > 0 {
                    probe_sas(dst.ptr(), dst.len(), Direction::ToUser)?;
                }
            }
            // 2. Commit all writes.
            for (dst, src) in writes {
                if dst.len() > 0 {
                    commit_sas(
                        Direction::ToUser,
                        dst.ptr(),
                        src.as_ptr().cast_mut(),
                        dst.len(),
                    )?;
                }
            }
            Ok(())
        }
    }
}
