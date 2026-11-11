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

/// Handler for `Syscall::CreateVm` (extracted from the dispatcher).
pub(super) fn create_vm(caller_id: usize, guest_pages: usize) -> SyscallResult {
    if !caller_has_hypervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    // An empty carve disables the nested table's SAS check, and the
    // window's byte size must be representable for every later bound.
    if guest_pages == 0
        || guest_pages
            .checked_mul(crate::memory::paging::PAGE_SIZE)
            .is_none()
    {
        return Err(SyscallError::InvalidInput);
    }
    crate::hypervisor::registry::create_vm(caller_id, guest_pages)
        .map_err(|_| SyscallError::NotSupported)
}

/// Handler for `Syscall::CreateVcpu` (extracted from the dispatcher).
pub(super) fn create_vcpu(caller_id: usize, vm_id: usize, entry_pc: u64) -> SyscallResult {
    if !caller_has_hypervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    crate::hypervisor::registry::create_vcpu(caller_id, vm_id, entry_pc)
        .map_err(|_| SyscallError::NotSupported)
}

/// Handler for `Syscall::MapGuestMemory` (extracted from the dispatcher).
pub(super) fn map_guest_memory(
    caller_id: usize,
    vm_id: usize,
    ipa: u64,
    size: usize,
    writable: bool,
) -> SyscallResult {
    if !caller_has_hypervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    // C3: overflow guard on IPA + size.
    ipa.checked_add(size as u64)
        .ok_or(SyscallError::InvalidInput)?;
    // C-2: the range is backed from the start of the guest carve, so
    // it may never be larger than the carve (the table re-checks).
    let (_, guest_bytes) = crate::hypervisor::registry::guest_window(caller_id, vm_id)
        .map_err(|_| SyscallError::NotSupported)?;
    if size == 0 || size > guest_bytes {
        return Err(SyscallError::InvalidInput);
    }
    crate::hypervisor::registry::map_guest_memory(caller_id, vm_id, ipa, size, writable)
        .map(|_| 0usize)
        .map_err(|_| SyscallError::NotSupported)
}

/// Handler for `Syscall::RunVcpu` (extracted from the dispatcher).
pub(super) fn run_vcpu(
    caller_id: usize,
    vm_id: usize,
    vcpu_id: usize,
    budget_ns: u64,
    out_ptr: usize,
) -> SyscallResult {
    if !caller_has_hypervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    use crate::hypervisor::registry::vmm_wire;
    validate_user_buf(out_ptr, vmm_wire::EXIT_LEN, MAX_USER_BUF)?;
    let view = caller_copy_view(caller_id)?;
    // Prove the record is writable before the guest runs: an exit
    // that cannot be delivered would be lost with the guest advanced.
    view.write_bytes(out_ptr, &[0u8; vmm_wire::EXIT_LEN])
        .map_err(|_| SyscallError::InvalidInput)?;
    let exit = crate::hypervisor::registry::run_vcpu(caller_id, vm_id, vcpu_id, budget_ns)
        .map_err(|_| SyscallError::NotSupported)?;
    view.write_bytes(out_ptr, &vmm_wire::encode_exit(&exit))
        .map_err(|_| SyscallError::InvalidInput)?;
    Ok(0)
}

