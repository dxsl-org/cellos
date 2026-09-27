//! Native-domain scheduler fixtures, shared by every architecture that can carry
//! a private root. They deliberately construct no user mapping.
use super::{
    domain_switch::{root_tuple, SwitchPlan},
    hart_local,
    tcb::{Task, TaskAddressSpace},
};
use crate::memory::address_space::AddressSpaceBuilder;
use alloc::vec::Vec;
use types::CellId;

/// Marker prefix: the RV64 lane greps `S22-RV64-*`, the AArch64 lane `S22-AARCH64-*`.
#[cfg(target_arch = "riscv64")]
const ARCH_TAG: &str = "RV64";
#[cfg(target_arch = "aarch64")]
const ARCH_TAG: &str = "AARCH64";

/// Invalidations one domain activation must issue on this architecture. RV64
/// fences for the incoming ASID as part of installing it; AArch64 carries the
/// ASID in `TTBR0_EL1`, so a switch to a root whose translations are still
/// valid issues no `tlbi` at all.
#[cfg(target_arch = "riscv64")]
const FLUSHES_PER_ACTIVATION: usize = 1;
#[cfg(not(target_arch = "riscv64"))]
const FLUSHES_PER_ACTIVATION: usize = 0;

/// Harts running kernel code in this boot.
///
/// Only RV64 starts secondaries (`smp::start_secondaries` is SBI HSM), so the
/// other architectures have no tracker to consult and exactly one PE executes
/// the fixture.
#[cfg(target_arch = "riscv64")]
fn observed_harts() -> usize {
    super::smp::online_hart_count()
}
#[cfg(not(target_arch = "riscv64"))]
fn observed_harts() -> usize {
    1
}

/// Runs only with QEMU test hooks: SAS dispatch must not program a root or
/// flush, and a private root must produce a non-SAS plan before it can reach
/// assembly.
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
        || shared_snapshot().len() < 4
    {
        log::error!("S22-{}-REGISTRY: FAIL", ARCH_TAG);
        return false;
    }
    let sas_task = Task::new(91_001, CellId(91_001), "domain-sas", Vec::new());
    crate::hal::domain::reset_switch_counters();
    let Some(sas_plan) = SwitchPlan::new(core::ptr::null_mut(), core::ptr::null(), Some(&sas_task))
    else {
        return false;
    };
    // The no-write path means exactly one thing: SAS to SAS, where the kernel
    // root is already live. It is the zero tuple, not a "small" root.
    if sas_plan.root_switch() != (0, 0) {
        log::error!("S22-{}-SAS-FASTPATH: FAIL root-programmed", ARCH_TAG);
        return false;
    }
    let (roots, flushes) = crate::hal::domain::switch_counters();
    let sas_ok = roots == 0 && flushes == 0;
    if sas_ok {
        log::info!(
            "S22-{}-SAS-FASTPATH: PASS roots=0 flushes=0 harts={}",
            ARCH_TAG,
            observed_harts()
        );
    } else {
        log::error!(
            "S22-{}-SAS-FASTPATH: FAIL roots={} flushes={}",
            ARCH_TAG,
            roots,
            flushes
        );
        return false;
    }

    let kernel_stack = match crate::task::stack::Stack::new_kernel(1) {
        Ok(stack) => stack,
        Err(error) => {
            log::error!("S22-{}-PLAN: FAIL stack={:?}", ARCH_TAG, error);
            return false;
        }
    };
    let mut builder = AddressSpaceBuilder::new();
    builder.map_registered_execution(&kernel_stack);
    let address_space = match builder.build() {
        Ok(address_space) => address_space,
        Err(error) => {
            log::error!("S22-{}-PLAN: FAIL address-space={:?}", ARCH_TAG, error);
            return false;
        }
    };
    let mut domain_task = Task::new(91_002, CellId(91_002), "domain-plan", Vec::new());
    domain_task.bind_address_space_for_test(address_space);
    let (root, asid) =
        match SwitchPlan::new(core::ptr::null_mut(), core::ptr::null(), Some(&domain_task)) {
            Some(plan) => plan.root_switch(),
            None => {
                log::error!("S22-{}-PLAN: FAIL plan-rejected", ARCH_TAG);
                return false;
            }
        };
    let (roots, flushes) = crate::hal::domain::switch_counters();
    let (domain_id, domain_generation) = hart_local::current_domain();
    let plan_ok = root != 0
        && asid != 0
        && roots == 1
        && flushes == FLUSHES_PER_ACTIVATION
        && domain_id != 0
        && domain_generation != 0
        && hart_local::domain_ack_generation_for(hart_local::current_hart_id()) == 0;
    if plan_ok {
        log::info!(
            "S22-{}-PLAN: PASS harts={}",
            ARCH_TAG,
            observed_harts()
        );
    } else {
        log::error!(
            "S22-{}-PLAN: FAIL root={:#x} asid={:#x} roots={} flushes={} domain=({},{})",
            ARCH_TAG,
            root,
            asid,
            roots,
            flushes,
            domain_id,
            domain_generation
        );
    }

    // The invalidation counter has to be live, or "zero flushes" above would be
    // satisfied by a counter nobody increments. One explicit ASID invalidation
    // must move it by exactly one.
    crate::hal::domain::flush_asid(asid);
    let (_, flushes_after) = crate::hal::domain::switch_counters();
    let flush_counter_ok = flushes_after == flushes + 1;
    if !flush_counter_ok {
        log::error!(
            "S22-{}-FLUSH-COUNTER: FAIL before={} after={}",
            ARCH_TAG,
            flushes,
            flushes_after
        );
    }

    // Re-derivation for the trap-root discipline: a domain re-selected while it
    // is still the hart's current domain must program its root again, because
    // trap entry installs the kernel root. Collapsing this plan to the no-write
    // path would resume the Cell under the kernel root, so the fixture asserts
    // the write happens, that it names THIS domain, and that the published
    // domain identity is left alone.
    let mut resume_ok = false;
    if let Some(resume_plan) =
        SwitchPlan::new(core::ptr::null_mut(), core::ptr::null(), Some(&domain_task))
    {
        let resume_root = resume_plan.root_switch();
        let (resume_roots, resume_flushes) = crate::hal::domain::switch_counters();
        resume_ok = resume_root == (root, asid)
            && resume_roots == 2
            && resume_flushes == flushes_after + FLUSHES_PER_ACTIVATION
            && hart_local::current_domain() == (domain_id, domain_generation);
    }
    if resume_ok {
        log::info!(
            "S22-{}-RESUME-ROOT: PASS harts={}",
            ARCH_TAG,
            observed_harts()
        );
    } else {
        log::error!("S22-{}-RESUME-ROOT: FAIL", ARCH_TAG);
    }

    plan_ok && flush_counter_ok && resume_ok && run_pinned_retire_regression()
}

