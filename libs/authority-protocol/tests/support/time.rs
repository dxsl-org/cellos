use super::{
    authenticate, context, header, time_signature, validated, RequestPolicy, TestState, TimePolicy,
};
use authority_protocol::*;

pub struct Clock(pub u64);
impl TrustedClock for Clock {
    fn now_unix_seconds(&self) -> u64 {
        self.0
    }
}

pub struct Challenges(pub u8);
impl TimeChallengeSource for Challenges {
    fn generate_challenge(&mut self) -> Result<([u8; 16], [u8; 32]), AuthorityFault> {
        let id = [self.0; 16];
        let nonce = [self.0.wrapping_add(1); 32];
        self.0 = self.0.wrapping_add(2);
        Ok((id, nonce))
    }
}

pub fn floors() -> ProtectedTimeFloors {
    ProtectedTimeFloors {
        source_epoch: 0,
        source_sequence: 0,
        unix_seconds: 0,
    }
}

pub fn grant_time(
    state: &mut TestState,
    challenges: &mut Challenges,
    sequence: u64,
    boot: u64,
    purpose: TimePurpose,
    source_sequence: u64,
    expires_at: u64,
) {
    let request = validated(RequestSignedTimeRequest {
        context: context(sequence, boot, Operation::RequestSignedTime),
        purpose: purpose as u8,
    });
    let challenge = state.request_signed_time(&request, challenges).unwrap();
    let mut fact = AcceptSignedTimeRequest {
        context: context(sequence + 1, boot, Operation::AcceptSignedTime),
        time_request_id: challenge.time_request_id,
        purpose: purpose as u8,
        source_epoch: 1,
        source_sequence,
        unix_seconds: 100 + source_sequence as i64,
        expires_at,
        nonce: challenge.nonce,
        source_signature: time_signature(),
    };
    authenticate(&mut fact);
    let verified = verify_signed_time(fact, &header(&fact), &RequestPolicy, &TimePolicy).unwrap();
    state.accept_time(&verified, &Clock(100)).unwrap();
}
