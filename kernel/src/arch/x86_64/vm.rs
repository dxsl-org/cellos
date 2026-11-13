// SPDX-License-Identifier: MPL-2.0
//! `vm`: `crate::hypervisor::registry` on x86_64, backed by the SVM VM store
//! (`crate::arch::hypervisor::svm_registry`).

use types::{ViError, ViResult};

pub(crate) fn create_vm(owner: usize, guest_pages: usize) -> ViResult<usize> {
    crate::arch::hypervisor::svm_registry::create_vm(owner, guest_pages)
}

pub(crate) fn create_vcpu(owner: usize, vm_id: usize, entry_pc: u64) -> ViResult<usize> {
    crate::arch::hypervisor::svm_registry::create_vcpu(owner, vm_id, entry_pc)
}

pub(crate) fn map_guest_memory(
    owner: usize,
    vm_id: usize,
    ipa: u64,
    size: usize,
    writable: bool,
) -> ViResult<()> {
    crate::arch::hypervisor::svm_registry::map_guest_memory(owner, vm_id, ipa, size, writable)
}

pub(crate) fn guest_window(owner: usize, vm_id: usize) -> ViResult<(u64, usize)> {
    crate::arch::hypervisor::svm_registry::guest_window(owner, vm_id)
}

pub(crate) fn run_vcpu(
    owner: usize,
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
    owner: usize,
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
    owner: usize,
    vm_id: usize,
    gpa: u64,
    src: &[u8],
) -> ViResult<usize> {
    crate::arch::hypervisor::svm_registry::write_guest_memory(owner, vm_id, gpa, src)
}

pub(crate) fn read_guest_memory(
    owner: usize,
    vm_id: usize,
    gpa: u64,
    dst: &mut [u8],
) -> ViResult<usize> {
    crate::arch::hypervisor::svm_registry::read_guest_memory(owner, vm_id, gpa, dst)
}

pub(crate) fn inject_irq(
    owner: usize,
    vm_id: usize,
    vcpu_id: usize,
    intid: u32,
) -> ViResult<usize> {
    // `intid` is reinterpreted as an x86 interrupt vector (8259 line remap).
    crate::arch::hypervisor::svm_registry::inject_irq(owner, vm_id, vcpu_id, intid)?;
    Ok(0)
}

pub(crate) fn reap_vms_for_task(dead_tid: usize) {
    crate::arch::hypervisor::svm_registry::reap_vms_for_task(dead_tid);
}