/// Handler for `Syscall::VcpuRegs` (extracted from the dispatcher).
pub(super) fn vcpu_regs(
    caller_id: usize,
    vm_id: usize,
    vcpu_id: usize,
    buf_ptr: usize,
    mode: usize,
) -> SyscallResult {
    if !caller_has_hypervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    // All modes access the full 32-word (256-byte) user buffer.
    // Mode 2 is Pi-only, read-only, and clears unused words.
    #[cfg(target_arch = "aarch64")]
    let max_mode =
        <hal::aarch64::vcpu::VcpuExt as hal::aarch64::backend::ArmVcpuExt>::REGS_MAX_MODE;
    #[cfg(not(target_arch = "aarch64"))]
    let max_mode = 1;
    if mode > max_mode {
        return Err(SyscallError::InvalidInput);
    }
    use crate::hypervisor::registry::vmm_wire;
    validate_user_buf(buf_ptr, vmm_wire::REGS_LEN, MAX_USER_BUF)?;
    let view = caller_copy_view(caller_id)?;
    // Read the buffer in every mode: mode 1 consumes it, and the read
    // modes leave words the backend does not model as the cell had them.
    let mut bytes = [0u8; vmm_wire::REGS_LEN];
    view.read_into(buf_ptr, &mut bytes)
        .map_err(|_| SyscallError::InvalidInput)?;
    let mut regs = vmm_wire::regs_from_bytes(&bytes);
    let result = crate::hypervisor::registry::vcpu_regs(caller_id, vm_id, vcpu_id, &mut regs, mode)
        .map_err(|_| SyscallError::NotSupported)?;
    if mode != 1 {
        view.write_bytes(buf_ptr, &vmm_wire::regs_to_bytes(&regs))
            .map_err(|_| SyscallError::InvalidInput)?;
    }
    Ok(result)
}

/// Handler for `Syscall::InjectIrq` (extracted from the dispatcher).
pub(super) fn inject_irq(
    caller_id: usize,
    vm_id: usize,
    vcpu_id: usize,
    intid: u32,
) -> SyscallResult {
    if !caller_has_hypervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    // m3: GICv2 has SPIs/PPIs/SGIs up to intid 1019.
    if intid > 1019 {
        return Err(SyscallError::InvalidInput);
    }
    crate::hypervisor::registry::inject_irq(caller_id, vm_id, vcpu_id, intid)
        .map_err(|_| SyscallError::NotSupported)
}

/// Handler for `Syscall::WriteGuestMemory` (extracted from the dispatcher).
pub(super) fn write_guest_memory(
    caller_id: usize,
    vm_id: usize,
    gpa: u64,
    src_ptr: usize,
    len: usize,
) -> SyscallResult {
    if !caller_has_hypervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    validate_user_buf(src_ptr, len, MAX_USER_BUF)?;
    check_guest_range(caller_id, vm_id, gpa, len)?;
    let view = caller_copy_view(caller_id)?;
    let mut staging = guest_copy_staging(len)?;
    let mut done = 0;
    while done < len {
        let chunk = &mut staging[..(len - done).min(GUEST_COPY_CHUNK)];
        view.read_into(src_ptr + done, chunk)
            .map_err(|_| SyscallError::InvalidInput)?;
        crate::hypervisor::registry::write_guest_memory(caller_id, vm_id, gpa + done as u64, chunk)
            .map_err(|_| SyscallError::InvalidInput)?;
        done += chunk.len();
    }
    Ok(len)
}

/// Handler for `Syscall::ReadGuestMemory` (extracted from the dispatcher).
pub(super) fn read_guest_memory(
    caller_id: usize,
    vm_id: usize,
    gpa: u64,
    dst_ptr: usize,
    len: usize,
) -> SyscallResult {
    if !caller_has_hypervisor(caller_id) {
        return Err(SyscallError::PermissionDenied);
    }
    validate_user_buf(dst_ptr, len, MAX_USER_BUF)?;
    check_guest_range(caller_id, vm_id, gpa, len)?;
    let view = caller_copy_view(caller_id)?;
    let mut staging = guest_copy_staging(len)?;
    let mut done = 0;
    while done < len {
        let chunk = &mut staging[..(len - done).min(GUEST_COPY_CHUNK)];
        crate::hypervisor::registry::read_guest_memory(caller_id, vm_id, gpa + done as u64, chunk)
            .map_err(|_| SyscallError::InvalidInput)?;
        view.write_bytes(dst_ptr + done, chunk)
            .map_err(|_| SyscallError::InvalidInput)?;
        done += chunk.len();
    }
    Ok(len)
}
