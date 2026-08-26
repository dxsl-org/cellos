//! Explicit authority transition table; `Sealed` is absorbing.

mod boot;
mod intents;
mod persistence;
mod provider;
mod read;
mod time;
mod transitions;
mod view;
pub use boot::OpenedBootFact;
pub use intents::{CsrChunkIntent, EnrollmentIntent, TlsSignatureIntent};
pub use time::{TimeChallengeSource, TrustedClock};

use crate::{constant_time_eq, AuthorityFault, RequestContext, DIGEST_LEN, ID_LEN};
pub use persistence::{
    verify_protected_record, ProtectedAuthorityRecord, ProtectedRecordVerifier, ProtectedStore,
    VerifiedProtectedRecord, PROTECTED_RECORD_MAX,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityMode {
    Ready,
    Serving,
    Sealed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootState {
    Closed,
    Open { epoch: u64 },
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimePurpose {
    Enrollment = 1,
    RelayHandshake = 2,
    TlsCertificateVerify = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingTimeChallenge {
    pub time_request_id: [u8; 16],
    pub purpose: TimePurpose,
    pub nonce: [u8; DIGEST_LEN],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectedTimeFloors {
    pub source_epoch: u64,
    pub source_sequence: u64,
    pub unix_seconds: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeState {
    Unavailable,
    Valid {
        source_epoch: u64,
        sequence: u64,
        expires_at: u64,
        time_request_id: [u8; 16],
        purpose: TimePurpose,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayIntent {
    pub device_id: [u8; ID_LEN],
    pub authority_id: [u8; ID_LEN],
    pub authority_epoch: u64,
    pub generation: u64,
    pub policy_epoch: u64,
    pub pending_slot: u8,
    pub pending_spki_digest: [u8; DIGEST_LEN],
    pub profile_digest: [u8; DIGEST_LEN],
    pub boot_epoch: u64,
    pub validation_request_id: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedCommitIntent(RelayIntent);
impl PreparedCommitIntent {
    pub const fn intent(&self) -> &RelayIntent {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayProfileState {
    Empty,
    Pending {
        generation: u64,
        csr_handle: u64,
    },
    Staged(RelayIntent),
    ReceiptConsumed(RelayIntent),
    Prepared(RelayIntent),
    Promoted {
        intent: RelayIntent,
        receipt: crate::ProviderCasReceipt,
    },
    Active(RelayIntent),
}

pub struct AuthorityState<S: ProtectedStore> {
    mode: AuthorityMode,
    boot: BootState,
    time: TimeState,
    relay: RelayProfileState,
    device_id: [u8; ID_LEN],
    authority_id: [u8; ID_LEN],
    authority_epoch: u64,
    boot_challenge: [u8; DIGEST_LEN],
    boot_floor: u64,
    generation_floor: u64,
    state_epoch: u64,
    approved_loader_digest: [u8; DIGEST_LEN],
    last_request_sequence: u64,
    previous_active: Option<RelayIntent>,
    pending_time: Option<PendingTimeChallenge>,
    time_floors: ProtectedTimeFloors,
    protected_revision: u64,
    store: S,
}

impl<S: ProtectedStore> AuthorityState<S> {
    pub const fn new(
        store: S,
        device_id: [u8; ID_LEN],
        authority_id: [u8; ID_LEN],
        authority_epoch: u64,
        boot_floor: u64,
        generation_floor: u64,
        state_epoch: u64,
        boot_challenge: [u8; DIGEST_LEN],
        time_floors: ProtectedTimeFloors,
    ) -> Self {
        Self {
            mode: AuthorityMode::Ready,
            boot: BootState::Closed,
            time: TimeState::Unavailable,
            relay: RelayProfileState::Empty,
            device_id,
            authority_id,
            authority_epoch,
            boot_challenge,
            boot_floor,
            generation_floor,
            state_epoch,
            approved_loader_digest: [0; DIGEST_LEN],
            last_request_sequence: 0,
            previous_active: None,
            pending_time: None,
            time_floors,
            protected_revision: 0,
            store,
        }
    }

    fn authorize_context(&mut self, context: &RequestContext) -> Result<(), AuthorityFault> {
        self.identity_and_sequence(context)?;
        match self.boot {
            BootState::Open { epoch }
                if epoch == context.boot_epoch
                    && constant_time_eq(&self.boot_challenge, &context.challenge) =>
            {
                Ok(())
            }
            _ => self.seal(AuthorityFault::ChallengeMismatch),
        }
    }
    fn identity_and_sequence(&mut self, context: &RequestContext) -> Result<(), AuthorityFault> {
        if self.mode == AuthorityMode::Sealed {
            return Err(AuthorityFault::Sealed);
        }
        if !constant_time_eq(&self.device_id, &context.device_id)
            || !constant_time_eq(&self.authority_id, &context.authority_id)
        {
            return self.seal(AuthorityFault::IdentityMismatch);
        }
        if context.sequence <= self.last_request_sequence {
            return self.seal(AuthorityFault::Replay);
        }
        self.last_request_sequence = context.sequence;
        Ok(())
    }
    fn persist_value<T>(&mut self, value: T) -> Result<T, AuthorityFault> {
        self.persist()?;
        Ok(value)
    }

    fn seal<T>(&mut self, fault: AuthorityFault) -> Result<T, AuthorityFault> {
        self.mode = AuthorityMode::Sealed;
        self.time = TimeState::Unavailable;
        self.pending_time = None;
        match self.persist() {
            Ok(()) => Err(fault),
            Err(_) => Err(AuthorityFault::PersistenceFailure),
        }
    }
}
