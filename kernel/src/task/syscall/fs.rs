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
pub(in crate::task) fn validate_user_buf(
    ptr: usize,
    len: usize,
    max: usize,
) -> Result<(), SyscallError> {
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

/// Handler for `Syscall::PipeCreate` (extracted from the dispatcher).
pub(super) fn pipe_create(caller_id: usize, capacity: usize, out_ptr: usize) -> SyscallResult {
    // The ring is charged to the creating cell, so a pipe is not a way to
    // hold memory outside the per-cell quota.
    let owner_cell = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
        .map(|task| task.cell_id.0 as usize)
        .ok_or(SyscallError::PermissionDenied)?;
    let (read_handle, write_handle) = crate::ipc::pipe::create(capacity, owner_cell)?;
    let mut out = [0u8; 16];
    let mut offset = 0usize;
    for word in [read_handle.to_raw(), write_handle.to_raw()] {
        let bytes = word.to_ne_bytes();
        out[offset..offset + bytes.len()].copy_from_slice(&bytes);
        offset += bytes.len();
    }
    if let Err(error) = write_user_slice(caller_id, out_ptr, &out[..offset], 64) {
        crate::ipc::pipe::close_end(read_handle, caller_id);
        crate::ipc::pipe::close_end(write_handle, caller_id);
        return Err(error);
    }
    let installed = crate::task::SCHEDULER
        .lock()
        .as_mut()
        .and_then(|sched| sched.tasks.get_mut(&caller_id))
        .map(|task| {
            #[cfg(cellos_p01_trace)]
            let _semantic = crate::memory::heap::p01_owner::creation(
                Some(crate::p01_resources::owner(task)),
                crate::memory::heap::p01_owner::TASK_FIELDS,
            );
            task.pipe_handles.insert(read_handle);
            task.pipe_handles.insert(write_handle);
        })
        .is_some();
    if !installed {
        crate::ipc::pipe::close_end(read_handle, caller_id);
        crate::ipc::pipe::close_end(write_handle, caller_id);
        return Err(SyscallError::PermissionDenied);
    }
    Ok(0)
}

/// Handler for `Syscall::PipeRead` (extracted from the dispatcher).
pub(super) fn pipe_read(
    caller_id: usize,
    handle: usize,
    buf_ptr: usize,
    len: usize,
    timeout_ticks: u64,
) -> SyscallResult {
    const CHUNK: usize = 512;
    let Some(handle) = crate::ipc::pipe::PipeHandle::from_raw(handle) else {
        return Err(SyscallError::InvalidInput);
    };
    if handle.end != crate::ipc::pipe::PipeEnd::Read || !caller_owns_pipe(caller_id, handle) {
        audit_syscall_denied(caller_id, handle.to_raw() as u32);
        return Err(SyscallError::PermissionDenied);
    }
    if len == 0 {
        return Ok(0);
    }
    let deadline = if timeout_ticks == 0 {
        None
    } else {
        Some(crate::task::system_ticks() as u64 + timeout_ticks)
    };
    let count = len.min(CHUNK);
    let mut scratch = [0u8; CHUNK];
    loop {
        match crate::ipc::pipe::try_read(handle, &mut scratch[..count], caller_id) {
            Some(crate::ipc::pipe::ReadOutcome::Read(read)) => {
                write_user_slice(caller_id, buf_ptr, &scratch[..read], MAX_USER_BUF)?;
                return Ok(read);
            }
            // EOF: the ring is empty and no writer end remains.
            Some(crate::ipc::pipe::ReadOutcome::Eof) => return Ok(0),
            Some(crate::ipc::pipe::ReadOutcome::Empty) => {
                if deadline.is_some_and(|d| crate::task::system_ticks() as u64 >= d) {
                    return Ok(0);
                }
                // Enqueue and park under one lock: a writer that drains the
                // queue takes `SCHEDULER` to wake us, so either it sees our
                // entry and wakes a parked task, or we see its bytes on the
                // next `try_read`. Lock order stays SCHEDULER -> PIPES.
                if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
                    match sched.tasks.get_mut(&caller_id) {
                        Some(task) => {
                            crate::ipc::pipe::park_reader(handle, caller_id);
                            task.state = crate::task::tcb::TaskState::PipeRead { handle, deadline };
                        }
                        None => return Err(SyscallError::PermissionDenied),
                    }
                }
                crate::task::yield_cpu();
            }
            None => return Err(SyscallError::InvalidInput),
        }
    }
}

