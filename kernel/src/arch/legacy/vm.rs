// SPDX-License-Identifier: MPL-2.0
//! `vm`: no hypervisor on this target; every `crate::hypervisor::registry`
//! operation returns `Err(ViError::NotSupported)` (`inject_irq` and the reaper
//! do nothing).

use types::{ViError, ViResult};

pub(crate) fn create_vm(owner: usize, guest_pages: usize) -> ViResult<usize> {
    let _ = (owner, guest_pages);
    Err(ViError::NotSupported)
}

pub(crate) fn create_vcpu(owner: usize, vm_id: usize, entry_pc: u64) -> ViResult<usize> {
    let _ = (owner, vm_id, entry_pc);
    Err(ViError::NotSupported)
}

pub(crate) fn map_guest_memory(
    owner: usize,
    vm_id: usize,
    ipa: u64,
    size: usize,
    writable: bool,
) -> ViResult<()> {
    let _ = (owner, vm_id, ipa, size, writable);
    Err(ViError::NotSupported)
}

pub(crate) fn guest_window(owner: usize, vm_id: usize) -> ViResult<(u64, usize)> {
    let _ = (owner, vm_id);
    Err(ViError::NotSupported)
}

pub(crate) fn run_vcpu(
    owner: usize,
    vm_id: usize,
    vcpu_id: usize,
    _budget_ns: u64,
) -> ViResult<abi::hypervisor::ViVmExit> {
    let _ = (owner, vm_id, vcpu_id, _budget_ns);
    Err(ViError::NotSupported)
}

pub(crate) fn vcpu_regs(
    owner: usize,
    vm_id: usize,
    vcpu_id: usize,
    regs: &mut [u64; 32],
    mode: usize,
) -> ViResult<usize> {
    let _ = (owner, vm_id, vcpu_id, regs, mode);
    Err(ViError::NotSupported)
}

pub(crate) fn write_guest_memory(
    owner: usize,
    vm_id: usize,
    gpa: u64,
    src: &[u8],
) -> ViResult<usize> {
    let _ = (owner, vm_id, gpa, src);
    Err(ViError::NotSupported)
}

pub(crate) fn read_guest_memory(
    owner: usize,
    vm_id: usize,
    gpa: u64,
    dst: &mut [u8],
) -> ViResult<usize> {
    let _ = (owner, vm_id, gpa, dst);
    Err(ViError::NotSupported)
}

pub(crate) fn inject_irq(
    owner: usize,
    vm_id: usize,
    vcpu_id: usize,
    intid: u32,
) -> ViResult<usize> {
    let _ = (owner, vm_id, vcpu_id, intid);
    Ok(0)
}

pub(crate) fn reap_vms_for_task(dead_tid: usize) {
    let _ = dead_tid;
}
