mod binding;
mod enroll;
mod noise;
mod relay;
mod rotate;
mod service_net;
mod tls;

pub use binding::{AcquireNodeIdentityPayload, BrokerBindingPayload, NodeIdentityStatusPayload};
pub use enroll::{
    RelayActivePublicKeyPayload, RelayCsrChunkRequestPayload, RelayCsrChunkResponsePayload,
    RelayEnrollmentAbortRequestPayload, RelayEnrollmentBeginRequestPayload,
    RelayEnrollmentBeginResponsePayload, RelayGenerationCommitRequestPayload,
    RelayGenerationCommitResponsePayload, RelayStageProfileRequestPayload,
};
pub use noise::{NoiseStaticDhRequestPayload, NoiseStaticDhResponsePayload};
pub use relay::RelayP256StatusPayload;
pub use rotate::{RotateNodeIdentityRequestPayload, RotateNodeIdentityResponsePayload};
pub use service_net::ServiceNetBindingPayload;
pub use tls::{
    Tls13ClientCertificateVerifyRequestPayload, Tls13ClientCertificateVerifyResponsePayload,
};

fn read_32(bytes: &[u8], at: usize) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes[at..at + 32]);
    out
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("fixed KMS u64"))
}

fn put_u16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut [u8], at: usize, value: u64) {
    out[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