/// Handler for `Syscall::PipeWrite` (extracted from the dispatcher).
pub(super) fn pipe_write(
    caller_id: usize,
    handle: usize,
    buf_ptr: usize,
    len: usize,
    timeout_ticks: u64,
) -> SyscallResult {
    const CHUNK: usize = 512;
    let Some(handle) = crate::ipc::pipe::PipeHandle::from_raw(handle) else {
        return Err(SyscallError::InvalidInput);
    };
    if handle.end != crate::ipc::pipe::PipeEnd::Write || !caller_owns_pipe(caller_id, handle) {
        audit_syscall_denied(caller_id, handle.to_raw() as u32);
        return Err(SyscallError::PermissionDenied);
    }
    if len == 0 {
        // A zero-length write closes the writer end: readers then observe
        // EOF once they have drained the ring.
        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
            if let Some(task) = sched.tasks.get_mut(&caller_id) {
                task.pipe_handles.remove(&handle);
            }
        }
        crate::ipc::pipe::close_end(handle, caller_id);
        return Ok(0);
    }
    let deadline = if timeout_ticks == 0 {
        None
    } else {
        Some(crate::task::system_ticks() as u64 + timeout_ticks)
    };
    let count = len.min(CHUNK);
    let mut scratch = [0u8; CHUNK];
    {
        let view = caller_copy_view_for(caller_id)?;
        view.read_into(buf_ptr, &mut scratch[..count])
            .map_err(|_| SyscallError::InvalidInput)?;
    }
    loop {
        match crate::ipc::pipe::try_write(handle, &scratch[..count], caller_id) {
            Some(crate::ipc::pipe::WriteOutcome::Wrote(written)) => return Ok(written),
            // No reader end remains: the stream is broken, not full.
            Some(crate::ipc::pipe::WriteOutcome::NoReader) => {
                return Err(SyscallError::BrokenPipe);
            }
            Some(crate::ipc::pipe::WriteOutcome::Full) => {
                if deadline.is_some_and(|d| crate::task::system_ticks() as u64 >= d) {
                    return Ok(0);
                }
                if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
                    match sched.tasks.get_mut(&caller_id) {
                        Some(task) => {
                            crate::ipc::pipe::park_writer(handle, caller_id);
                            task.state =
                                crate::task::tcb::TaskState::PipeWrite { handle, deadline };
                        }
                        None => return Err(SyscallError::PermissionDenied),
                    }
                }
                crate::task::yield_cpu();
            }
            None => return Err(SyscallError::InvalidInput),
        }
    }
}

/// Handler for `Syscall::PipeClose` (extracted from the dispatcher).
pub(super) fn pipe_close(caller_id: usize, handle: usize) -> SyscallResult {
    let Some(handle) = crate::ipc::pipe::PipeHandle::from_raw(handle) else {
        return Err(SyscallError::InvalidInput);
    };
    let owned = crate::task::SCHEDULER
        .lock()
        .as_mut()
        .and_then(|sched| sched.tasks.get_mut(&caller_id))
        .map(|task| task.pipe_handles.remove(&handle))
        .unwrap_or(false);
    if !owned {
        audit_syscall_denied(caller_id, handle.to_raw() as u32);
        return Err(SyscallError::PermissionDenied);
    }
    crate::ipc::pipe::close_end(handle, caller_id);
    Ok(0)
}

