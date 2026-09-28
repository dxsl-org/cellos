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

#[cfg(target_arch = "aarch64")]
use crate::hal::PageTableTrait;

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

    // Phase 02, AArch64 private-root invalidation: (a) the leaf composition,
    // (c) the release path's flush kind, (b) the behavioural proof that an
    // ASID-targeted invalidation reaches a private leaf. (b) runs last: it is
    // the only one that enters a root of its own making.
    #[cfg(target_arch = "aarch64")]
    let private_leaf_ok = run_private_leaf_witness();
    #[cfg(not(target_arch = "aarch64"))]
    let private_leaf_ok = true;
    #[cfg(target_arch = "aarch64")]
    let release_flush_ok = run_release_flush_witness();
    #[cfg(not(target_arch = "aarch64"))]
    let release_flush_ok = true;
    #[cfg(target_arch = "aarch64")]
    let asid_invalidation_ok = run_asid_invalidation_witness();
    #[cfg(not(target_arch = "aarch64"))]
    let asid_invalidation_ok = true;

    // The admission posture and its denial cases. RV64 asserts these from `kmain`
    // (`main.rs` owns that call site); AArch64 gets them from this fixture so the
    // architecture whose switch-ordering gate just reopened also re-asserts, on
    // the same code path, that the *disabled* posture still denies and that the
    // single publication point refuses a domain-class launch while draining.
    // `ENABLED` is a real assertion here: `enable_for_boot` has already run.
    //
    // The EL2 host is excluded: it cannot program a private root at all, so
    // `enable_for_boot` refuses the posture there by design and an `ENABLED`
    // assertion would be asserting against the machine rather than the code.
    #[cfg(target_arch = "aarch64")]
    let admission_ok = if crate::hal::aarch64::el2::is_el2() {
        true
    } else {
        crate::loader::domain_admission::run_selftest()
    };
    #[cfg(not(target_arch = "aarch64"))]
    let admission_ok = true;

    plan_ok
        && flush_counter_ok
        && resume_ok
        && run_pinned_retire_regression()
        && root_switch_ok
        && private_leaf_ok
        && release_flush_ok
        && asid_invalidation_ok
        && admission_ok
}

/// Whether a private root has been observed live in this boot.
///
/// The teardown verdict is gated on it, so a boot that never admitted a domain
/// cannot satisfy "frames accounted for on teardown" vacuously.
#[cfg(target_arch = "aarch64")]
static CPU_SEEN_PRIVATE_ROOT: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// The domain's **live** `TTBR0_EL1`, read from inside the domain's own kernel
/// context.
///
/// Called from `task::complete_incoming_switch`, which runs on the incoming side
/// of a raw switch. For a resumed private-root task that is the task's own kernel
/// stack under its own root — the one point where "what the plan programmed" and
/// "what the PE is translating through" can be compared with no trap in between.
///
/// A *fresh* task's first entry is not one of those points: the switch routine
/// returns into `x30`, so the code after the call belongs to the outgoing task
/// and is reached only when that task is resumed. The observation therefore
/// lands on the domain's first *resume*, which a `Yield` (or a peer cell running
/// alongside it) guarantees — and a self-reselection still executes the
/// post-switch code, because the routine saves and restores the same context.
///
/// The safe-root handoff is excluded by construction: the caller invokes this
/// after `acknowledge_safe_root`, which clears the published domain identity, so
/// the kernel root live at that instant cannot be misread as a private root.
#[cfg(target_arch = "aarch64")]
pub(crate) fn observe_incoming_live_root() {
    use core::sync::atomic::{AtomicBool, Ordering};

    /// One report per boot: the first resumed private-root context is the fact
    /// under test, and every later switch would only repeat it.
    static REPORTED: AtomicBool = AtomicBool::new(false);

    let (id, generation) = hart_local::current_domain();
    if id == 0 {
        return;
    }
    let live = crate::hal::domain::current_root();
    let asid = (live >> 48) & 0xffff;
    let base = live & 0x0000_ffff_ffff_f000;
    if REPORTED.swap(true, Ordering::AcqRel) {
        return;
    }
    if asid != 0 && base != 0 {
        CPU_SEEN_PRIVATE_ROOT.store(true, Ordering::Release);
        log::info!(
            "S22-{}-DOMAIN-LIVE: PASS asid={} root={:#x} ttbr0={:#x} domain={} generation={}",
            ARCH_TAG,
            asid,
            base,
            live,
            id,
            generation
        );
    } else {
        log::error!(
            "S22-{}-DOMAIN-LIVE: FAIL asid={} ttbr0={:#x} domain={} generation={}",
            ARCH_TAG,
            asid,
            live,
            id,
            generation
        );
    }
}

