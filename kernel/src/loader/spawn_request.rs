//! Route-complete inputs supplied before governed ELF admission.

use alloc::vec::Vec;

pub struct SpawnRequest {
    pub(crate) spawner: crate::task::cap::Spawner,
    pub(crate) caller: Option<crate::task::CallerLaunchAuthority>,
    pub(crate) priority: u8,
    pub(crate) replacement: Option<crate::cell::hotswap::ReplacementReservation>,
    pub(crate) inherit_from: usize,
    pub(crate) argv: Option<Vec<u8>>,
    /// The kernel resolved the launch path (boot, Path or Pinned route), so the
    /// child may be named by its signed image. False for ELF/Mem bytes, whose
    /// path is advisory.
    pub(crate) kernel_resolved_path: bool,
    #[cfg(feature = "p01-trace")]
    pub(crate) trace: Option<crate::p01_measurement::Attempt>,
}

impl SpawnRequest {
    /// Governed kernel boot path. It has no live caller to inherit from.
    pub fn governed_boot() -> Self {
        Self {
            spawner: crate::task::cap::Spawner::Root,
            caller: None,
            priority: api::TaskPriority::Normal as u8,
            replacement: None,
            inherit_from: 0,
            argv: None,
            kernel_resolved_path: true,
            #[cfg(feature = "p01-trace")]
            trace: None,
        }
    }

    /// Attach command-line arguments to this launch request.
    pub fn with_argv(mut self, argv: Vec<u8>) -> Self {
        self.argv = Some(argv);
        self
    }

    /// Governed syscall path with separately captured caller authority and
    /// edge-specific child ceiling.
    pub(crate) fn governed_caller(
        tid: usize,
        (cell_id, generation): (u64, u64),
        caller_authority: crate::task::cap::CapSet,
        child_ceiling: crate::task::cap::CapSet,
        priority: u8,
        argv: Option<Vec<u8>>,
        kernel_resolved_path: bool,
    ) -> Self {
        Self {
            spawner: crate::task::cap::Spawner::Ceiling(child_ceiling),
            caller: Some(crate::task::CallerLaunchAuthority {
                tid,
                cell_id,
                generation,
                ceiling: caller_authority,
            }),
            priority,
            replacement: None,
            inherit_from: tid,
            argv,
            kernel_resolved_path,
            #[cfg(feature = "p01-trace")]
            trace: None,
        }
    }

    /// Bind a governed request to one frozen-source reservation while retaining
    /// the supervisor's independent launch-authority snapshot.
    pub(crate) fn governed_replacement(
        tid: usize,
        (cell_id, generation): (u64, u64),
        caller_authority: crate::task::cap::CapSet,
        route_ceiling: crate::task::cap::CapSet,
        replacement: crate::cell::hotswap::ReplacementReservation,
        argv: Option<Vec<u8>>,
    ) -> Self {
        let child_ceiling = route_ceiling.intersect(replacement.ceiling());
        Self {
            spawner: crate::task::cap::Spawner::Ceiling(child_ceiling),
            caller: Some(crate::task::CallerLaunchAuthority {
                tid,
                cell_id,
                generation,
                ceiling: caller_authority,
            }),
            priority: api::TaskPriority::Normal as u8,
            replacement: Some(replacement),
            inherit_from: tid,
            argv,
            // Replacements always load through `spawn_from_path`.
            kernel_resolved_path: true,
            #[cfg(feature = "p01-trace")]
            trace: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SpawnRequest;
    use crate::task::cap::{CapSet, Spawner};

    #[test]
    fn caller_authority_and_child_ceiling_are_independent() {
        let caller_authority = CapSet {
            spawn: true,
            network: true,
            ..CapSet::EMPTY
        };
        let request = SpawnRequest::governed_caller(
            7,
            (3, 11),
            caller_authority,
            CapSet::EMPTY,
            api::TaskPriority::Normal as u8,
            None,
            false,
        );

        assert!(matches!(request.spawner, Spawner::Ceiling(CapSet::EMPTY)));
        assert_eq!(
            request.caller.expect("caller authority").ceiling,
            caller_authority
        );
    }
}