/// Handler for `Syscall::PipeShare` (extracted from the dispatcher).
pub(super) fn pipe_share(caller_id: usize, handle: usize, target_tid: usize) -> SyscallResult {
    let Some(handle) = crate::ipc::pipe::PipeHandle::from_raw(handle) else {
        return Err(SyscallError::InvalidInput);
    };
    // Owner-only: a task cannot hand out an endpoint it does not hold. That
    // ownership rule is the whole authorization model for pipes. The
    // endpoint count and target table change together while SCHEDULER is
    // held, preserving the documented SCHEDULER -> PIPES lock order.
    let mut sched = crate::task::SCHEDULER.lock();
    let sched = sched.as_mut().ok_or(SyscallError::Unknown)?;
    let owned = sched
        .tasks
        .get(&caller_id)
        .is_some_and(|task| task.pipe_handles.contains(&handle));
    if !owned {
        crate::audit::log_event(
            crate::audit::AuditEvent::SyscallDenied,
            &crate::audit::encode_subject(
                crate::audit::Subject::of(sched, caller_id),
                handle.to_raw() as u32,
                0,
            ),
        );
        return Err(SyscallError::PermissionDenied);
    }
    let already_granted = sched
        .tasks
        .get(&target_tid)
        .ok_or(SyscallError::InvalidInput)?
        .pipe_handles
        .contains(&handle);
    if already_granted {
        return Ok(0);
    }
    if !crate::ipc::pipe::duplicate_end(handle) {
        return Err(SyscallError::InvalidInput);
    }
    let target = sched
        .tasks
        .get_mut(&target_tid)
        .expect("target task remained live while holding SCHEDULER");
    #[cfg(cellos_p01_trace)]
    let _semantic = crate::memory::heap::p01_owner::creation(
        Some(crate::p01_resources::owner(target)),
        crate::memory::heap::p01_owner::TASK_FIELDS,
    );
    target.pipe_handles.insert(handle);
    Ok(0)
}

/// Handler for `Syscall::Open` (extracted from the dispatcher).
pub(super) fn open(caller_id: usize, path_ptr: usize, path_len: usize) -> SyscallResult {
    let path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
    if let Ok(fd) = crate::fsapi::file_open(caller_id, &path) {
        return Ok(fd);
    }
    Err(SyscallError::FileNotFound)
}

/// Handler for `Syscall::Read` (extracted from the dispatcher).
pub(super) fn read(caller_id: usize, fd: usize, buf_ptr: usize, buf_len: usize) -> SyscallResult {
    validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
    let mut kbuf = alloc::vec::Vec::new();
    kbuf.try_reserve_exact(buf_len)
        .map_err(|_| SyscallError::OutOfMemory)?;
    kbuf.resize(buf_len, 0);
    let read_bytes = crate::fsapi::file_read(fd, &mut kbuf);
    // A committed child's endpoint failure is explicit, never a short count.
    if read_bytes == usize::MAX {
        return Err(SyscallError::PermissionDenied);
    }
    if read_bytes > 0 {
        write_user_slice(caller_id, buf_ptr, &kbuf[..read_bytes], MAX_USER_BUF)?;
    }
    Ok(read_bytes)
}

/// Handler for `Syscall::Close` (extracted from the dispatcher).
pub(super) fn close(fd: usize) -> SyscallResult {
    crate::fsapi::file_close(fd);
    Ok(0)
}

/// Handler for `Syscall::ReadDir` (extracted from the dispatcher).
pub(super) fn read_dir(
    caller_id: usize,
    fd: usize,
    buf_ptr: usize,
    buf_len: usize,
) -> SyscallResult {
    validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
    let mut kbuf = alloc::vec::Vec::new();
    kbuf.try_reserve_exact(buf_len)
        .map_err(|_| SyscallError::OutOfMemory)?;
    kbuf.resize(buf_len, 0);
    let read_bytes =
        crate::fsapi::file_readdir(fd, &mut kbuf).map_err(|_| SyscallError::Unknown)?;
    if read_bytes > 0 {
        write_user_slice(caller_id, buf_ptr, &kbuf[..read_bytes], MAX_USER_BUF)?;
    }
    Ok(read_bytes)
}

