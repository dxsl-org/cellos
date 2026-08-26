use super::{DIGEST_LEN, HOSTNAME_MAX, PROFILE_MAX, REQUEST_CONTEXT_WIRE_LEN, TLS_SIGNATURE_MAX};
use crate::{Operation, FRAME_MAX_PAYLOAD};

/// Maximum canonical request payload for an operation.
pub const fn max_payload_len(operation: Operation) -> usize {
    match operation {
        Operation::ValidateAndStageRelayProfile => {
            REQUEST_CONTEXT_WIRE_LEN + 8 + 8 + 1 + (DIGEST_LEN * 3) + 2 + PROFILE_MAX
        }
        Operation::AcceptSignedTime => {
            REQUEST_CONTEXT_WIRE_LEN + 16 + 1 + 32 + DIGEST_LEN + 2 + TLS_SIGNATURE_MAX
        }
        Operation::BeginRelayEnrollment => REQUEST_CONTEXT_WIRE_LEN + 2 + HOSTNAME_MAX,
        _ => REQUEST_CONTEXT_WIRE_LEN + 160,
    }
}

const _: () =
    assert!(max_payload_len(Operation::ValidateAndStageRelayProfile) <= FRAME_MAX_PAYLOAD);
