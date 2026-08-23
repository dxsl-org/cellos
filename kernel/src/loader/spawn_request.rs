//! Route-complete inputs supplied before governed ELF admission.

use alloc::vec::Vec;

pub struct SpawnRequest {
    pub(crate) spawner: crate::task::cap::Spawner,
    pub(crate) caller: Option<crate::task::CallerLaunchAuthority>,
    pub(crate) priority: u8,
    pub(crate) replacement: Option<crate::cell::hotswap::ReplacementReservation>,
    pub(crate) inherit_from: usize,
    pub(crate) argv: Option<Vec<u8>>,
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
        }
    }

    /// Governed syscall path with separately captured caller authority and
    /// edge-specific child ceiling.
    pub(crate) fn governed_caller(
        tid: usize,
        generation: u64,
        caller_authority: crate::task::cap::CapSet,
        child_ceiling: crate::task::cap::CapSet,
        priority: u8,
        argv: Option<Vec<u8>>,
    ) -> Self {
        Self {
            spawner: crate::task::cap::Spawner::Ceiling(child_ceiling),
            caller: Some(crate::task::CallerLaunchAuthority {
                tid,
                generation,
                ceiling: caller_authority,
            }),
            priority,
            replacement: None,
            inherit_from: tid,
            argv,
        }
    }

    /// Bind a governed request to one frozen-source reservation while retaining
    /// the supervisor's independent launch-authority snapshot.
    pub(crate) fn governed_replacement(
        tid: usize,
        generation: u64,
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
                generation,
                ceiling: caller_authority,
            }),
            priority: api::TaskPriority::Normal as u8,
            replacement: Some(replacement),
            inherit_from: tid,
            argv,
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
            11,
            caller_authority,
            CapSet::EMPTY,
            api::TaskPriority::Normal as u8,
            None,
        );

        assert!(matches!(request.spawner, Spawner::Ceiling(CapSet::EMPTY)));
        assert_eq!(
            request.caller.expect("caller authority").ceiling,
            caller_authority
        );
    }
}