/// Handler for `Syscall::Fstat` (extracted from the dispatcher).
pub(super) fn fstat(caller_id: usize, fd: usize, out_ptr: usize, out_len: usize) -> SyscallResult {
    let len = api::syscall::VI_FSTAT_V1_LEN;
    if out_len < len {
        return Err(SyscallError::BufferTooSmall);
    }
    validate_user_buf(out_ptr, len, MAX_USER_BUF)?;
    preflight_user_output(caller_id, out_ptr, len)?;

    let metadata = crate::fsapi::file_fstat(caller_id, fd).map_err(|_| SyscallError::Unknown)?;
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &metadata as *const api::syscall::ViFstatV1 as *const u8,
            len,
        )
    };
    write_user_slice(caller_id, out_ptr, bytes, MAX_USER_BUF)?;
    Ok(len)
}

/// Handler for `Syscall::Rename` (extracted from the dispatcher).
pub(super) fn rename(
    caller_id: usize,
    old_ptr: usize,
    old_len: usize,
    new_ptr: usize,
    new_len: usize,
) -> SyscallResult {
    let old_path = read_user_string(caller_id, old_ptr, old_len, MAX_LOG_MSG)?;
    let new_path = read_user_string(caller_id, new_ptr, new_len, MAX_LOG_MSG)?;
    let vfs_tid = crate::cell::service_registry::lookup(api::syscall::service::VFS)
        .ok_or(SyscallError::InvalidCommand)?;
    let request = api::ipc::VfsRequest::Rename {
        old: &old_path,
        new: &new_path,
    };
    let mut request_buf = [0u8; api::ipc::IPC_BUF_SIZE];
    let encoded =
        api::ipc::encode(&request, &mut request_buf).map_err(|_| SyscallError::InvalidCommand)?;
    match crate::task::ipc_send_kernel(caller_id, vfs_tid, encoded) {
        Ok(0) => {}
        Ok(1) => crate::task::yield_cpu(),
        Err(crate::task::IpcSendError::Backpressure) => return Err(SyscallError::TryAgain),
        Ok(_) => return Err(SyscallError::InvalidCommand),
        Err(crate::task::IpcSendError::TargetGone) => return Err(SyscallError::InvalidCommand),
    }

    let mut response_buf = [0u8; api::ipc::IPC_BUF_SIZE];
    loop {
        let response = crate::task::ipc_recv_kernel(caller_id, vfs_tid, &mut response_buf)
            .map_err(|_| SyscallError::InvalidCommand)?;
        match response {
            Some((_, len)) => {
                return match api::ipc::decode::<api::ipc::VfsResponse>(&response_buf[..len]) {
                    Ok(api::ipc::VfsResponse::Ok) => Ok(0),
                    _ => Err(SyscallError::PermissionDenied),
                };
            }
            None => crate::task::yield_cpu(),
        }
    }
}

/// Handler for `Syscall::ChDir` (extracted from the dispatcher).
pub(super) fn ch_dir(caller_id: usize, path_ptr: usize, path_len: usize) -> SyscallResult {
    let path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
    if crate::fsapi::file_chdir(caller_id, &path).is_ok() {
        return Ok(0);
    }
    Err(SyscallError::FileNotFound)
}

/// Handler for `Syscall::GetCwd` (extracted from the dispatcher).
pub(super) fn get_cwd(caller_id: usize, buf_ptr: usize, buf_len: usize) -> SyscallResult {
    validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
    let mut kbuf = alloc::vec::Vec::new();
    kbuf.try_reserve_exact(buf_len)
        .map_err(|_| SyscallError::OutOfMemory)?;
    kbuf.resize(buf_len, 0);
    if let Ok(len) = crate::fsapi::file_getcwd(caller_id, &mut kbuf) {
        write_user_slice(caller_id, buf_ptr, &kbuf[..len], MAX_USER_BUF)?;
        return Ok(len);
    }
    Err(SyscallError::BufferTooSmall)
}

/// Handler for `Syscall::Write` (extracted from the dispatcher).
pub(super) fn write(caller_id: usize, fd: usize, buf_ptr: usize, buf_len: usize) -> SyscallResult {
    let bytes = read_user_slice(caller_id, buf_ptr, buf_len, MAX_USER_BUF)?;
    let written = crate::fsapi::file_write(fd, &bytes);
    if written == usize::MAX {
        return Err(SyscallError::PermissionDenied);
    }
    Ok(written)
}

