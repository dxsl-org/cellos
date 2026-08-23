//! RV64 native-domain scheduler fixtures. They deliberately construct no user mapping.
use super::{
    domain_switch::SwitchPlan,
    hart_local,
    tcb::{Task, TaskAddressSpace},
};
use crate::memory::address_space::AddressSpaceBuilder;
use alloc::vec::Vec;
use types::CellId;

/// Runs only with QEMU test hooks: SAS dispatch must not touch SATP or flush,
/// and a private root must produce a non-SAS plan before it can reach assembly.
pub(crate) fn run_primary() -> bool {
    use crate::memory::domain_supervisor_registry::{
        contains_shared_kind, shared_snapshot, SupervisorRangeKind,
    };
    let kinds = [
        SupervisorRangeKind::KernelStack,
        SupervisorRangeKind::PrivatePageTable,
    ];
    if kinds.len() != 2
        || !contains_shared_kind(SupervisorRangeKind::KernelHeap)
        || !contains_shared_kind(SupervisorRangeKind::StaticText)
        || !contains_shared_kind(SupervisorRangeKind::StaticReadOnly)
        || !contains_shared_kind(SupervisorRangeKind::StaticWritable)
        || shared_snapshot().len() != 4
    {
        log::error!("S22-RV64-REGISTRY: FAIL");
        return false;
    }
    let sas_task = Task::new(91_001, CellId(91_001), "domain-sas", Vec::new());
    crate::hal::domain::reset_switch_counters();
    let Some(sas_plan) = SwitchPlan::new(core::ptr::null_mut(), core::ptr::null(), Some(&sas_task))
    else {
        return false;
    };
    if !sas_plan.is_sas_fast_path() || sas_plan.root_switch() != (0, 0) {
        return false;
    }
    let (roots, flushes) = crate::hal::domain::switch_counters();
    let sas_ok = roots == 0 && flushes == 0;
    if sas_ok {
        log::info!(
            "S22-RV64-SAS-FASTPATH: PASS roots=0 flushes=0 harts={}",
            super::smp::online_hart_count()
        );
    } else {
        log::error!(
            "S22-RV64-SAS-FASTPATH: FAIL roots={} flushes={}",
            roots,
            flushes
        );
        return false;
    }

    let kernel_stack = match crate::task::stack::Stack::new_kernel(1) {
        Ok(stack) => stack,
        Err(error) => {
            log::error!("S22-RV64-PLAN: FAIL stack={:?}", error);
            return false;
        }
    };
    let mut builder = AddressSpaceBuilder::new();
    builder.map_registered_execution(&kernel_stack);
    let address_space = match builder.build() {
        Ok(address_space) => address_space,
        Err(error) => {
            log::error!("S22-RV64-PLAN: FAIL address-space={:?}", error);
            return false;
        }
    };
    let mut domain_task = Task::new(91_002, CellId(91_002), "domain-plan", Vec::new());
    domain_task.bind_address_space_for_test(address_space);
    let (root_ppn, asid) =
        match SwitchPlan::new(core::ptr::null_mut(), core::ptr::null(), Some(&domain_task)) {
            Some(plan) if !plan.is_sas_fast_path() => plan.root_switch(),
            _ => {
                log::error!("S22-RV64-PLAN: FAIL");
                return false;
            }
        };
    let (roots, flushes) = crate::hal::domain::switch_counters();
    let (domain_id, domain_generation) = hart_local::current_domain();
    let plan_ok = root_ppn != 0
        && asid != 0
        && roots == 1
        && flushes == 1
        && domain_id != 0
        && domain_generation != 0
        && hart_local::domain_ack_generation_for(hart_local::current_hart_id()) == 0;
    if plan_ok {
        log::info!(
            "S22-RV64-PLAN: PASS harts={}",
            super::smp::online_hart_count()
        );
    } else {
        log::error!("S22-RV64-PLAN: FAIL");
    }
    plan_ok
}

/// Verify that a pre-dispatch domain binding survived cross-hart selection.
pub(crate) fn resumed_worker_domain_matches(worker_tid: usize) -> bool {
    let scheduler = super::SCHEDULER.lock();
    let Some(task) = scheduler
        .as_ref()
        .and_then(|scheduler| scheduler.tasks.get(&worker_tid))
    else {
        return false;
    };
    let TaskAddressSpace::Domain(address_space) = &task.address_space else {
        return false;
    };
    hart_local::current_hart_id() == 0
        && hart_local::current_domain()
            == (address_space.identity().raw(), address_space.generation())
}
