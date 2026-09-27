//! Scheduler-owned root transition plan. Compiled for every architecture that
//! can carry a private root; the tuple encoding is arch-specific and lives in
//! [`root_tuple`].
use super::{
    hart_local,
    tcb::{Task, TaskAddressSpace},
};
use crate::hal::arch::Context;
use crate::memory::address_space::AddressSpace;
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
pub(crate) struct SwitchPlan {
    pub outgoing: *mut Context,
    pub incoming: *const Context,
    transition: DomainTransition,
}

/// The root tuple the switch routine programs for `space` on this architecture.
///
/// RV64 takes a page number; the others take the byte address the tables start
/// at. The encoding lives here so a plan, the assembly boundary, and the
/// fixtures cannot disagree about it.
pub(crate) fn root_tuple(space: &AddressSpace) -> (usize, usize) {
    #[cfg(target_arch = "riscv64")]
    {
        (space.root_ppn(), space.asid())
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        (space.root_ppn() << 12, space.asid())
    }
}

/// The kernel root in the same encoding, ASID 0 (untagged).
fn kernel_root_tuple() -> (usize, usize) {
    let root = crate::memory::paging::KERNEL_ROOT
        .lock()
        .expect("native domain requires SAS root");
    #[cfg(target_arch = "riscv64")]
    {
        (root >> 12, 0)
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        (root, 0)
    }
}

#[derive(Clone)]
enum DomainTransition {
    SasToSas,
    Activate(DomainRef),
    SameDomain(DomainRef),
    ToSafeRoot,
}

impl SwitchPlan {
    pub(crate) fn new(
        outgoing: *mut Context,
        incoming: *const Context,
        task: Option<&Task>,
    ) -> Option<Self> {
        // No liveness gate here: the execution pin was already acquired in
        // pick_next_local's filter, before the task was dequeued or attributed,
        // so a plan that reaches this point MUST proceed — returning `None`
        // would strand a selected task. A retirement flipping the pinned root
        // Dying after acquisition is legal and drain-safe.
        let transition = match task.and_then(DomainRef::from_task) {
            Some(domain) if hart_local::current_domain() == domain.tuple() => {
                DomainTransition::SameDomain(domain)
            }
            Some(domain) => {
                hart_local::advance_execution_pin(Some(Arc::clone(&domain.0)));
                DomainTransition::Activate(domain)
            }
            None if hart_local::current_domain().0 == 0 => DomainTransition::SasToSas,
            None => {
                // Leaving the pinned private root: hand its release to the
                // safe-root completion hook alongside outgoing attribution.
                hart_local::advance_execution_pin(None);
                DomainTransition::ToSafeRoot
            }
        };
        Some(Self {
            outgoing,
            incoming,
            transition,
        })
    }

    /// Return the root switch that assembly must issue after saving `outgoing`.
    /// A zero PPN is the explicit no-write path, and it now means exactly one
    /// thing: SAS to SAS, where the kernel root is already live.
    pub(crate) fn root_switch(&self) -> (usize, usize) {
        match &self.transition {
            DomainTransition::Activate(domain) => {
                let (id, generation) = domain.tuple();
                let root = root_tuple(&domain.0);
                // The execution pin was acquired at selection time under
                // scheduler-stable state; programming the dead root's SATP here
                // is exactly what begin_execution's recheck prevents.
                hart_local::set_current_domain(id, generation);
                crate::hal::domain::observe_switch_activation();
                #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
                log::info!(
                    "S22-RV64-SWITCH: PASS harts={}",
                    super::smp::online_hart_count()
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
                root_tuple(&domain.0)
            }
            DomainTransition::ToSafeRoot => {
                hart_local::mark_safe_root_pending();
                crate::hal::domain::observe_switch_activation();
                kernel_root_tuple()
            }
            DomainTransition::SasToSas => (0, 0),
        }
    }
}
