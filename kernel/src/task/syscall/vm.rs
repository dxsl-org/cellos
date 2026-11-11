// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: vm (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, dev::*, fs::*, grant::*, ipc::*, misc::*, spawn::*};

pub(super) fn caller_has_hypervisor(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.hypervisor)
}

/// Staging granule for guest-memory copies. Cell bytes cross the copy view
/// into a kernel buffer of at most this size; the registry only ever sees that
/// buffer, never a cell address.
pub(super) const GUEST_COPY_CHUNK: usize = 256 * 1024;

/// A zeroed kernel staging buffer for a `len`-byte guest-memory copy.
pub(super) fn guest_copy_staging(len: usize) -> Result<Vec<u8>, SyscallError> {
    let size = len.min(GUEST_COPY_CHUNK);
    let mut staging = Vec::new();
    staging
        .try_reserve_exact(size)
        .map_err(|_| SyscallError::OutOfMemory)?;
    staging.resize(size, 0);
    Ok(staging)
}

/// Refuse a guest-memory request unless `[gpa, gpa + len)` lies wholly inside
/// the VM's guest-RAM window, so no chunk is copied for a request that would
/// fail part-way on bounds.
pub(super) fn check_guest_range(
    caller_id: usize,
    vm_id: usize,
    gpa: u64,
    len: usize,
) -> Result<(), SyscallError> {
    let (base, bytes) = crate::hypervisor::registry::guest_window(caller_id, vm_id)
        .map_err(|_| SyscallError::InvalidInput)?;
    let end = gpa
        .checked_sub(base)
        .and_then(|offset| offset.checked_add(len as u64))
        .ok_or(SyscallError::InvalidInput)?;
    if end > bytes as u64 {
        return Err(SyscallError::InvalidInput);
    }
    Ok(())
}
