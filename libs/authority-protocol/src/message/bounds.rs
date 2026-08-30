use super::{HOSTNAME_MAX, PROFILE_CHUNK_MAX, REQUEST_CONTEXT_WIRE_LEN, TLS_SIGNATURE_MAX};
use crate::{Operation, FRAME_MAX_PAYLOAD};

/// Maximum request or response payload for an operation.
pub const fn max_payload_len(operation: Operation) -> usize {
    match operation {
        Operation::OpenBoot => 225,
        Operation::ReadCommittedRelayState => 225,
        Operation::RequestSignedTime => 226,
        Operation::AcceptSignedTime => {
            REQUEST_CONTEXT_WIRE_LEN + 16 + 1 + 32 + 32 + 2 + TLS_SIGNATURE_MAX
        }
        Operation::BeginRelayEnrollment => REQUEST_CONTEXT_WIRE_LEN + 2 + HOSTNAME_MAX,
        Operation::ReadRelayCsrChunk => 287,
        Operation::ValidateAndStageRelayProfile => 358,
        Operation::ConsumeStagedRelayProfile => 233,
        Operation::CommitRelayGeneration => 233,
        Operation::AbortRelayEnrollment => 193,
        Operation::GetRelayActivePublicKey => 282,
        Operation::SignTls13ClientCertificateVerify => 265,
        Operation::BeginRelayProfileUpload => 310,
        Operation::WriteRelayProfileChunk => {
            REQUEST_CONTEXT_WIRE_LEN + 8 + 1 + 2 + PROFILE_CHUNK_MAX
        }
    }
}

const _: () = assert!(max_payload_len(Operation::WriteRelayProfileChunk) <= FRAME_MAX_PAYLOAD);
