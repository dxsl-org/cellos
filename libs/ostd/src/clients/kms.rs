//! Raw fixed-frame client for the key-management service.

mod relay;

use crate::{syscall, task};
use types::kms::{
    AcquireNodeIdentityPayload, BindingEpoch, BrokerBindingPayload, KmsErrorCode, KmsOpcode,
    KmsRequestV1, KmsResponseV1, KmsWireError, NodeIdentityHandle, NodeIdentityStatusPayload,
    NoiseStaticDhRequestPayload, NoiseStaticDhResponsePayload, RelayP256StatusPayload,
    RotateNodeIdentityReason, RotateNodeIdentityRequestPayload, RotateNodeIdentityResponsePayload,
    ServiceNetBindingPayload, KMS_MESSAGE_LEN, KMS_NODE_KEY_ID_C2C,
};

const REQUEST_ID: u32 = 1;

/// Failure returned by a KMS IPC round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmsClientError {
    ServiceNotFound,
    Ipc,
    WrongSender,
    MismatchedResponse,
    InvalidPayload,
    Wire(KmsWireError),
    Service(KmsErrorCode),
}

/// Synchronous client for opaque node-identity operations.
pub struct KmsClient {
    tid: usize,
}

impl KmsClient {
    /// Resolve the live KMS provider with bounded scheduler retries.
    pub fn connect() -> Result<Self, KmsClientError> {
        for _ in 0..8 {
            if let Some(tid) = syscall::sys_lookup_service(api::syscall::service::KMS) {
                return Ok(Self { tid });
            }
            task::yield_now();
        }
        Err(KmsClientError::ServiceNotFound)
    }

    /// Bind KMS authority to the current supervised broker instance.
    pub fn register_broker_instance(&self) -> Result<BrokerBindingPayload, KmsClientError> {
        let response = self.call_opcode(KmsOpcode::RegisterBrokerInstance, &[])?;
        self.decode_payload(&response, BrokerBindingPayload::decode)
    }

    /// Bind relay authority to this live supervised service-net generation.
    pub fn register_service_net_instance(&self) -> Result<ServiceNetBindingPayload, KmsClientError> {
        let response = self.call_opcode(KmsOpcode::RegisterServiceNetInstance, &[])?;
        self.decode_payload(&response, ServiceNetBindingPayload::decode)
    }

    /// Read independent Relay P-256 readiness and protected metadata.
    pub fn get_relay_p256_status(&self) -> Result<RelayP256StatusPayload, KmsClientError> {
        let response = self.call_opcode(KmsOpcode::GetRelayP256Status, &[])?;
        self.decode_payload(&response, RelayP256StatusPayload::decode)
    }


    /// Read fail-closed readiness and public node-identity metadata.
    pub fn get_node_identity_status(&self) -> Result<NodeIdentityStatusPayload, KmsClientError> {
        let response = self.call_opcode(KmsOpcode::GetNodeIdentityStatus, &[])?;
        self.decode_payload(&response, NodeIdentityStatusPayload::decode)
    }

    /// Acquire the current opaque node-identity handle and public key metadata.
    pub fn acquire_node_identity(&self) -> Result<AcquireNodeIdentityPayload, KmsClientError> {
        let response = self.call_opcode(KmsOpcode::AcquireNodeIdentity, &[])?;
        self.decode_payload(&response, AcquireNodeIdentityPayload::decode)
    }

    /// Ask KMS to perform X25519 static DH for an opaque C2C identity handle.
    ///
    /// The response contains the ephemeral shared secret, never the node's
    /// private scalar. A stale handle or binding epoch fails closed in KMS.
    pub fn noise_static_dh(
        &self,
        handle: NodeIdentityHandle,
        binding_epoch: BindingEpoch,
        peer_public_key: &[u8; 32],
    ) -> Result<[u8; 32], KmsClientError> {
        if handle.0 == 0 || binding_epoch.0 == 0 || peer_public_key.iter().all(|byte| *byte == 0) {
            return Err(KmsClientError::InvalidPayload);
        }
        let payload = NoiseStaticDhRequestPayload {
            handle,
            key_id: KMS_NODE_KEY_ID_C2C,
            reserved: 0,
            binding_epoch,
            peer_public_key: *peer_public_key,
        };
        let response = self.call_opcode(KmsOpcode::NoiseStaticDh, &payload.encode())?;
        let payload = self.decode_payload(&response, NoiseStaticDhResponsePayload::decode)?;
        if payload.handle != handle || payload.binding_epoch != binding_epoch {
            return Err(KmsClientError::MismatchedResponse);
        }
        let shared_secret = payload.shared_secret;
        if shared_secret.iter().all(|byte| *byte == 0) {
            return Err(KmsClientError::InvalidPayload);
        }
        Ok(shared_secret)
    }

    /// Rotate the node identity under supervisor authority.
    pub fn rotate_node_identity(
        &self,
        reason: RotateNodeIdentityReason,
        expected_blob_revision: u64,
    ) -> Result<RotateNodeIdentityResponsePayload, KmsClientError> {
        let payload = RotateNodeIdentityRequestPayload {
            reason,
            reserved0: 0,
            flags: 0,
            expected_blob_revision,
        };
        let response = self.call_opcode(KmsOpcode::RotateNodeIdentity, &payload.encode())?;
        self.decode_payload(&response, RotateNodeIdentityResponsePayload::decode)
    }

    fn call_opcode(
        &self,
        opcode: KmsOpcode,
        payload: &[u8],
    ) -> Result<KmsResponseV1, KmsClientError> {
        let request =
            KmsRequestV1::new(opcode, REQUEST_ID, payload).map_err(KmsClientError::Wire)?;
        self.call(&request)
    }

    fn call(&self, request: &KmsRequestV1) -> Result<KmsResponseV1, KmsClientError> {
        match syscall::sys_send(self.tid, &request.to_bytes()) {
            syscall::SyscallResult::Ok(_) => {}
            syscall::SyscallResult::Err(_) => return Err(KmsClientError::Ipc),
        }

        let mut bytes = [0u8; KMS_MESSAGE_LEN];
        // KMS replies must be correlated to the live service tid so queued
        // traffic from other senders cannot be consumed as a false reply.
        match syscall::sys_recv(self.tid, &mut bytes) {
            syscall::SyscallResult::Ok(sender) if sender == self.tid => {}
            syscall::SyscallResult::Ok(_) => return Err(KmsClientError::WrongSender),
            syscall::SyscallResult::Err(_) => return Err(KmsClientError::Ipc),
        }

        let response = KmsResponseV1::from_bytes(&bytes).map_err(KmsClientError::Wire)?;
        if response.opcode().map_err(KmsClientError::Wire)?
            != request.opcode().map_err(KmsClientError::Wire)?
            || response.request_id != request.request_id
        {
            return Err(KmsClientError::MismatchedResponse);
        }
        if let Some(code) = response.error_code().map_err(KmsClientError::Wire)? {
            return Err(KmsClientError::Service(code));
        }
        Ok(response)
    }

    fn decode_payload<T>(
        &self,
        response: &KmsResponseV1,
        decode: fn(&[u8]) -> Option<T>,
    ) -> Result<T, KmsClientError> {
        let body = response.payload().map_err(KmsClientError::Wire)?;
        decode(body).ok_or(KmsClientError::InvalidPayload)
    }
}

