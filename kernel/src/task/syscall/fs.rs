// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: fs (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, decode::*, dev::*, grant::*, ipc::*, misc::*, spawn::*, vm::*};

/// Maximum bytes a single syscall may read/write through a user buffer.
/// Bounds kernel work per syscall and acts as a coarse sanity check against
/// `len = usize::MAX` style attacks. 64 MiB is well above any legitimate
/// caller need today; tighten further for specific syscalls (see MAX_LOG_MSG).
pub(super) const MAX_USER_BUF: usize = 64 * 1024 * 1024;

pub(super) fn caller_has_block_io(caller_id: usize) -> bool {
    caller_has_cap(caller_id, |t| t.block_io)
}

/// Per-cell block-I/O range gate (Milestone 2.5 P03).
///
/// Replaces the old global `sector >= CELL_TABLE_BASE_LBA` check: the caller's
/// `block_regions` bitmask (from its manifest PART_* bits, or the legacy VFS
/// grant) defines which MBR partitions its raw block syscalls may address.
/// Deny-by-default: a sector outside every granted partition is rejected —
/// which structurally protects P2 (cell table) and P3 (snapshot), since no
/// bit exists for them. Logs every denial (silent denials cost us a day once).
pub(super) fn check_block_access(caller_id: usize, sector: u64, count: u64) -> bool {
    use crate::loader::disk_layout as dl;
    let regions = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .map_or(0, |s| s.cell_rights(caller_id).block_regions);
    let end = match sector.checked_add(count) {
        Some(e) => e,
        None => return false,
    };
    const GRANTABLE: [(u8, u64, u64); 4] = [
        (0b001, dl::PART_FAT32_BASE_LBA, dl::PART_FAT32_SECTORS), // P1 (PART_DATA)
        (0b010, dl::PART_LFS_BASE_LBA, dl::PART_LFS_SECTORS),     // P4 (PART_LFS)
        (0b100, dl::PART_SRV_BASE_LBA, dl::PART_SRV_SECTORS), // P5 (SRV, CellosFS Native, co-granted w/ LFS)
        (
            0b1000,
            dl::PART_CELLSTORE_BASE_LBA,
            dl::PART_CELLSTORE_SECTORS,
        ), // P6 (cell-store, read-only; VFS /bin overlay)
    ];
    for (bit, base, size) in GRANTABLE {
        if regions & bit != 0 && sector >= base && end <= base + size {
            return true;
        }
    }
    log::warn!(
        "[blk] sector {}..{} denied for tid {} (regions={:#04b})",
        sector,
        end,
        caller_id,
        regions
    );
    false
}

/// Validate a user-supplied (ptr, len) buffer descriptor.
///
/// Rejects: NULL pointer, zero-length when expected non-empty, lengths above
/// `max`, and pointer+length arithmetic overflow.
///
/// Does NOT walk the page table to confirm the U-bit. The trap handler enables
/// SUM only for the duration of `handle_syscall`, so a kernel-space `ptr`
/// supplied by user code will fault on access — but the fault is far more
/// graceful when we reject obvious garbage up front.
#[inline]
pub(crate) fn validate_user_buf(ptr: usize, len: usize, max: usize) -> Result<(), SyscallError> {
    if ptr == 0 {
        return Err(SyscallError::InvalidInput);
    }
    if len > max {
        return Err(SyscallError::BufferTooSmall);
    }
    if ptr.checked_add(len).is_none() {
        return Err(SyscallError::InvalidInput);
    }
    Ok(())
}

/// Derive the phase-03 recoverable copy view for a live caller task.
///
/// Acquires SCHEDULER, clones the view, and releases the lock. A missing task
/// is rejected so an unavailable task record cannot become SAS authority.
pub(super) fn caller_copy_view(caller_id: usize) -> Result<TaskCopyView, SyscallError> {
    TaskCopyView::for_task(caller_id).ok_or(SyscallError::InvalidInput)
}

/// Copy view for a caller, for kernel-internal users outside this module (the
/// futex word read). Same validation as every other user-pointer access.
pub(crate) fn caller_copy_view_for(caller_id: usize) -> Result<TaskCopyView, SyscallError> {
    caller_copy_view(caller_id)
}