/// Handler for `Syscall::MkDir` (extracted from the dispatcher).
pub(super) fn mk_dir(caller_id: usize, path_ptr: usize, path_len: usize) -> SyscallResult {
    let _path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
    Err(SyscallError::PermissionDenied)
}

/// Handler for `Syscall::QueryDirHandles` (extracted from the dispatcher).
pub(super) fn query_dir_handles(
    caller_id: usize,
    cell_id: u64,
    buf_ptr: usize,
    buf_len: usize,
) -> SyscallResult {
    if !may_query_dir_handles(caller_id, cell_id) {
        return Err(SyscallError::PermissionDenied);
    }
    let len = api::dir_attestation::DIR_ATTESTATION_LEN;
    if buf_len < len {
        return Err(SyscallError::BufferTooSmall);
    }
    let record = crate::task::dir_inherit::attestation_for(cell_id)
        .ok_or(SyscallError::InvalidInput)?
        .to_bytes();
    write_user_slice(caller_id, buf_ptr, &record, MAX_USER_BUF)?;
    Ok(len)
}

/// Handler for `Syscall::OpenCap` (extracted from the dispatcher).
pub(super) fn open_cap(caller_id: usize, path_ptr: usize, path_len: usize) -> SyscallResult {
    let path_str = read_user_string(caller_id, path_ptr, path_len, 256)?;
    use crate::fs::VIFS1;
    let file = {
        let mut guard = VIFS1.lock();
        guard
            .as_mut()
            .ok_or(SyscallError::FileNotFound)?
            .open(&path_str, api::fs::OpenMode::Read)
            .map_err(|_| SyscallError::FileNotFound)?
    };
    let (cell_id, cell_gen) = if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        sched
            .tasks
            .get(&caller_id)
            .map(|t| (t.cell_id, t.cell_generation))
            .unwrap_or((types::CellId(0), 0))
    } else {
        (types::CellId(0), 0)
    };
    let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
    let auth = auths
        .get_mut(&(cell_id.0, cell_gen))
        .ok_or(SyscallError::PermissionDenied)?;
    let ticket = auth
        .allocate_file_slot(file, api::cap::CapPerms::FILE_READ.0)
        .map_err(|_| SyscallError::OutOfMemory)?;
    Ok(ticket as usize)
}

/// Handler for `Syscall::ReadCap` (extracted from the dispatcher).
pub(super) fn read_cap(
    caller_id: usize,
    cap_id: usize,
    buf_ptr: usize,
    buf_len: usize,
) -> SyscallResult {
    if buf_len == 0 {
        return Ok(0);
    }
    validate_user_buf(buf_ptr, buf_len, MAX_USER_BUF)?;
    let (cell_id, cell_gen) = if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        sched
            .tasks
            .get(&caller_id)
            .map(|t| (t.cell_id, t.cell_generation))
            .unwrap_or((types::CellId(0), 0))
    } else {
        (types::CellId(0), 0)
    };
    let mut boxed_file = {
        let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
        let auth = auths
            .get_mut(&(cell_id.0, cell_gen))
            .ok_or(SyscallError::PermissionDenied)?;
        auth.park_file(cap_id as u64, api::cap::CapPerms::READ.0)
            .map_err(|_| SyscallError::PermissionDenied)?
    };
    let mut kbuf = alloc::vec::Vec::new();
    kbuf.try_reserve_exact(buf_len)
        .map_err(|_| SyscallError::OutOfMemory)?;
    kbuf.resize(buf_len, 0);
    let read_result = boxed_file.read(&mut kbuf);
    if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
        .lock()
        .get_mut(&(cell_id.0, cell_gen))
    {
        auth.unpark_file(cap_id as u64, boxed_file);
    }
    match read_result {
        Ok(n) => {
            if n > 0 {
                write_user_slice(caller_id, buf_ptr, &kbuf[..n], MAX_USER_BUF)?;
            }
            Ok(n)
        }
        Err(_) => Err(SyscallError::Unknown),
    }
}

