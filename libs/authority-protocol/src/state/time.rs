use super::{
    AuthorityState, PendingTimeChallenge, ProtectedStore, ProtectedTimeFloors, TimePurpose,
    TimeState,
};
use crate::{
    AcceptSignedTimeRequest, AuthorityFault, RequestSignedTimeRequest, ValidatedRequest,
    VerifiedSignedTime, DIGEST_LEN,
};

pub trait TrustedClock {
    fn now_unix_seconds(&self) -> u64;
}

pub trait TimeChallengeSource {
    fn generate_challenge(&mut self) -> Result<([u8; 16], [u8; DIGEST_LEN]), AuthorityFault>;
}
impl TryFrom<u8> for TimePurpose {
    type Error = AuthorityFault;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Enrollment),
            2 => Ok(Self::RelayHandshake),
            3 => Ok(Self::TlsCertificateVerify),
            _ => Err(AuthorityFault::TimeInvalid),
        }
    }
}

impl<S: ProtectedStore> AuthorityState<S> {
    pub fn request_signed_time(
        &mut self,
        validated: &ValidatedRequest<RequestSignedTimeRequest>,
        source: &mut impl TimeChallengeSource,
    ) -> Result<PendingTimeChallenge, AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        if self.pending_time.is_some() || self.time != TimeState::Unavailable {
            return self.seal(AuthorityFault::InvalidState);
        }
        let purpose = match TimePurpose::try_from(request.purpose) {
            Ok(value) => value,
            Err(fault) => return self.seal(fault),
        };
        let (time_request_id, nonce) = match source.generate_challenge() {
            Ok((id, nonce)) if id != [0; 16] && nonce != [0; DIGEST_LEN] => (id, nonce),
            Ok(_) => return self.seal(AuthorityFault::TimeInvalid),
            Err(fault) => return self.seal(fault),
        };
        let challenge = PendingTimeChallenge {
            time_request_id,
            purpose,
            nonce,
        };
        self.pending_time = Some(challenge);
        self.persist()?;
        Ok(challenge)
    }

    pub fn accept_time(
        &mut self,
        verified: &VerifiedSignedTime,
        clock: &impl TrustedClock,
    ) -> Result<(), AuthorityFault> {
        let request: &AcceptSignedTimeRequest = verified.request();
        self.authorize_context(&request.context)?;
        let purpose = match TimePurpose::try_from(request.purpose) {
            Ok(value) => value,
            Err(fault) => return self.seal(fault),
        };
        let expected = PendingTimeChallenge {
            time_request_id: request.time_request_id,
            purpose,
            nonce: request.nonce,
        };
        if request.unix_seconds < 0 {
            return self.seal(AuthorityFault::TimeInvalid);
        }
        let unix_seconds = request.unix_seconds as u64;
        if self.pending_time != Some(expected)
            || request.expires_at <= clock.now_unix_seconds()
            || request.expires_at <= unix_seconds
        {
            return self.seal(AuthorityFault::TimeInvalid);
        }
        if request.source_epoch < self.time_floors.source_epoch
            || (request.source_epoch == self.time_floors.source_epoch
                && request.source_sequence <= self.time_floors.source_sequence)
            || unix_seconds <= self.time_floors.unix_seconds
        {
            return self.seal(AuthorityFault::Regression);
        }
        self.time_floors = ProtectedTimeFloors {
            source_epoch: request.source_epoch,
            source_sequence: request.source_sequence,
            unix_seconds,
        };
        self.pending_time = None;
        self.time = TimeState::Valid {
            source_epoch: request.source_epoch,
            sequence: request.source_sequence,
            expires_at: request.expires_at,
            time_request_id: request.time_request_id,
            purpose,
        };
        self.persist()?;
        Ok(())
    }

    pub(super) fn consume_live_time(
        &mut self,
        purpose: TimePurpose,
        now: u64,
    ) -> Result<(), AuthorityFault> {
        match self.time {
            TimeState::Valid {
                expires_at,
                purpose: actual,
                ..
            } if now < expires_at && actual == purpose => {
                self.time = TimeState::Unavailable;
                self.persist()
            }
            TimeState::Valid { expires_at, .. } if now >= expires_at => {
                self.seal(AuthorityFault::TimeInvalid)
            }
            TimeState::Valid { .. } => self.seal(AuthorityFault::TimeInvalid),
            TimeState::Unavailable => self.seal(AuthorityFault::TimeUnavailable),
        }
    }
}