/// The frames a torn-down domain held are accounted for, not withheld forever.
///
/// Called from `task::complete_incoming_switch` after the displaced root's staged
/// release and the safe-root acknowledge. That acknowledge is the trigger:
/// `domain_ack_generation` becoming non-zero is the published proof that this hart
/// *left* a private root, and it is set on the same path that releases the
/// outgoing root's pin.
///
/// A departure is not a teardown — a preempted domain still holds its root — so
/// the verdict waits for the release itself to be observable. The witness is the
/// ASID-targeted invalidation counter: retiring a root issues exactly one of them
/// (that is what `S22-AARCH64-RELEASE-FLUSH` asserts, with the all-context
/// counter as its live control), and no other post-boot path on this lane issues
/// one. The first departure after a private root was seen live records the
/// counter; the reading is taken at the first later departure that sees it move,
/// which is therefore after `AddressSpace::drop` ran.
///
/// `quarantine_frames` is the kernel's only sink for frames whose invalidation
/// was never acknowledged, so a zero count at that point is the fail-closed half
/// of the release contract: no frame was silently dropped and none was retained
/// forever. The liveness half — frames returning to the allocator, counted — is
/// the release path's own `DOMAIN-FRAME-RELEASE: PASS tag=… frames=… quarantined=…`
/// line, emitted from inside `AddressSpace::drop`.
#[cfg(target_arch = "aarch64")]
pub(crate) fn observe_domain_teardown(hart: usize) {
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// Targeted invalidations seen at the first departure after a live root.
    const UNSET: usize = usize::MAX;
    static BASELINE: AtomicUsize = AtomicUsize::new(UNSET);
    static REPORTED: AtomicBool = AtomicBool::new(false);

    if !CPU_SEEN_PRIVATE_ROOT.load(Ordering::Acquire) {
        return;
    }
    let ack_generation = hart_local::domain_ack_generation_for(hart);
    if ack_generation == 0 {
        return;
    }
    let (targeted, _) = crate::hal::domain::flush_kind_counters();
    let baseline = BASELINE.load(Ordering::Acquire);
    if baseline == UNSET {
        BASELINE.store(targeted, Ordering::Release);
        return;
    }
    let releases = targeted.wrapping_sub(baseline);
    if releases == 0 {
        return;
    }
    let quarantined = crate::memory::address_space::quarantined_frame_count();
    if REPORTED.swap(true, Ordering::AcqRel) {
        return;
    }
    if quarantined == 0 {
        log::info!(
            "S22-{}-DOMAIN-TEARDOWN: PASS releases={} quarantined={} ack_generation={}",
            ARCH_TAG,
            releases,
            quarantined,
            ack_generation
        );
    } else {
        log::error!(
            "S22-{}-DOMAIN-TEARDOWN: FAIL releases={} quarantined={} ack_generation={}",
            ARCH_TAG,
            releases,
            quarantined,
            ack_generation
        );
    }
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

// ─── Phase 02: AArch64 private-root leaf invalidation ───────────────────────
//
// These witnesses exist because a global leaf is immune to ASID-targeted
// invalidation: `tlbi aside1is` cannot retire it, and it stays usable after
// `TTBR0_EL1` is reprogrammed to another root. That is why every leaf a private
// root owns must carry `PTE_nG`, and why the release path can then be a
// tag-targeted flush instead of `vmalle1is`.

/// A page-aligned, zeroed frame from the kernel heap.
///
/// The witness must reach a private root's page tables **from inside that
/// root**, and the kernel heap is the one allocation arena every private root
/// already maps: it is registered as a shared `KernelHeap` range and
/// `phys_to_virt` is the identity on AArch64.
#[cfg(target_arch = "aarch64")]
fn fixture_heap_frame() -> Option<usize> {
    let layout = alloc::alloc::Layout::from_size_align(crate::memory::paging::PAGE_SIZE, crate::memory::paging::PAGE_SIZE)
        .ok()?;
    // SAFETY: the layout has a non-zero size, and the result is either null
    // (handled here) or a fresh 4 KiB-aligned block this fixture owns.
    let frame = unsafe { alloc::alloc::alloc_zeroed(layout) };
    (!frame.is_null()).then_some(frame as usize)
}

#[cfg(target_arch = "aarch64")]
fn free_fixture_heap_frame(frame: usize) {
    let layout = alloc::alloc::Layout::from_size_align(
        crate::memory::paging::PAGE_SIZE,
        crate::memory::paging::PAGE_SIZE,
    )
    .expect("fixture frame layout");
    // SAFETY: `frame` came from `fixture_heap_frame` under exactly this layout.
    unsafe { alloc::alloc::dealloc(frame as *mut u8, layout) };
}

/// Install one mapping in a fixture-built root, recording every intermediate
/// frame it allocates so the caller can identity-map them into the same root.
#[cfg(target_arch = "aarch64")]
fn map_fixture(
    root: *mut hal::PageTable,
    frames: &mut Vec<usize>,
    virtual_address: usize,
    physical_address: usize,
    flags: crate::memory::paging::Flags,
) -> bool {
    let mut allocate = || {
        let frame = fixture_heap_frame()?;
        frames.push(frame);
        Some(frame)
    };
    // SAFETY: `root` names a page table only this fixture mutates, and every
    // frame the closure returns is a live, zeroed 4 KiB heap block.
    unsafe { &mut *root }
        .map(virtual_address, physical_address, flags, &mut allocate)
        .is_ok()
}

/// (a) Every leaf that exists only in a private root is non-global, and the
/// shared kernel ranges are not.
///
/// Every production private-leaf path is exercised directly: the cell's user
/// stack and ELF segments (`map_existing_user_page`, exactly `create_cell_domain`'s
/// call), the cell's own kernel stack (`map_registered_execution`),
/// `map_private_page`, `map_grant_page`, and both stacks of the dynamic-thread
/// path (`map_existing_task_stacks`) — plus the shared `KernelHeap` range. The
/// kernel root's own leaf for the cell kernel stack is asserted present, because
/// trap entry reprograms `TTBR0_EL1` to the kernel root while still executing on
/// it: the private leaf is non-global *because* the SAS root carries the copy the
/// handler runs on.
#[cfg(target_arch = "aarch64")]
pub(crate) fn run_private_leaf_witness() -> bool {
    use crate::memory::address_space::{AddressSpaceBuilder, MappingKind};
    use crate::memory::domain_supervisor_registry::{shared_snapshot, SupervisorRangeKind};
    use crate::memory::paging::Flags;

    let Ok(kernel_stack) = crate::task::stack::Stack::new_kernel(1) else {
        log::error!("S22-{}-LEAF-NONG: FAIL stack", ARCH_TAG);
        return false;
    };
    let user_leaf_va = 0x0010_0000usize;
    let private_leaf_va = 0x0010_1000usize;
    let grant_leaf_va = 0x0010_2000usize;
    let Some(shared_va) = shared_snapshot()
        .iter()
        .find(|range| range.kind == SupervisorRangeKind::KernelHeap)
        .map(|range| range.start)
    else {
        log::error!("S22-{}-LEAF-NONG: FAIL no shared kernel range", ARCH_TAG);
        return false;
    };
    let Some(grant_backing) = fixture_heap_frame() else {
        log::error!("S22-{}-LEAF-NONG: FAIL grant backing frame", ARCH_TAG);
        return false;
    };
    let Some(image_backing) = fixture_heap_frame() else {
        free_fixture_heap_frame(grant_backing);
        log::error!("S22-{}-LEAF-NONG: FAIL image backing frame", ARCH_TAG);
        return false;
    };
    // A second stack pair for the dynamic-thread path
    // (`map_existing_task_stacks`), which the scheduler uses when a cell spawns
    // a thread after admission.
    let (Ok(thread_kernel_stack), Ok(thread_user_stack)) =
        (crate::task::stack::Stack::new_kernel(2), crate::task::stack::Stack::new_user(2))
    else {
        free_fixture_heap_frame(image_backing);
        free_fixture_heap_frame(grant_backing);
        log::error!("S22-{}-LEAF-NONG: FAIL thread stacks", ARCH_TAG);
        return false;
    };

    let mut builder = AddressSpaceBuilder::new();
    builder.map_registered_execution(&kernel_stack);
    // Exactly what `create_cell_domain` does for a cell's user stack and every
    // ELF segment.
    let user_requested = builder
        .map_existing_user_page(
            user_leaf_va,
            image_backing,
            MappingKind::Private,
            Flags::from_bits(Flags::READ | Flags::WRITE),
        )
        .is_ok();
    let Ok(address_space) = builder.build() else {
        free_fixture_heap_frame(image_backing);
        free_fixture_heap_frame(grant_backing);
        log::error!("S22-{}-LEAF-NONG: FAIL build", ARCH_TAG);
        return false;
    };
    let private_mapped = address_space
        .map_private_page(
            private_leaf_va,
            MappingKind::Private,
            Flags::from_bits(Flags::READ | Flags::WRITE),
        )
        .is_ok();
    let grant_mapped = address_space
        .map_grant_page(
            grant_leaf_va,
            grant_backing,
            Flags::from_bits(Flags::READ | Flags::WRITE),
        )
        .is_ok();
    let thread_stacks_mapped = address_space
        .map_existing_task_stacks(&thread_kernel_stack, &thread_user_stack)
        .is_ok();

    // SAFETY: the root frame belongs to `address_space` and outlives this walk.
    let private_root = unsafe {
        &*(crate::memory::frame::phys_to_virt(address_space.root_ppn() << 12)
            as *const hal::PageTable)
    };
    let private_leaf = |virtual_address| private_root.leaf_entry(virtual_address);
    let kernel_root_leaf = *crate::memory::paging::KERNEL_ROOT.lock();
    // SAFETY: KERNEL_ROOT names the live kernel page table.
    let kernel_root = kernel_root_leaf.map(|root| unsafe {
        &*(crate::memory::frame::phys_to_virt(root) as *const hal::PageTable)
    });

    let user_leaf = private_leaf(user_leaf_va);
    let private_leaf_word = private_leaf(private_leaf_va);
    let grant_leaf = private_leaf(grant_leaf_va);
    let kernel_stack_leaf = private_leaf(kernel_stack.usable_start());
    let thread_kernel_stack_leaf = private_leaf(thread_kernel_stack.usable_start());
    let thread_user_stack_leaf = private_leaf(thread_user_stack.usable_start());
    let shared_leaf = private_leaf(shared_va);
    let kernel_root_kernel_stack =
        kernel_root.and_then(|root| root.leaf_entry(kernel_stack.usable_start()));

    let non_global = crate::hal::paging::leaf_is_non_global;
    let checks = [
        (user_requested && user_leaf.map(non_global) == Some(true)),
        (private_mapped && private_leaf_word.map(non_global) == Some(true)),
        (grant_mapped && grant_leaf.map(non_global) == Some(true)),
        (kernel_stack_leaf.map(non_global) == Some(true)),
        (thread_stacks_mapped && thread_kernel_stack_leaf.map(non_global) == Some(true)),
        (thread_stacks_mapped && thread_user_stack_leaf.map(non_global) == Some(true)),
        (shared_leaf.map(non_global) == Some(false)),
        (kernel_root_kernel_stack.is_some()),
    ];
    let ok = checks.iter().all(|passed| *passed);
    if ok {
        log::info!(
            "S22-{}-LEAF-NONG: PASS user={:#x} private={:#x} grant={:#x} kstack={:#x} thread_kstack={:#x} thread_ustack={:#x} shared={:#x} kernel_root_kstack=present",
            ARCH_TAG,
            user_leaf.unwrap_or(0),
            private_leaf_word.unwrap_or(0),
            grant_leaf.unwrap_or(0),
            kernel_stack_leaf.unwrap_or(0),
            thread_kernel_stack_leaf.unwrap_or(0),
            thread_user_stack_leaf.unwrap_or(0),
            shared_leaf.unwrap_or(0),
        );
    } else {
        log::error!(
            "S22-{}-LEAF-NONG: FAIL user={:?} private={:?} grant={:?} kstack={:?} thread_kstack={:?} thread_ustack={:?} shared={:?} kernel_root_kstack={:?}",
            ARCH_TAG,
            user_leaf,
            private_leaf_word,
            grant_leaf,
            kernel_stack_leaf,
            thread_kernel_stack_leaf,
            thread_user_stack_leaf,
            shared_leaf,
            kernel_root_kernel_stack,
        );
    }
    drop(address_space);
    free_fixture_heap_frame(image_backing);
    free_fixture_heap_frame(grant_backing);
    ok
}

/// (c) A released private root is invalidated by its **tag**, not by every
/// context.
///
/// `AddressSpace::drop` runs the real release path for the root it retires; the
/// backend counts a targeted `tlbi` and an all-context one separately. The
/// all-context counter is then moved by an explicit `flush_all`, so "zero full
/// flushes" cannot be satisfied by a counter nobody increments.
#[cfg(target_arch = "aarch64")]
pub(crate) fn run_release_flush_witness() -> bool {
    use crate::memory::address_space::AddressSpaceBuilder;

    let Ok(kernel_stack) = crate::task::stack::Stack::new_kernel(1) else {
        log::error!("S22-{}-RELEASE-FLUSH: FAIL stack", ARCH_TAG);
        return false;
    };
    let (targeted_before, full_before) = crate::hal::domain::flush_kind_counters();
    let mut builder = AddressSpaceBuilder::new();
    builder.map_registered_execution(&kernel_stack);
    match builder.build() {
        Ok(address_space) => drop(address_space),
        Err(error) => {
            log::error!("S22-{}-RELEASE-FLUSH: FAIL build={:?}", ARCH_TAG, error);
            return false;
        }
    }
    let (targeted_after, full_after) = crate::hal::domain::flush_kind_counters();
    let released_targeted = targeted_after == targeted_before + 1;
    let released_no_full = full_after == full_before;
    // Live control: the full-flush counter must be able to move at all.
    crate::hal::domain::flush_all();
    let (_, full_control) = crate::hal::domain::flush_kind_counters();
    let full_counter_live = full_control == full_after + 1;
    let ok = released_targeted && released_no_full && full_counter_live;
    if ok {
        log::info!(
            "S22-{}-RELEASE-FLUSH: PASS targeted={} full={} targeted_delta=1 full_delta=0 control_live=true",
            ARCH_TAG,
            targeted_after,
            full_after,
        );
    } else {
        log::error!(
            "S22-{}-RELEASE-FLUSH: FAIL targeted {}->{} full {}->{} control {}->{}",
            ARCH_TAG,
            targeted_before,
            targeted_after,
            full_before,
            full_after,
            full_after,
            full_control,
        );
    }
    ok
}

/// (b) The behavioural witness the global design cannot pass.
///
/// Inside one private root under one ASID: read `VA` (which caches the
/// translation), rewrite that same leaf to a second frame, run the production
/// ASID-targeted invalidation, and read `VA` again. With a non-global leaf the
/// invalidation reaches it and the second read observes the new frame; with the
/// previous global composition the stale entry survives and it reads the old
/// frame.
///
/// The control VA carries the identical leaf *without* `PTE_nG` and runs the
/// same sequence: it must keep observing the stale frame. A second check runs
/// first — a flush for a **different** leased tag must leave this tag's entry
/// alone. Only when both hold can the two reads distinguish the designs; when
/// they do not, the verdict is `UNPROVEN` with the environment's exact failure,
/// never `PASS`. (QEMU 8.2.2 fails the first: its `tlbi aside1is` retires an
/// unrelated tag's entry too, so it cannot separate a global leaf from a
/// non-global one.)
///
/// No root register is written between the two reads: the remap happens from
/// inside the root (its own tables are heap frames, identity-mapped into it), so
/// a TTBR0 write that flushes the TLB cannot mask the result.
#[cfg(target_arch = "aarch64")]
pub(crate) fn run_asid_invalidation_witness() -> bool {
    use crate::memory::address_space::FixtureAsidLease;
    use crate::memory::domain_supervisor_registry::{shared_snapshot, SupervisorRangeKind};
    use crate::memory::paging::{Flags, PAGE_SIZE};
    use core::cell::UnsafeCell;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    const WITNESS_VA: usize = 0x0010_0000;
    const CONTROL_VA: usize = 0x0010_1000;
    const WITNESS_OLD: u32 = 0xA1A1_0001;
    const WITNESS_NEW: u32 = 0xB2B2_0002;
    const CONTROL_OLD: u32 = 0xC3C3_0001;
    const CONTROL_NEW: u32 = 0xD4D4_0002;

    struct Slot(UnsafeCell<crate::hal::arch::Context>);
    // SAFETY: one hart, interrupts disabled for the whole witness, and the
    // switch routine is the only writer.
    unsafe impl Sync for Slot {}

    static CTX_ENTRY: Slot = Slot(UnsafeCell::new(crate::hal::arch::Context::zeroed()));
    static CTX_FIXTURE: Slot = Slot(UnsafeCell::new(crate::hal::arch::Context::zeroed()));
    static ROOT_ADDR: AtomicUsize = AtomicUsize::new(0);
    static ASID: AtomicUsize = AtomicUsize::new(0);
    static FOREIGN_ASID: AtomicUsize = AtomicUsize::new(0);
    static KERNEL_BASE: AtomicUsize = AtomicUsize::new(0);
    static WITNESS_NEW_PA: AtomicUsize = AtomicUsize::new(0);
    static CONTROL_NEW_PA: AtomicUsize = AtomicUsize::new(0);
    static WITNESS_FLAGS: AtomicUsize = AtomicUsize::new(0);
    static CONTROL_FLAGS: AtomicUsize = AtomicUsize::new(0);
    static REMAP_OK: AtomicBool = AtomicBool::new(false);
    static ENTERED: AtomicBool = AtomicBool::new(false);
    static WITNESS_BEFORE: AtomicUsize = AtomicUsize::new(0);
    static WITNESS_AFTER_FOREIGN: AtomicUsize = AtomicUsize::new(0);
    static WITNESS_AFTER: AtomicUsize = AtomicUsize::new(0);
    static CONTROL_BEFORE: AtomicUsize = AtomicUsize::new(0);
    static CONTROL_AFTER: AtomicUsize = AtomicUsize::new(0);

    /// Runs under the fixture root, on the fixture stack, and touches nothing
    /// but statics — a frame on the outgoing stack is unreachable once this root
    /// is live.
    extern "C" fn entry() {
        // SAFETY: both VAs are mapped by the root this entry is running under.
        let read = |virtual_address: usize| unsafe {
            core::ptr::read_volatile(virtual_address as *const u32) as usize
        };
        // Cache both translations under this ASID.
        WITNESS_BEFORE.store(read(WITNESS_VA), Ordering::Release);
        CONTROL_BEFORE.store(read(CONTROL_VA), Ordering::Release);
        // Rewrite both leaves in place: same root, same ASID, no flush yet.
        let root = ROOT_ADDR.load(Ordering::Acquire) as *mut hal::PageTable;
        let mut no_allocate = || -> Option<usize> { None };
        let witness_ok = unsafe {
            (*root).map(
                WITNESS_VA,
                WITNESS_NEW_PA.load(Ordering::Acquire),
                Flags::from_bits(WITNESS_FLAGS.load(Ordering::Acquire)),
                &mut no_allocate,
            )
        }
        .is_ok();
        let control_ok = unsafe {
            (*root).map(
                CONTROL_VA,
                CONTROL_NEW_PA.load(Ordering::Acquire),
                Flags::from_bits(CONTROL_FLAGS.load(Ordering::Acquire)),
                &mut no_allocate,
            )
        }
        .is_ok();
        REMAP_OK.store(witness_ok && control_ok, Ordering::Release);
        // A flush for a DIFFERENT tag must not retire this tag's translations.
        // This is the discrimination check the environment has to pass before the
        // two reads below can distinguish a global leaf from a non-global one: if
        // a foreign tag's invalidation clears this entry, the environment is not
        // modelling ASID scoping at all.
        crate::hal::domain::flush_asid(FOREIGN_ASID.load(Ordering::Acquire));
        WITNESS_AFTER_FOREIGN.store(read(WITNESS_VA), Ordering::Release);
        // The production invalidation for a retiring private tag.
        crate::hal::domain::flush_asid(ASID.load(Ordering::Acquire));
        WITNESS_AFTER.store(read(WITNESS_VA), Ordering::Release);
        CONTROL_AFTER.store(read(CONTROL_VA), Ordering::Release);
        ENTERED.store(true, Ordering::Release);
        // Back to the fixture context, under the kernel root.
        unsafe {
            crate::hal::arch::Context::switch_with_root(
                CTX_ENTRY.0.get(),
                CTX_FIXTURE.0.get(),
                KERNEL_BASE.load(Ordering::Acquire),
                0,
            );
        }
        loop {
            core::hint::spin_loop();
        }
    }

    let Some(tag) = FixtureAsidLease::acquire(91_004) else {
        log::error!("S22-{}-ASID-INVALIDATION: FAIL tag", ARCH_TAG);
        return false;
    };
    // A second leased tag, so "a foreign tag" is a tag no live root can hold.
    let Some(foreign_tag) = FixtureAsidLease::acquire(91_005) else {
        log::error!("S22-{}-ASID-INVALIDATION: FAIL foreign tag", ARCH_TAG);
        return false;
    };
    let kernel_root = crate::hal::domain::kernel_ttbr0();
    let Some(root) = fixture_heap_frame() else {
        log::error!("S22-{}-ASID-INVALIDATION: FAIL root frame", ARCH_TAG);
        return false;
    };
    // SAFETY: fresh zeroed 4 KiB block, used only as a page table by this fixture.
    unsafe { core::ptr::write(root as *mut hal::PageTable, hal::PageTable::empty()) };
    let mut frames = Vec::new();
    frames.push(root);

    let code_flags = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::EXECUTE | Flags::ACCESSED | Flags::DIRTY,
    );
    let witness_flags = Flags::from_bits(
        Flags::VALID
            | Flags::READ
            | Flags::WRITE
            | Flags::USER
            | Flags::ACCESSED
            | Flags::DIRTY
            | Flags::NON_GLOBAL,
    );
    let control_flags = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::WRITE | Flags::USER | Flags::ACCESSED | Flags::DIRTY,
    );

    let mut ok = kernel_root != 0;
    // The entry's code and statics: the fixture root has no other reason to map
    // the kernel image, and it never logs from inside itself.
    for range in shared_snapshot().iter().filter(|range| {
        matches!(
            range.kind,
            SupervisorRangeKind::StaticText
                | SupervisorRangeKind::StaticReadOnly
                | SupervisorRangeKind::StaticWritable
        )
    }) {
        for virtual_address in (range.start..range.end).step_by(PAGE_SIZE) {
            ok &= map_fixture(
                root as *mut hal::PageTable,
                &mut frames,
                virtual_address,
                virtual_address,
                code_flags,
            );
        }
    }

    let (
        Some(witness_old_frame),
        Some(witness_new_frame),
        Some(control_old_frame),
        Some(control_new_frame),
        Some(stack),
    ) = (
        fixture_heap_frame(),
        fixture_heap_frame(),
        fixture_heap_frame(),
        fixture_heap_frame(),
        fixture_heap_frame(),
    )
    else {
        log::error!("S22-{}-ASID-INVALIDATION: FAIL data frames", ARCH_TAG);
        return false;
    };
    for frame in [
        witness_old_frame,
        witness_new_frame,
        control_old_frame,
        control_new_frame,
        stack,
    ] {
        frames.push(frame);
    }
    // SAFETY: all five are live heap frames owned by this fixture.
    unsafe {
        core::ptr::write_volatile(witness_old_frame as *mut u32, WITNESS_OLD);
        core::ptr::write_volatile(witness_new_frame as *mut u32, WITNESS_NEW);
        core::ptr::write_volatile(control_old_frame as *mut u32, CONTROL_OLD);
        core::ptr::write_volatile(control_new_frame as *mut u32, CONTROL_NEW);
    }
    ok &= map_fixture(
        root as *mut hal::PageTable,
        &mut frames,
        WITNESS_VA,
        witness_old_frame,
        witness_flags,
    );
    ok &= map_fixture(
        root as *mut hal::PageTable,
        &mut frames,
        CONTROL_VA,
        control_old_frame,
        control_flags,
    );
    // Every frame this fixture allocated, identity-mapped into the fixture root:
    // the remap below runs *inside* that root and walks the tables by physical
    // address, and on AArch64 those addresses are their identity VAs. The list
    // grows while these very mappings allocate their own intermediate tables.
    let mut index = 0;
    while index < frames.len() {
        let frame = frames[index];
        index += 1;
        ok &= map_fixture(
            root as *mut hal::PageTable,
            &mut frames,
            frame,
            frame,
            code_flags,
        );
    }

    let verdict = if !ok {
        log::error!("S22-{}-ASID-INVALIDATION: FAIL fixture-map", ARCH_TAG);
        false
    } else {
        let saved_daif = crate::hal::arch::save_and_disable_interrupts();
        ROOT_ADDR.store(root, Ordering::Release);
        ASID.store(tag.value(), Ordering::Release);
        FOREIGN_ASID.store(foreign_tag.value(), Ordering::Release);
        KERNEL_BASE.store(kernel_root & 0x0000_ffff_ffff_f000, Ordering::Release);
        WITNESS_NEW_PA.store(witness_new_frame, Ordering::Release);
        CONTROL_NEW_PA.store(control_new_frame, Ordering::Release);
        WITNESS_FLAGS.store(witness_flags.bits(), Ordering::Release);
        CONTROL_FLAGS.store(control_flags.bits(), Ordering::Release);
        // SAFETY: the entry stack is a live heap frame mapped into the root, and
        // the context is a static this fixture owns.
        unsafe {
            let entry_context = &mut *CTX_ENTRY.0.get();
            entry_context.sp = (stack + PAGE_SIZE) as u64;
            entry_context.x30 = entry as *const () as usize as u64;
            entry_context.daif = saved_daif as u64;
        }
        // SAFETY: both context slots are valid, and `root`/`tag.value()` name the
        // root this fixture built and holds.
        unsafe {
            crate::hal::arch::Context::switch_with_root(
                CTX_FIXTURE.0.get(),
                CTX_ENTRY.0.get(),
                root,
                tag.value(),
            );
        }
        // SAFETY: restores the DAIF value this witness saved before disabling
        // interrupts, in the same context it was taken.
        unsafe {
            crate::hal::arch::restore_sstatus(saved_daif);
        }

        let entered = ENTERED.load(Ordering::Acquire);
        let remapped = REMAP_OK.load(Ordering::Acquire);
        let witness_before = WITNESS_BEFORE.load(Ordering::Acquire);
        let witness_after_foreign = WITNESS_AFTER_FOREIGN.load(Ordering::Acquire);
        let witness_after = WITNESS_AFTER.load(Ordering::Acquire);
        let control_before = CONTROL_BEFORE.load(Ordering::Acquire);
        let control_after = CONTROL_AFTER.load(Ordering::Acquire);
        // The entry the witness reads twice must survive a foreign tag's
        // invalidation. Without that, an ASID-targeted flush in this environment
        // is a full flush and neither read can say anything about nG.
        let asid_scoped = witness_after_foreign == WITNESS_OLD as usize;
        let non_global_reached = witness_before == WITNESS_OLD as usize
            && witness_after == WITNESS_NEW as usize;
        let global_kept_stale = control_before == CONTROL_OLD as usize
            && control_after == CONTROL_OLD as usize;
        if !entered || !remapped || witness_before != WITNESS_OLD as usize || control_before != CONTROL_OLD as usize
        {
            log::error!(
                "S22-{}-ASID-INVALIDATION: FAIL entered={} remap_ok={} witness_before={:#x} control_before={:#x}",
                ARCH_TAG,
                entered,
                remapped,
                witness_before,
                control_before,
            );
            false
        } else if asid_scoped && non_global_reached && global_kept_stale {
            log::info!(
                "S22-{}-ASID-INVALIDATION: PASS witnessed={:#x}->{:#x} control={:#x}->{:#x} asid_scoped=true",
                ARCH_TAG,
                witness_before,
                witness_after,
                control_before,
                control_after,
            );
            true
        } else if !asid_scoped {
            // A foreign tag's `tlbi` retired this tag's entry: the environment
            // does not model ASID scoping, so it cannot observe the difference.
            log::warn!(
                "S22-{}-ASID-INVALIDATION: UNPROVEN witnessed={:#x}->{:#x} after_foreign={:#x} control={:#x}->{:#x} environment_asid_flush_unscoped=true",
                ARCH_TAG,
                witness_before,
                witness_after,
                witness_after_foreign,
                control_before,
                control_after,
            );
            true
        } else if non_global_reached && !global_kept_stale {
            // Scoping works, but the global control leaf was invalidated by an
            // ASID op too, so the nG bit is not what made the difference here.
            log::warn!(
                "S22-{}-ASID-INVALIDATION: UNPROVEN witnessed={:#x}->{:#x} control={:#x}->{:#x} environment_ignores_global_bit=true",
                ARCH_TAG,
                witness_before,
                witness_after,
                control_before,
                control_after,
            );
            true
        } else {
            log::error!(
                "S22-{}-ASID-INVALIDATION: FAIL witnessed={:#x}->{:#x} after_foreign={:#x} control={:#x}->{:#x}",
                ARCH_TAG,
                witness_before,
                witness_after,
                witness_after_foreign,
                control_before,
                control_after,
            );
            false
        }
    };

    // Release the tags (each runs the ASID-targeted invalidation) before any
    // frame goes back to the allocator.
    drop(foreign_tag);
    drop(tag);
    for frame in frames {
        free_fixture_heap_frame(frame);
    }
    verdict
}
