//! Scheduler-owned root decision. Compiled for every architecture that can
//! carry a private root; the root encoding is the architecture's
//! (`crate::arch::sched::root_for_table`).
use crate::arch::sched::{IrqSaved, Root, SwitchPlan};
use crate::hal::arch::Context;
use crate::memory::address_space::AddressSpace;
use crate::sched::hart_local;
use crate::task::tcb::{Task, TaskAddressSpace};
use alloc::sync::Arc;

#[derive(Clone)]
pub(crate) struct DomainRef(Arc<AddressSpace>);

impl DomainRef {
    /// No liveness re-check here: the execution pin acquired in pick_next_local
    /// makes a later retire() legal and drain-safe, so a pinned task's plan
    /// MUST still derive Activate instead of diverting to the safe root under
    /// KERNEL_ROOT with none of its mappings.
    fn from_task(task: &Task) -> Option<Self> {
        match &task.address_space {
            TaskAddressSpace::Sas => None,
            TaskAddressSpace::Domain(space) => Some(Self(Arc::clone(space))),
        }
    }

    pub(crate) fn tuple(&self) -> (u64, u64) {
        (self.0.identity().raw(), self.0.generation())
    }

    #[allow(dead_code)]
    pub(crate) fn from_address_space(space: &Arc<AddressSpace>) -> Self {
        Self(Arc::clone(space))
    }

    #[allow(dead_code)]
    pub(crate) fn address_space(&self) -> &Arc<AddressSpace> {
        &self.0
    }
}

/// A root decision made under `SCHEDULER`, carried to the raw-switch boundary.
/// No assembly code is allowed to rediscover task or root ownership.
///
/// The decision's root side effects (publishing the domain identity, staging
/// the safe-root acknowledge) run only in [`RootDecision::switch`], on the way
/// into the switch that programs the root: nothing can record a domain it did
/// not program. Fixtures inspect a decision through [`RootDecision::preview`].
pub(crate) struct RootDecision {
    pub outgoing: *mut Context,
    pub incoming: *const Context,
    transition: DomainTransition,
}

/// The root the switch programs for `space`.
pub(crate) fn root_of(space: &AddressSpace) -> Option<Root> {
    crate::arch::sched::root_for_table(space.root_ppn() << 12, space.asid())
}

/// The kernel root, ASID 0 (untagged).
fn kernel_root() -> Option<Root> {
    let root = crate::memory::paging::KERNEL_ROOT
        .lock()
        .expect("native domain requires SAS root");
    crate::arch::sched::root_for_table(root, 0)
}

#[derive(Clone)]
enum DomainTransition {
    SasToSas,
    Activate(DomainRef),
    SameDomain(DomainRef),
    ToSafeRoot,
}

impl DomainTransition {
    /// The transition from the hart's published domain `current` to `domain`.
    fn classify(current: (u64, u64), domain: Option<DomainRef>) -> Self {
        match domain {
            Some(domain) if current == domain.tuple() => Self::SameDomain(domain),
            Some(domain) => Self::Activate(domain),
            None if current.0 == 0 => Self::SasToSas,
            None => Self::ToSafeRoot,
        }
    }
}

impl RootDecision {
    pub(crate) fn new(
        outgoing: *mut Context,
        incoming: *const Context,
        task: Option<&Task>,
    ) -> Option<Self> {
        // Test posture: a Context the scheduler never primed (no kernel stack,
        // sp 0) must not reach the switch, which would load sp = 0 and surface
        // two faults later as a misleading kernel-stack overflow.
        #[cfg(feature = "test-hooks")]
        if !incoming.is_null() {
            // SAFETY: a non-null incoming pointer names the selected task's
            // Context, stable under `SCHEDULER`.
            let sp = unsafe { (*incoming).sp };
            assert!(
                sp != 0 && task.is_none_or(|task| task.kernel_stack.is_some()),
                "[sched] dispatch of an unprimed Context: tid={} hart={} sp={:#x} kernel_stack={}",
                task.map_or(0, |task| task.id),
                hart_local::current_hart_id(),
                sp,
                task.is_some_and(|task| task.kernel_stack.is_some()),
            );
        }
        // No liveness gate here: the execution pin was already acquired in
        // pick_next_local's filter, before the task was dequeued or attributed,
        // so a plan that reaches this point MUST proceed — returning `None`
        // would strand a selected task. A retirement flipping the pinned root
        // Dying after acquisition is legal and drain-safe.
        let domain = task.and_then(DomainRef::from_task);
        let transition = DomainTransition::classify(hart_local::current_domain(), domain);
        match &transition {
            DomainTransition::Activate(domain) => {
                hart_local::advance_execution_pin(Some(Arc::clone(&domain.0)));
            }
            // Leaving the pinned private root: hand its release to the
            // safe-root completion hook alongside outgoing attribution.
            DomainTransition::ToSafeRoot => hart_local::advance_execution_pin(None),
            DomainTransition::SasToSas | DomainTransition::SameDomain(_) => {}
        }
        Some(Self {
            outgoing,
            incoming,
            transition,
        })
    }