/// The attested `CALLER_FLAG_VFS_MUTATE` bit: an explicit (non-permit-all)
/// allowlist declaring bit 63 AND the granted `vfs_mutate` right (annex
/// S2-T/S2-W §3). The declaration alone no longer attests anything, so a Cell
/// whose edge withholds W (the `/apps/`, `/data/bin/`, `/home/` sandbox) is
/// refused every classified VFS mutation whatever its ELF declares.
pub(super) fn vfs_mutate_caller_flag(allowlist: u64, vfs_mutate: bool) -> u32 {
    if vfs_mutate && allowlist != u64::MAX && (allowlist & (1u64 << 63)) != 0 {
        api::caller_identity::CALLER_FLAG_VFS_MUTATE
    } else {
        0
    }
}

/// End a VFS request abandoned at its public receive boundary. Entering that
/// receive proves the holder has stopped using the prior GrantSlice pointer.
pub(super) fn finish_vfs_context_drop(caller_id: usize, dropped: Option<(usize, u64)>) {
    let Some((grant_owner, request_generation)) = dropped else {
        return;
    };
    for range in crate::memory::pin::release_vfs_lease(caller_id, grant_owner, request_generation) {
        free_released_range(range);
    }
}

/// May `caller_id` read the kernel's provenance record for `target_cell`?
///
/// Two callers have a reason to: the filesystem service, which must decide
/// whether to bind the set, and a cell asking about itself. Anything else
/// enumerating another cell's grant is reconnaissance with no legitimate use.
///
/// The filesystem service is identified by the service registry rather than by
/// name, because a cell's name comes from a spawner-chosen path hint and is
/// therefore forgeable; registering a well-known service id is SpawnCap-gated
/// and is not.
///
/// Lock order SCHEDULER → REGISTRY, matching `Scheduler::reap`; REGISTRY is a
/// leaf and is never held across a scheduler acquisition.
pub(super) fn may_query_dir_handles(caller_id: usize, target_cell: u64) -> bool {
    let guard = crate::task::SCHEDULER.lock();
    let Some(sched) = guard.as_ref() else {
        return false;
    };
    let Some(caller_cell) = sched.tasks.get(&caller_id).map(|t| t.cell_id.0) else {
        return false;
    };
    if caller_cell == 0 {
        return false;
    }
    if caller_cell == target_cell {
        return true;
    }
    crate::cell::service_registry::lookup(api::syscall::service::VFS)
        .and_then(|tid| sched.tasks.get(&tid))
        .is_some_and(|vfs| vfs.cell_id.0 == caller_cell)
}

pub(super) fn read_user_string(
    caller_id: usize,
    ptr: usize,
    len: usize,
    max: usize,
) -> Result<alloc::string::String, SyscallError> {
    if len == 0 || len > max {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(ptr, len, max)?;
    let view = caller_copy_view(caller_id)?;
    let bytes = view
        .read_bytes(ptr, len)
        .map_err(|_| SyscallError::InvalidInput)?;
    alloc::string::String::from_utf8(bytes).map_err(|_| SyscallError::InvalidInput)
}

pub(super) fn read_user_slice(
    caller_id: usize,
    ptr: usize,
    len: usize,
    max: usize,
) -> Result<alloc::vec::Vec<u8>, SyscallError> {
    if len > max {
        return Err(SyscallError::BufferTooSmall);
    }
    validate_user_buf(ptr, len, max)?;
    let view = caller_copy_view(caller_id)?;
    view.read_bytes(ptr, len)
        .map_err(|_| SyscallError::InvalidInput)
}

pub(super) fn write_user_slice(
    caller_id: usize,
    ptr: usize,
    bytes: &[u8],
    max: usize,
) -> Result<(), SyscallError> {
    if bytes.len() > max {
        return Err(SyscallError::BufferTooSmall);
    }
    validate_user_buf(ptr, bytes.len(), max)?;
    let view = caller_copy_view(caller_id)?;
    view.write_bytes(ptr, bytes)
        .map_err(|_| SyscallError::InvalidInput)
}
