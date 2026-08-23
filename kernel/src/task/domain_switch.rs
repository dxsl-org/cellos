//! Scheduler-owned root transition plan.  This module is RV64-only by construction.
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
    fn from_task(task: &Task) -> Option<Self> {
        match &task.address_space {
            TaskAddressSpace::Sas => None,
            TaskAddressSpace::Domain(space) if space.is_live() => Some(Self(Arc::clone(space))),
            TaskAddressSpace::Domain(_) => None,
        }
    }

    fn tuple(&self) -> (u64, u64) {
        (self.0.identity().raw(), self.0.generation())
    }
}

/// A root decision made under `SCHEDULER`, carried to the raw-switch boundary.
/// No assembly code is allowed to rediscover task or root ownership.
pub(crate) struct SwitchPlan {
    pub outgoing: *mut Context,
    pub incoming: *const Context,
    transition: DomainTransition,
}

#[derive(Clone)]
enum DomainTransition {
    SasToSas,
    Activate(DomainRef),
    SameDomain,
    ToSafeRoot,
}

impl SwitchPlan {
    pub(crate) fn new(
        outgoing: *mut Context,
        incoming: *const Context,
        task: Option<&Task>,
    ) -> Option<Self> {
        if task.is_some_and(|task| !task.address_space_is_live()) {
            return None;
        }
        let transition = match task.and_then(DomainRef::from_task) {
            Some(domain) if hart_local::current_domain() == domain.tuple() => {
                DomainTransition::SameDomain
            }
            Some(domain) => DomainTransition::Activate(domain),
            None if hart_local::current_domain().0 == 0 => DomainTransition::SasToSas,
            None => DomainTransition::ToSafeRoot,
        };
        Some(Self {
            outgoing,
            incoming,
            transition,
        })
    }

    /// Return the root switch that assembly must issue after saving `outgoing`.
    /// A zero PPN is the explicit no-write SAS/same-domain fast path.
    pub(crate) fn root_switch(&self) -> (usize, usize) {
        match &self.transition {
            DomainTransition::Activate(domain) => {
                let (id, generation) = domain.tuple();
                let root = (domain.0.root_ppn(), domain.0.asid());
                let _ = domain
                    .0
                    .set_current_hart(hart_local::current_hart_id(), true);
                hart_local::set_current_domain(id, generation);
                crate::hal::domain::observe_switch_activation();
                #[cfg(feature = "test-hooks")]
                log::info!(
                    "S22-RV64-SWITCH: PASS harts={}",
                    super::smp::online_hart_count()
                );
                root
            }
            DomainTransition::ToSafeRoot => {
                let root = crate::memory::paging::KERNEL_ROOT
                    .lock()
                    .expect("native domain requires SAS root");
                hart_local::mark_safe_root_pending();
                crate::hal::domain::observe_switch_activation();
                (root >> 12, 0)
            }
            DomainTransition::SasToSas | DomainTransition::SameDomain => (0, 0),
        }
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn is_sas_fast_path(&self) -> bool {
        matches!(
            self.transition,
            DomainTransition::SasToSas | DomainTransition::SameDomain
        )
    }
}