/// Regression for the pin→plan window: `retire()` is a bare atomic store that
/// takes no lock, so it can land between the execution pin (pick_next_local
/// filter) and `SwitchPlan::new`. A successfully pinned task must still derive
/// Activate and program ITS root — never the KERNEL_ROOT safe-root tuple.
pub(crate) fn run_pinned_retire_regression() -> bool {
    let kernel_stack = match crate::task::stack::Stack::new_kernel(2) {
        Ok(stack) => stack,
        Err(error) => {
            log::error!("S22-{}-PIN-DYING: FAIL stack={:?}", ARCH_TAG, error);
            return false;
        }
    };
    let mut builder = AddressSpaceBuilder::new();
    builder.map_registered_execution(&kernel_stack);
    let address_space = match builder.build() {
        Ok(address_space) => address_space,
        Err(error) => {
            log::error!("S22-{}-PIN-DYING: FAIL address-space={:?}", ARCH_TAG, error);
            return false;
        }
    };
    let hart = hart_local::current_hart_id();
    if address_space.begin_execution(hart).is_err() {
        log::error!("S22-{}-PIN-DYING: FAIL pin-rejected-live-root", ARCH_TAG);
        return false;
    }
    let bit = 1usize << hart;
    if address_space.current_harts() & bit == 0 {
        log::error!("S22-{}-PIN-DYING: FAIL pin-bit-unset", ARCH_TAG);
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
        log::error!(
            "S22-{}-PIN-DYING: FAIL repin-erased-preexisting-pin",
            ARCH_TAG
        );
        return false;
    }
    let mut pinned_task = Task::new(91_003, CellId(91_003), "domain-pin-dying", Vec::new());
    pinned_task.bind_address_space_for_test(alloc::sync::Arc::clone(&address_space));
    let Some(plan) = SwitchPlan::new(core::ptr::null_mut(), core::ptr::null(), Some(&pinned_task))
    else {
        log::error!("S22-{}-PIN-DYING: FAIL plan-rejected-after-pin", ARCH_TAG);
        return false;
    };
    let (root, asid) = plan.root_switch();
    let expected = root_tuple(&address_space);
    // The safe-root diversion this regression guards against surfaces as the
    // kernel tuple (kernel root, asid 0); a private root has asid != 0.
    if (root, asid) == expected && asid != 0 {
        log::info!(
            "S22-{}-PIN-DYING: PASS harts={}",
            ARCH_TAG,
            observed_harts()
        );
        true
    } else {
        log::error!(
            "S22-{}-PIN-DYING: FAIL root=({:#x},{:#x}) expected=({:#x},{:#x})",
            ARCH_TAG,
            root,
            asid,
            expected.0,
            expected.1
        );
        false
    }
}

/// Verify that a pre-dispatch domain binding survived cross-hart selection.
///
/// Consumed by `context_handoff_selftest`, which runs on RV64 (only RV64 starts
/// secondaries, so only there can a selection cross a hart).
#[cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]
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
