use super::{AuthorityMode, AuthorityState, BootState, ProtectedStore};
use crate::{
    constant_time_eq, AuthorityFault, OpenBootRequest, ValidatedRequest, VerifiedBootMeasurement,
    DIGEST_LEN,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenedBootFact {
    pub boot_epoch: u64,
    pub state_epoch: u64,
    pub approved_loader_digest: [u8; DIGEST_LEN],
}

impl<S: ProtectedStore> AuthorityState<S> {
    /// Allocate a protected boot epoch after authenticating the fresh challenge.
    pub fn open_boot(
        &mut self,
        validated: &ValidatedRequest<OpenBootRequest>,
        measurement: &VerifiedBootMeasurement,
    ) -> Result<u64, AuthorityFault> {
        let request = validated.request();
        self.identity_and_sequence(&request.context)?;
        if !constant_time_eq(&self.boot_challenge, &request.context.challenge) {
            return self.seal(AuthorityFault::ChallengeMismatch);
        }
        if !constant_time_eq(measurement.loader_digest(), &request.loader_digest) {
            return self.seal(AuthorityFault::IdentityMismatch);
        }
        if self.mode != AuthorityMode::Ready || self.boot != BootState::Closed {
            return self.seal(AuthorityFault::InvalidState);
        }
        self.boot_floor = match self.boot_floor.checked_add(1) {
            Some(epoch) => epoch,
            None => return self.seal(AuthorityFault::PersistenceFailure),
        };
        self.state_epoch = match self.state_epoch.checked_add(1) {
            Some(epoch) => epoch,
            None => return self.seal(AuthorityFault::PersistenceFailure),
        };
        self.approved_loader_digest = request.loader_digest;
        self.boot = BootState::Open {
            epoch: self.boot_floor,
        };
        self.mode = AuthorityMode::Serving;
        self.persist()?;
        Ok(self.boot_floor)
    }
}