/// Handler for `Syscall::SeekCap` (extracted from the dispatcher).
pub(super) fn seek_cap(
    caller_id: usize,
    cap_id: usize,
    offset: usize,
    whence: usize,
) -> SyscallResult {
    let pos = match whence {
        0 => api::fs::SeekFrom::Start(offset as u64),
        1 => api::fs::SeekFrom::Current(offset as isize as i64),
        2 => api::fs::SeekFrom::End(offset as isize as i64),
        _ => return Err(SyscallError::InvalidInput),
    };
    let (cell_id, cell_gen) = if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        sched
            .tasks
            .get(&caller_id)
            .map(|t| (t.cell_id, t.cell_generation))
            .unwrap_or((types::CellId(0), 0))
    } else {
        (types::CellId(0), 0)
    };
    let mut boxed_file = {
        let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
        let auth = auths
            .get_mut(&(cell_id.0, cell_gen))
            .ok_or(SyscallError::PermissionDenied)?;
        auth.park_file(cap_id as u64, api::cap::CapPerms::READ.0)
            .map_err(|_| SyscallError::PermissionDenied)?
    };
    let seek_result = boxed_file.seek(pos);
    if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
        .lock()
        .get_mut(&(cell_id.0, cell_gen))
    {
        auth.unpark_file(cap_id as u64, boxed_file);
    }
    match seek_result {
        Ok(new_pos) => Ok(new_pos as usize),
        Err(_) => Err(SyscallError::Unknown),
    }
}

/// Handler for `Syscall::WriteCap` (extracted from the dispatcher).
pub(super) fn write_cap(
    caller_id: usize,
    cap_id: usize,
    buf_ptr: usize,
    buf_len: usize,
) -> SyscallResult {
    if buf_len == 0 {
        return Ok(0);
    }
    let bytes = read_user_slice(caller_id, buf_ptr, buf_len, MAX_USER_BUF)?;
    let (cell_id, cell_gen) = if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        sched
            .tasks
            .get(&caller_id)
            .map(|t| (t.cell_id, t.cell_generation))
            .unwrap_or((types::CellId(0), 0))
    } else {
        (types::CellId(0), 0)
    };
    let mut boxed_file = {
        let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
        let auth = auths
            .get_mut(&(cell_id.0, cell_gen))
            .ok_or(SyscallError::PermissionDenied)?;
        auth.park_file(cap_id as u64, api::cap::CapPerms::WRITE.0)
            .map_err(|_| SyscallError::PermissionDenied)?
    };
    let write_result = boxed_file.write(&bytes);
    if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
        .lock()
        .get_mut(&(cell_id.0, cell_gen))
    {
        auth.unpark_file(cap_id as u64, boxed_file);
    }
    match write_result {
        Ok(n) => Ok(n),
        Err(_) => Err(SyscallError::Unknown),
    }
}

/// Handler for `Syscall::StatCap` (extracted from the dispatcher).
pub(super) fn stat_cap(caller_id: usize, cap_id: usize) -> SyscallResult {
    let (cell_id, cell_gen) = if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        sched
            .tasks
            .get(&caller_id)
            .map(|t| (t.cell_id, t.cell_generation))
            .unwrap_or((types::CellId(0), 0))
    } else {
        (types::CellId(0), 0)
    };
    let mut boxed_file = {
        let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
        let auth = auths
            .get_mut(&(cell_id.0, cell_gen))
            .ok_or(SyscallError::PermissionDenied)?;
        auth.park_file(cap_id as u64, api::cap::CapPerms::READ.0)
            .map_err(|_| SyscallError::PermissionDenied)?
    };
    let result = boxed_file.size();
    if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
        .lock()
        .get_mut(&(cell_id.0, cell_gen))
    {
        auth.unpark_file(cap_id as u64, boxed_file);
    }
    result
        .map(|s| s as usize)
        .map_err(|_| SyscallError::Unknown)
}

