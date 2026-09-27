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
    // The switch routine programs the root register from this pair, and the
    // encoding has one definition: it must name THIS root and THIS tag, or the
    // Cell would resume under a different address space than the plan selected.
    #[cfg(target_arch = "aarch64")]
    let encoding_ok = {
        let register = crate::hal::domain::root_register_value(root, asid);
        register != 0
            && register & 0x0000_ffff_ffff_f000 == root
            && (register >> 48) & 0xffff == asid
    };
    #[cfg(not(target_arch = "aarch64"))]
    let encoding_ok = true;
    let plan_ok = root != 0
        && asid != 0
        && roots == 1
        && flushes == FLUSHES_PER_ACTIVATION
        && domain_id != 0
        && domain_generation != 0
        && encoding_ok
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

    #[cfg(target_arch = "aarch64")]
    let root_switch_ok = run_root_switch_witness();
    #[cfg(not(target_arch = "aarch64"))]
    let root_switch_ok = true;

    plan_ok && flush_counter_ok && resume_ok && run_pinned_retire_regression() && root_switch_ok
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

/// A real root switch: the switch routine's root-*writing* path, executed.
///
/// Every switch the kernel performs today is SAS to SAS, so the ordering fix only
/// ever takes its "no write" branch. This fixture builds two private roots, runs
/// on each of their stacks in turn, and returns to the boot stack — the boot stack
/// is mapped by neither root, so it also discriminates the ordering: programming
/// a root before the outgoing context was saved (the shape phase 02 fixed) would
/// fault on the save's next stack access instead of returning.
///
/// The contexts live in statics (kernel data is shared into every root), and the
/// entries touch nothing but statics: a frame on the outgoing stack is unreachable
/// once the incoming root is live.
#[cfg(target_arch = "aarch64")]
pub(crate) fn run_root_switch_witness() -> bool {
    use crate::memory::address_space::{AddressSpace, AddressSpaceBuilder};
    use crate::task::stack::Stack;
    use alloc::sync::Arc;
    use core::cell::UnsafeCell;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct ContextSlot(UnsafeCell<crate::hal::arch::Context>);
    // SAFETY: one hart, interrupts disabled for the whole fixture, and the switch
    // routine is the only writer.
    unsafe impl Sync for ContextSlot {}

    static CTX_FIXTURE: ContextSlot =
        ContextSlot(UnsafeCell::new(crate::hal::arch::Context::zeroed()));
    static CTX_SCRATCH: ContextSlot =
        ContextSlot(UnsafeCell::new(crate::hal::arch::Context::zeroed()));
    static CTX_B: ContextSlot = ContextSlot(UnsafeCell::new(crate::hal::arch::Context::zeroed()));
    static CTX_C: ContextSlot = ContextSlot(UnsafeCell::new(crate::hal::arch::Context::zeroed()));
    static BASE_B: AtomicUsize = AtomicUsize::new(0);
    static ASID_B: AtomicUsize = AtomicUsize::new(0);
    static BASE_C: AtomicUsize = AtomicUsize::new(0);
    static ASID_C: AtomicUsize = AtomicUsize::new(0);
    static KERNEL_BASE: AtomicUsize = AtomicUsize::new(0);
    static OBSERVED_B: AtomicUsize = AtomicUsize::new(0);
    static OBSERVED_C: AtomicUsize = AtomicUsize::new(0);
    static B_ENTERED: AtomicBool = AtomicBool::new(false);
    static C_ENTERED: AtomicBool = AtomicBool::new(false);
    static B_RESUMED: AtomicBool = AtomicBool::new(false);
    static SCRATCH_ENTERED: AtomicBool = AtomicBool::new(false);
    static SCRATCH_RESUMED: AtomicBool = AtomicBool::new(false);

    /// Runs on root C's stack, under root C.
    extern "C" fn entry_c() {
        OBSERVED_C.store(crate::hal::domain::current_root(), Ordering::Release);
        C_ENTERED.store(true, Ordering::Release);
        // Root-to-root: the outgoing context is B's, saved before C's root lands.
        unsafe {
            crate::hal::arch::Context::switch_with_root(
                CTX_C.0.get(),
                CTX_B.0.get(),
                BASE_B.load(Ordering::Acquire),
                ASID_B.load(Ordering::Acquire),
            );
        }
        // C is never resumed: B returns to the boot context instead.
        loop {
            core::hint::spin_loop();
        }
    }

    /// Runs on root B's stack, under root B; resumed here after C returns to it.
    extern "C" fn entry_b() {
        OBSERVED_B.store(crate::hal::domain::current_root(), Ordering::Release);
        B_ENTERED.store(true, Ordering::Release);
        unsafe {
            crate::hal::arch::Context::switch_with_root(
                CTX_B.0.get(),
                CTX_C.0.get(),
                BASE_C.load(Ordering::Acquire),
                ASID_C.load(Ordering::Acquire),
            );
        }
        B_RESUMED.store(true, Ordering::Release);
        // Back to the scratch stack under the kernel root, ASID 0. That stack is
        // a plain allocation, mapped by no private root, which is what makes the
        // next step discriminate the ordering.
        unsafe {
            crate::hal::arch::Context::switch_with_root(
                CTX_B.0.get(),
                CTX_SCRATCH.0.get(),
                KERNEL_BASE.load(Ordering::Acquire),
                0,
            );
        }
        loop {
            core::hint::spin_loop();
        }
    }

    /// Runs on the scratch stack under the kernel root.
    ///
    /// This is the discriminating step: the SAS-to-domain switch below saves its
    /// outgoing context onto this stack, and no private root maps it. The shape
    /// phase 02 fixed — programming the incoming root before that save — would
    /// fault here instead of returning.
    extern "C" fn entry_scratch() {
        SCRATCH_ENTERED.store(true, Ordering::Release);
        unsafe {
            crate::hal::arch::Context::switch_with_root(
                CTX_SCRATCH.0.get(),
                CTX_B.0.get(),
                BASE_B.load(Ordering::Acquire),
                ASID_B.load(Ordering::Acquire),
            );
        }
        SCRATCH_RESUMED.store(true, Ordering::Release);
        // Stage a safe-root completion the way `SwitchPlan::root_switch` does for a
        // transition to the kernel root, so the incoming-side completion below has
        // real work to do.
        hart_local::mark_safe_root_pending();
        // No root write: B returned to the kernel root already.
        unsafe {
            crate::hal::arch::Context::switch_with_root(
                CTX_SCRATCH.0.get(),
                CTX_FIXTURE.0.get(),
                0,
                0,
            );
        }
        loop {
            core::hint::spin_loop();
        }
    }

    fn build_root(stack: &Stack) -> Option<Arc<AddressSpace>> {
        let mut builder = AddressSpaceBuilder::new();
        builder.map_registered_execution(stack);
        builder.build().ok()
    }

    let kernel_root = crate::hal::domain::kernel_ttbr0();
    if kernel_root == 0 {
        log::error!("S22-{}-ROOT-SWITCH: FAIL kernel root not recorded", ARCH_TAG);
        return false;
    }
    let (Ok(stack_b), Ok(stack_c), Ok(stack_scratch)) = (
        Stack::new_kernel(2),
        Stack::new_kernel(2),
        // Deliberately NOT registered with either root: the ordering witness
        // depends on the outgoing stack being unreachable under the incoming root.
        Stack::new_kernel(2),
    ) else {
        log::error!("S22-{}-ROOT-SWITCH: FAIL stack", ARCH_TAG);
        return false;
    };
    let (Some(root_b), Some(root_c)) = (build_root(&stack_b), build_root(&stack_c)) else {
        log::error!("S22-{}-ROOT-SWITCH: FAIL root", ARCH_TAG);
        return false;
    };

    let saved_daif = crate::hal::arch::save_and_disable_interrupts();
    KERNEL_BASE.store(kernel_root & 0x0000_ffff_ffff_f000, Ordering::Release);
    BASE_B.store(root_b.root_ppn() << 12, Ordering::Release);
    ASID_B.store(root_b.asid(), Ordering::Release);
    BASE_C.store(root_c.root_ppn() << 12, Ordering::Release);
    ASID_C.store(root_c.asid(), Ordering::Release);
    unsafe {
        let ctx_b = &mut *CTX_B.0.get();
        ctx_b.sp = stack_b.top as u64;
        ctx_b.x30 = entry_b as *const () as usize as u64;
        ctx_b.daif = saved_daif as u64;
        let ctx_c = &mut *CTX_C.0.get();
        ctx_c.sp = stack_c.top as u64;
        ctx_c.x30 = entry_c as *const () as usize as u64;
        ctx_c.daif = saved_daif as u64;
        let ctx_scratch = &mut *CTX_SCRATCH.0.get();
        ctx_scratch.sp = stack_scratch.top as u64;
        ctx_scratch.x30 = entry_scratch as *const () as usize as u64;
        ctx_scratch.daif = saved_daif as u64;
    }

    // Boot context -> scratch stack (no root write), then the whole chain runs
    // with its SAS side on memory no private root maps.
    unsafe {
        crate::hal::arch::Context::switch_with_root(
            CTX_FIXTURE.0.get(),
            CTX_SCRATCH.0.get(),
            0,
            0,
        );
    }

    // The incoming side of the last switch: the same steps the scheduler path runs.
    crate::task::complete_incoming_switch(hart_local::current_hart_id());
    let safe_root_consumed = !hart_local::take_safe_root_pending();
    let identity_cleared = hart_local::current_domain() == (0, 0);
    let observed_kernel = crate::hal::domain::current_root();
    let expected_b = crate::hal::domain::root_register_value(
        BASE_B.load(Ordering::Acquire),
        ASID_B.load(Ordering::Acquire),
    );
    let expected_c = crate::hal::domain::root_register_value(
        BASE_C.load(Ordering::Acquire),
        ASID_C.load(Ordering::Acquire),
    );
    // The witness discriminates the ordering only if the outgoing stack is not
    // mapped in the incoming root: the switch's save phase runs on it. Report the
    // fact rather than assume it, so a future change to the shared ranges cannot
    // quietly turn this into a test that would pass either way.
    let boot_stack_probe = stack_scratch.usable_start();
    let boot_stack_shared = crate::memory::domain_supervisor_registry::shared_snapshot()
        .iter()
        .any(|range| boot_stack_probe >= range.start && boot_stack_probe < range.end);

    let ok = safe_root_consumed
        && identity_cleared
        && SCRATCH_ENTERED.load(Ordering::Acquire)
        && SCRATCH_RESUMED.load(Ordering::Acquire)
        && B_ENTERED.load(Ordering::Acquire)
        && C_ENTERED.load(Ordering::Acquire)
        && B_RESUMED.load(Ordering::Acquire)
        && OBSERVED_B.load(Ordering::Acquire) == expected_b
        && OBSERVED_C.load(Ordering::Acquire) == expected_c
        && observed_kernel == kernel_root;
    unsafe {
        crate::hal::arch::restore_sstatus(saved_daif);
    }
    if ok {
        log::info!(
            "S22-{}-ROOT-SWITCH: PASS b={:#x} c={:#x} back={:#x} outgoing_stack_shared={} safe_root_consumed={}",
            ARCH_TAG,
            expected_b,
            expected_c,
            observed_kernel,
            boot_stack_shared,
            safe_root_consumed
        );
    } else {
        log::error!(
            "S22-{}-ROOT-SWITCH: FAIL b_entered={} c_entered={} b_resumed={} observed_b={:#x} want_b={:#x} observed_c={:#x} want_c={:#x} back={:#x} want_kernel={:#x}",
            ARCH_TAG,
            B_ENTERED.load(Ordering::Acquire),
            C_ENTERED.load(Ordering::Acquire),
            B_RESUMED.load(Ordering::Acquire),
            OBSERVED_B.load(Ordering::Acquire),
            expected_b,
            OBSERVED_C.load(Ordering::Acquire),
            expected_c,
            observed_kernel,
            kernel_root
        );
    }
    ok
}
