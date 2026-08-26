use super::{
    AuthorityMode, AuthorityState, BootState, OpenedBootFact, ProtectedStore, ProtectedTimeFloors,
    RelayProfileState, TimeState,
};

impl<S: ProtectedStore> AuthorityState<S> {
    /// Return the absorbing authority mode without exposing mutation.
    pub const fn mode(&self) -> AuthorityMode {
        self.mode
    }

    /// Return the current protected boot state.
    pub const fn boot_state(&self) -> BootState {
        self.boot
    }

    /// Return the signed response fact for the currently open measured boot.
    pub const fn opened_boot_fact(&self) -> Option<OpenedBootFact> {
        match self.boot {
            BootState::Open { epoch } => Some(OpenedBootFact {
                boot_epoch: epoch,
                state_epoch: self.state_epoch,
                approved_loader_digest: self.approved_loader_digest,
            }),
            BootState::Closed => None,
        }
    }

    /// Return the current one-shot time lease state.
    pub const fn time_state(&self) -> TimeState {
        self.time
    }

    /// Return the current relay transaction snapshot.
    pub const fn relay_state(&self) -> RelayProfileState {
        self.relay
    }

    /// Return retained signed-time rollback floors for durable persistence.
    pub const fn time_floors(&self) -> ProtectedTimeFloors {
        self.time_floors
    }
}
