// SPDX-License-Identifier: MPL-2.0
//! `vm`: `crate::hypervisor::registry` on x86_64, backed by the SVM VM store
//! (`crate::arch::hypervisor::svm_registry`).

use crate::hypervisor::registry::VmCreateError;
use crate::memory::cell_quota::Charge;
use types::{ViError, ViResult};

pub(crate) fn create_vm(owner: Charge, guest_pages: usize) -> Result<usize, VmCreateError> {
    crate::arch::hypervisor::svm_registry::create_vm(owner, guest_pages)
}

/// The guest charge `create_vm` takes for a VM of `guest_pages`
/// (`svm_registry::guest_bytes`).
#[cfg(feature = "test-hooks")]
pub(crate) fn guest_bytes(guest_pages: usize) -> Option<usize> {
    crate::arch::hypervisor::svm_registry::guest_bytes(guest_pages)
}

/// VM tags (SVM ASIDs) not yet issued (`svm_registry::free_vm_tags`).
#[cfg(feature = "test-hooks")]
pub(crate) fn free_vm_tags() -> usize {
    crate::arch::hypervisor::svm_registry::free_vm_tags()
}

/// VM tags one cell incarnation may hold (the `TAG-POOL-OWNER-LIMIT`
/// fixture): SVM ASIDs, one per vCPU. `None` on VMX, whose VMs carry no tag
/// (no VPID), so no owner limit applies to them.
#[cfg(feature = "test-hooks")]
pub(crate) fn vm_tags_per_owner() -> Option<u32> {
    (crate::arch::boot::cpu_features::x86_virt_kind()
        == Some(crate::arch::boot::cpu_features::X86Virt::Svm))
    .then_some(crate::arch::hypervisor::svm_registry::SVM_ASIDS_PER_OWNER)
}

/// A VM takes its tag with each vCPU (`create_vcpu`), not at creation.
#[cfg(feature = "test-hooks")]
pub(crate) const VM_TAG_PER_VCPU: bool = true;

/// Whether `owner` could take a VM tag now, without keeping it
/// (`svm_registry::vm_tag_probe`).
#[cfg(feature = "test-hooks")]
pub(crate) fn vm_tag_probe(owner: Charge) -> Result<(), crate::memory::tag_pool::TagRefusal> {
    crate::arch::hypervisor::svm_registry::vm_tag_probe(owner)
}

/// The host the kernel's VM fixtures run on: SVM or VMX root operation is
/// active on every online CPU.
#[cfg(feature = "test-hooks")]
pub(crate) fn guest_fixture_host() -> bool {
    crate::cpu_features::has_x86_virt()
}

/// Boot fixture `VM-GUEST-BUDGET` (census §8.6(b)), shared with AArch64.
#[cfg(feature = "test-hooks")]
#[path = "../guest_budget_selftest.rs"]
pub(crate) mod guest_budget_selftest;

/// Boot fixture `TAG-POOL-OWNER-LIMIT` (census §8.5), shared with AArch64.
#[cfg(feature = "test-hooks")]
#[path = "../tag_pool_selftest.rs"]
pub(crate) mod tag_pool_selftest;

pub(crate) fn create_vcpu(owner: Charge, vm_id: usize, entry_pc: u64) -> ViResult<usize> {
    crate::arch::hypervisor::svm_registry::create_vcpu(owner, vm_id, entry_pc)
}

pub(crate) fn map_guest_memory(
    owner: Charge,
    vm_id: usize,
    ipa: u64,
    size: usize,
    writable: bool,
) -> ViResult<()> {
    crate::arch::hypervisor::svm_registry::map_guest_memory(owner, vm_id, ipa, size, writable)
}

pub(crate) fn guest_window(owner: Charge, vm_id: usize) -> ViResult<(u64, usize)> {
    crate::arch::hypervisor::svm_registry::guest_window(owner, vm_id)
}

pub(crate) fn run_vcpu(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    _budget_ns: u64,
) -> ViResult<abi::hypervisor::ViVmExit> {
    use abi::hypervisor::ViVmExit as ApiVmExit;
    use hal::ViVmExit as HalVmExit;
    let hal_exit = crate::arch::hypervisor::svm_registry::run_vcpu_hal(owner, vm_id, vcpu_id)?;
    // HAL → API conversion (VERSION 2 ABI — the x86 variants are frozen at
    // discriminants 8-11). `reg` is not decoded for PortIn (guest `IN`
    // always targets (E)AX) → 0.
    let api_exit = match hal_exit {
        HalVmExit::MmioRead { ipa, size, reg } => ApiVmExit::MmioRead { ipa, size, reg },
        HalVmExit::MmioWrite { ipa, size, val } => ApiVmExit::MmioWrite { ipa, size, val },
        HalVmExit::Preempted => ApiVmExit::Preempted,
        HalVmExit::Shutdown => ApiVmExit::Shutdown,
        HalVmExit::Unknown { ec, iss } => ApiVmExit::Unknown { ec, iss },
        HalVmExit::PortIn { port, size } => ApiVmExit::PortIn { port, size, reg: 0 },
        HalVmExit::PortOut { port, size, val } => ApiVmExit::PortOut { port, size, val },
        HalVmExit::Hlt => ApiVmExit::Hlt,
        HalVmExit::Msr {
            index,
            is_write,
            value,
        } => ApiVmExit::Msr {
            index,
            is_write,
            val: value,
        },
        // ARM-only HAL variants — unreachable on x86 (no aarch64 exits here).
        HalVmExit::Hvc { .. } | HalVmExit::Wfi | HalVmExit::SysReg { .. } => {
            ApiVmExit::Unknown { ec: 0, iss: 0 }
        }
    };
    Ok(api_exit)
}

pub(crate) fn vcpu_regs(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    regs: &mut [u64; 32],
    mode: usize,
) -> ViResult<usize> {
    if mode > 1 {
        return Err(ViError::InvalidArgument);
    }
    crate::arch::hypervisor::svm_registry::vcpu_regs(owner, vm_id, vcpu_id, regs, mode == 1)
}

pub(crate) fn write_guest_memory(
    owner: Charge,
    vm_id: usize,
    gpa: u64,
    src: &[u8],
) -> ViResult<usize> {
    crate::arch::hypervisor::svm_registry::write_guest_memory(owner, vm_id, gpa, src)
}

pub(crate) fn read_guest_memory(
    owner: Charge,
    vm_id: usize,
    gpa: u64,
    dst: &mut [u8],
) -> ViResult<usize> {
    crate::arch::hypervisor::svm_registry::read_guest_memory(owner, vm_id, gpa, dst)
}

pub(crate) fn inject_irq(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    intid: u32,
) -> ViResult<usize> {
    // `intid` is reinterpreted as an x86 interrupt vector (8259 line remap).
    crate::arch::hypervisor::svm_registry::inject_irq(owner, vm_id, vcpu_id, intid)?;
    Ok(0)
}

pub(crate) fn reap_vms_for(owner: Charge) {
    crate::arch::hypervisor::svm_registry::reap_vms_for(owner);
}
