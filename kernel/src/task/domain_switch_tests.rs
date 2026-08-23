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
    plan_ok && run_pinned_retire_regression()
}

/// Regression for the pin→plan window: `retire()` is a bare atomic store that
/// takes no lock, so it can land between the execution pin (pick_next_local
/// filter) and `SwitchPlan::new`. A successfully pinned task must still derive
/// Activate and program ITS root — never the KERNEL_ROOT safe-root tuple.
pub(crate) fn run_pinned_retire_regression() -> bool {
    let kernel_stack = match crate::task::stack::Stack::new_kernel(2) {
        Ok(stack) => stack,
        Err(error) => {
            log::error!("S22-RV64-PIN-DYING: FAIL stack={:?}", error);
            return false;
        }
    };
    let mut builder = AddressSpaceBuilder::new();
    builder.map_registered_execution(&kernel_stack);
    let address_space = match builder.build() {
        Ok(address_space) => address_space,
        Err(error) => {
            log::error!("S22-RV64-PIN-DYING: FAIL address-space={:?}", error);
            return false;
        }
    };
    let hart = hart_local::current_hart_id();
    if address_space.begin_execution(hart).is_err() {
        log::error!("S22-RV64-PIN-DYING: FAIL pin-rejected-live-root");
        return false;
    }
    let bit = 1usize << hart;
    if address_space.current_harts() & bit == 0 {
        log::error!("S22-RV64-PIN-DYING: FAIL pin-bit-unset");
        return false;
    }
    address_space.retire();
    // Re-pinning a root that died after the first pin must fail closed AND
    // leave the pre-existing execution pin intact — erasing it would drop this
    // executing hart out of the drain set.
    if !matches!(
        address_space.begin_execution(hart),
        Err(crate::memory::address_space::AddressSpaceError::Dying)
    ) || address_space.current_harts() & bit == 0
    {
        log::error!("S22-RV64-PIN-DYING: FAIL repin-erased-preexisting-pin");
        return false;
    }
    let mut pinned_task = Task::new(91_003, CellId(91_003), "domain-pin-dying", Vec::new());
    pinned_task.bind_address_space_for_test(alloc::sync::Arc::clone(&address_space));
    let Some(plan) = SwitchPlan::new(core::ptr::null_mut(), core::ptr::null(), Some(&pinned_task))
    else {
        log::error!("S22-RV64-PIN-DYING: FAIL plan-rejected-after-pin");
        return false;
    };
    if plan.is_sas_fast_path() {
        log::error!("S22-RV64-PIN-DYING: FAIL diverted-to-fast-path");
        return false;
    }
    let (root_ppn, asid) = plan.root_switch();
    let expected = (address_space.root_ppn(), address_space.asid());
    // The safe-root diversion this regression guards against surfaces as the
    // kernel tuple (KERNEL_ROOT>>12, asid 0); a private root has asid != 0.
    if (root_ppn, asid) == expected && asid != 0 {
        log::info!(
            "S22-RV64-PIN-DYING: PASS harts={}",
            super::smp::online_hart_count()
        );
        true
    } else {
        log::error!(
            "S22-RV64-PIN-DYING: FAIL root=({:#x},{:#x}) expected=({:#x},{:#x})",
            root_ppn,
            asid,
            expected.0,
            expected.1
        );
        false
    }
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