/// Handler for `Syscall::TruncateCap` (extracted from the dispatcher).
pub(super) fn truncate_cap(caller_id: usize, cap_id: usize, len: usize) -> SyscallResult {
    let (cell_id, cell_gen) = if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        sched
            .tasks
            .get(&caller_id)
            .map(|t| (t.cell_id, t.cell_generation))
            .unwrap_or((types::CellId(0), 0))
    } else {
        (types::CellId(0), 0)
    };
    let mut boxed_file = {
        let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
        let auth = auths
            .get_mut(&(cell_id.0, cell_gen))
            .ok_or(SyscallError::PermissionDenied)?;
        auth.park_file(cap_id as u64, api::cap::CapPerms::WRITE.0)
            .map_err(|_| SyscallError::PermissionDenied)?
    };
    let result = boxed_file.truncate(len as u64);
    if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
        .lock()
        .get_mut(&(cell_id.0, cell_gen))
    {
        auth.unpark_file(cap_id as u64, boxed_file);
    }
    result.map(|_| 0usize).map_err(|_| SyscallError::Unknown)
}

/// Handler for `Syscall::SyncCap` (extracted from the dispatcher).
pub(super) fn sync_cap(caller_id: usize, cap_id: usize) -> SyscallResult {
    let (cell_id, cell_gen) = if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        sched
            .tasks
            .get(&caller_id)
            .map(|t| (t.cell_id, t.cell_generation))
            .unwrap_or((types::CellId(0), 0))
    } else {
        (types::CellId(0), 0)
    };
    let mut boxed_file = {
        let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
        let auth = auths
            .get_mut(&(cell_id.0, cell_gen))
            .ok_or(SyscallError::PermissionDenied)?;
        auth.park_file(cap_id as u64, api::cap::CapPerms::WRITE.0)
            .map_err(|_| SyscallError::PermissionDenied)?
    };
    let result = boxed_file.sync();
    if let Some(auth) = crate::cell::cspace::CELL_AUTHORITIES
        .lock()
        .get_mut(&(cell_id.0, cell_gen))
    {
        auth.unpark_file(cap_id as u64, boxed_file);
    }
    result.map(|_| 0usize).map_err(|_| SyscallError::Unknown)
}

/// Handler for `Syscall::CloseCap` (extracted from the dispatcher).
pub(super) fn close_cap(caller_id: usize, cap_id: usize) -> SyscallResult {
    let (cell_id, cell_gen) = if let Some(sched) = crate::task::SCHEDULER.lock().as_ref() {
        sched
            .tasks
            .get(&caller_id)
            .map(|t| (t.cell_id, t.cell_generation))
            .unwrap_or((types::CellId(0), 0))
    } else {
        (types::CellId(0), 0)
    };
    let mut auths = crate::cell::cspace::CELL_AUTHORITIES.lock();
    let auth = auths
        .get_mut(&(cell_id.0, cell_gen))
        .ok_or(SyscallError::PermissionDenied)?;
    auth.close_ticket(cap_id as u64)
        .map_err(|_| SyscallError::PermissionDenied)?;
    Ok(0)
}

/// Handler for `Syscall::Create` (extracted from the dispatcher).
pub(super) fn create(caller_id: usize, path_ptr: usize, path_len: usize) -> SyscallResult {
    let _path = read_user_string(caller_id, path_ptr, path_len, MAX_LOG_MSG)?;
    Err(SyscallError::PermissionDenied)
}

/// Handler for `Syscall::Seek` (extracted from the dispatcher).
pub(super) fn seek(fd: usize, offset: isize, whence: usize) -> SyscallResult {
    crate::fsapi::file_seek(fd, offset, whence).map_err(|_| SyscallError::Unknown)
}

/// Handler for `Syscall::FileOp` (extracted from the dispatcher).
pub(super) fn file_op(caller_id: usize, op: usize, arg1: usize, arg2: usize) -> SyscallResult {
    match op {
        0 => {
            let path = read_user_string(caller_id, arg1, arg2, MAX_LOG_MSG)?;
            crate::fsapi::file_remove(caller_id, &path).map_err(|_| SyscallError::PermissionDenied)
        }
        1 => {
            // Rename - Stub
            Err(SyscallError::NotSupported)
        }
        _ => Err(SyscallError::InvalidCommand),
    }
}