    /// Run the root side effects and return the root the switch must program
    /// after saving `outgoing`. `None` is the explicit no-write path, and it
    /// means exactly one thing: SAS to SAS, where the kernel root is already
    /// live.
    ///
    /// Consumes the decision so its root reference is released before the raw
    /// switch. The outgoing task's stack frame never unwinds when that task has
    /// exited, so a decision still alive across the switch would leak one
    /// reference to the incoming root and keep that domain (its tables and
    /// ledger) alive forever. The hart's execution pin, not the decision, keeps
    /// the programmed root alive past this point.
    fn commit(self) -> Option<Root> {
        match self.transition {
            DomainTransition::Activate(domain) => {
                let (id, generation) = domain.tuple();
                let root = root_of(&domain.0);
                // The execution pin was acquired at selection time under
                // scheduler-stable state; programming the dead root's SATP here
                // is exactly what begin_execution's recheck prevents.
                hart_local::set_current_domain(id, generation);
                crate::hal::domain::observe_switch_activation();
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::root_programming();
                #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
                log::info!(
                    "S22-RV64-SWITCH: PASS harts={}",
                    crate::sched::smp::online_hart_count()
                );
                root
            }
            DomainTransition::SameDomain(domain) => {
                // Same-domain resume must still program the root. Trap entry
                // installs the kernel root, so "same domain" no longer implies
                // "the root is already live"; skipping the write would resume a
                // Cell under the kernel root with the kernel's mappings visible
                // to its S-mode code — a silent isolation failure.
                crate::hal::domain::observe_switch_activation();
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::root_programming();
                root_of(&domain.0)
            }
            DomainTransition::ToSafeRoot => {
                hart_local::mark_safe_root_pending();
                crate::hal::domain::observe_switch_activation();
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::root_programming();
                kernel_root()
            }
            DomainTransition::SasToSas => None,
        }
    }

    /// Switch from `out` to `inc` (the selection's contexts, with the hart's
    /// boot context in place of a null one), programming this decision's root
    /// between the outgoing save and the incoming load.
    ///
    /// Returns this activation's interrupt token once it resumes.
    ///
    /// # Safety
    /// `out` and `inc` must be valid context slots for this hart, and `irq`
    /// must be the token captured before selection on this hart.
    pub(crate) unsafe fn switch(
        self,
        out: *mut Context,
        inc: *const Context,
        irq: IrqSaved,
    ) -> IrqSaved {
        let root = self.commit();
        #[cfg(feature = "test-hooks")]
        rootless::note(root.is_none());
        // The root is programmed *inside* the switch, after the outgoing
        // context is saved and before the incoming stack is adopted.
        // Programming it here instead would leave the incoming root live while
        // the outgoing context is still being saved on a stack the new root
        // does not map.
        // SAFETY: the caller's contract; the root comes from a decision made
        // under `SCHEDULER` with the execution pin held.
        unsafe {
            crate::arch::sched::switch(SwitchPlan {
                out,
                inc,
                root,
                irq,
            })
        }
    }

    /// The pre-`SwitchPlan` entry: the root side effects with the tuple the
    /// old switch call took. Built only for the Phase 14 A/B fixture.
    #[cfg(cellos_switch_legacy)]
    pub(crate) fn root_switch(self) -> (usize, usize) {
        self.commit()
            .map_or((0, 0), |root| (root.base(), root.asid()))
    }

    /// What a switch for `task` would program on a hart whose published domain
    /// is `current`, with none of the decision's side effects: no execution
    /// pin, no published identity.
    #[cfg(feature = "test-hooks")]
    pub(crate) fn preview(current: (u64, u64), task: Option<&Task>) -> Preview {
        let transition = DomainTransition::classify(current, task.and_then(DomainRef::from_task));
        match transition {
            DomainTransition::Activate(domain) => Preview {
                root: root_of(&domain.0),
                publishes: Some(domain.tuple()),
            },
            DomainTransition::SameDomain(domain) => Preview {
                root: root_of(&domain.0),
                publishes: None,
            },
            DomainTransition::ToSafeRoot => Preview {
                root: kernel_root(),
                publishes: None,
            },
            DomainTransition::SasToSas => Preview {
                root: None,
                publishes: None,
            },
        }
    }
}

/// A side-effect-free view of a root decision ([`RootDecision::preview`]).
#[cfg(feature = "test-hooks")]
pub(crate) struct Preview {
    /// The root the switch would program; `None` is the no-write path.
    pub root: Option<Root>,
    /// The domain identity the switch would publish (Activate only).
    pub publishes: Option<(u64, u64)>,
}

/// Test posture: a switch that programmed no root must leave no domain
/// recorded on its hart. [`RootDecision::switch`] notes a rootless switch; the
/// incoming-side completion hook checks it.
#[cfg(feature = "test-hooks")]
mod rootless {
    use crate::sched::hart_local;
    use core::sync::atomic::{AtomicBool, Ordering};

    static PENDING: [AtomicBool; crate::sched::smp::MAX_HARTS] =
        [const { AtomicBool::new(false) }; crate::sched::smp::MAX_HARTS];

    pub(super) fn note(rootless: bool) {
        if let Some(slot) = PENDING.get(hart_local::current_hart_id()) {
            slot.store(rootless, Ordering::Release);
        }
    }

    /// Called by `vi_context_switch_complete` before the safe-root acknowledge.
    pub(crate) fn check_completion(hart: usize) {
        if PENDING
            .get(hart)
            .is_some_and(|slot| slot.swap(false, Ordering::AcqRel))
        {
            let (id, generation) = hart_local::current_domain();
            assert!(
                id == 0,
                "[sched] hart {} completed a switch that programmed no root with domain ({}, {}) recorded",
                hart,
                id,
                generation,
            );
        }
    }
}

#[cfg(feature = "test-hooks")]
pub(crate) use rootless::check_completion as check_rootless_completion;
