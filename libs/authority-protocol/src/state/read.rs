use super::{
    AuthorityState, CsrChunkIntent, ProtectedStore, RelayIntent, RelayProfileState, TimePurpose,
    TrustedClock,
};
use crate::{
    AuthorityFault, GetRelayActivePublicKeyRequest, ReadCommittedRelayStateRequest,
    ReadRelayCsrChunkRequest, ValidatedRequest,
};

impl<S: ProtectedStore> AuthorityState<S> {
    pub fn authorize_committed_state(
        &mut self,
        validated: &ValidatedRequest<ReadCommittedRelayStateRequest>,
    ) -> Result<RelayIntent, AuthorityFault> {
        self.authorize_context(&validated.request().context)?;
        match self.relay {
            RelayProfileState::Active(intent) => self.persist_value(intent),
            _ => self.seal(AuthorityFault::InvalidState),
        }
    }

    pub fn authorize_csr_chunk(
        &mut self,
        validated: &ValidatedRequest<ReadRelayCsrChunkRequest>,
    ) -> Result<CsrChunkIntent, AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        match self.relay {
            RelayProfileState::Pending {
                generation,
                csr_handle,
            } if request.csr_handle == csr_handle => self.persist_value(CsrChunkIntent {
                generation,
                csr_handle,
                chunk_index: request.chunk_index,
            }),
            _ => self.seal(AuthorityFault::InvalidState),
        }
    }

    pub fn authorize_active_public_key(
        &mut self,
        validated: &ValidatedRequest<GetRelayActivePublicKeyRequest>,
        clock: &impl TrustedClock,
    ) -> Result<RelayIntent, AuthorityFault> {
        self.authorize_context(&validated.request().context)?;
        self.consume_live_time(TimePurpose::RelayHandshake, clock.now_unix_seconds())?;
        match self.relay {
            RelayProfileState::Active(intent) => self.persist_value(intent),
            _ => self.seal(AuthorityFault::InvalidState),
        }
    }
}
