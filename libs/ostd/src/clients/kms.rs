//! Raw fixed-frame client for the key-management service.

mod relay;
mod relay_enroll;
mod ssh;

pub use ssh::{SSH_FRAME_TICKS, SSH_SIGN_TICKS};
/// The K2 SSH custody types and bounds a caller needs, so `cellbox` and the admin cell
/// depend on `ostd` alone (no extra `types` edge).
pub use types::kms::{
    ssh_chunk_count, validate_ssh_key_name, validate_ssh_username, KmsCustodyPosture, KmsErrorCode,
    KmsSshKeyCustody, KmsWireError, SshChunkAckPayload, SshKeyStatusPayload, SshSignResponsePayload,
    SshTransferBeginResponsePayload, KMS_SSH_CHUNK_LEN, KMS_SSH_IMPORT_MAX_LEN,
    KMS_SSH_KEY_NAME_MAX, KMS_SSH_PUBLIC_KEY_LEN, KMS_SSH_SECRET_MAX, KMS_SSH_SESSION_ID_LEN,
    KMS_SSH_SIGNATURE_LEN, KMS_SSH_USERNAME_MAX,
};

use crate::{syscall, task};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use types::kms::{
    AcquireNodeIdentityPayload, BindingEpoch, BrokerBindingPayload, KmsOpcode,
    KmsRequestV1, KmsResponseV1, NodeIdentityHandle, NodeIdentityStatusPayload,
    NoiseStaticDhRequestPayload, NoiseStaticDhResponsePayload, RelayP256StatusPayload,
    RotateNodeIdentityReason, RotateNodeIdentityRequestPayload, RotateNodeIdentityResponsePayload,
    ServiceNetBindingPayload, KMS_MESSAGE_LEN, KMS_NODE_KEY_ID_C2C,
};

const FIRST_REQUEST_ID: u32 = 1;
/// First payload `request_id` (nonzero; the service requires each to exceed its floor).
const FIRST_PAYLOAD_ID: u64 = 1;

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
    /// No reply inside the bound of a bounded (SSH) call. The client is poisoned: a late
    /// reply would otherwise be taken for the next call's, so every later call returns
    /// this too, without sending anything.
    Timeout,
}

/// Synchronous client for opaque node-identity operations.
pub struct KmsClient {
    tid: usize,
    next_request_id: AtomicU32,
    /// Payload `request_id` of KMS opcodes 15, 20, 21 and 22: nonzero and strictly
    /// increasing within this client (the service keeps a per-principal floor).
    next_payload_id: AtomicU64,
    poisoned: AtomicBool,
    #[cfg(test)]
    script: Option<scripted::Script>,
}

impl KmsClient {
    /// Resolve the live KMS provider with bounded scheduler retries.
    pub fn connect() -> Result<Self, KmsClientError> {
        for _ in 0..8 {
            if let Some(tid) = syscall::sys_lookup_service(api::syscall::service::KMS) {
                return Ok(Self {
                    tid,
                    next_request_id: AtomicU32::new(FIRST_REQUEST_ID),
                    next_payload_id: AtomicU64::new(FIRST_PAYLOAD_ID),
                    poisoned: AtomicBool::new(false),
                    #[cfg(test)]
                    script: None,
                });
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
    pub fn register_service_net_instance(
        &self,
    ) -> Result<ServiceNetBindingPayload, KmsClientError> {
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
    ///
    /// `expected_blob_revision` is an exact nonzero compare-and-swap guard.
    /// Callers must first read status; zero is never a wildcard recovery mode.
    pub fn rotate_node_identity(
        &self,
        reason: RotateNodeIdentityReason,
        expected_blob_revision: u64,
    ) -> Result<RotateNodeIdentityResponsePayload, KmsClientError> {
        if expected_blob_revision == 0 {
            return Err(KmsClientError::InvalidPayload);
        }
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
        let request_id = self
            .next_request_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1).filter(|next| *next != 0)
            })
            .map_err(|_| KmsClientError::InvalidPayload)?;
        let request =
            KmsRequestV1::new(opcode, request_id, payload).map_err(KmsClientError::Wire)?;
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

    /// Next payload `request_id` (opcodes 15, 20, 21, 22): nonzero, strictly increasing.
    fn next_payload_id(&self) -> Result<u64, KmsClientError> {
        self.next_payload_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| KmsClientError::InvalidPayload)
    }

    /// Bounded twin of [`Self::call_opcode`] for the SSH opcodes.
    fn call_ssh(
        &self,
        opcode: KmsOpcode,
        payload: &[u8],
        ticks: u64,
    ) -> Result<KmsResponseV1, KmsClientError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(KmsClientError::Timeout);
        }
        let request_id = self
            .next_request_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1).filter(|next| *next != 0)
            })
            .map_err(|_| KmsClientError::InvalidPayload)?;
        let mut request =
            KmsRequestV1::new(opcode, request_id, payload).map_err(KmsClientError::Wire)?;
        self.call_bounded(&mut request, ticks)
    }

    /// `sys_send`, then a receive bounded by `ticks` scheduler ticks (10 ms each). A frame
    /// the service cannot decode gets no reply at all, so the unbounded receive of
    /// [`Self::call`] would block forever. The request's payload and the frame built from
    /// it are wiped once sent (an unlock chunk carries passphrase bytes). A timeout poisons
    /// the client: a late reply must never be taken for the next call's.
    fn call_bounded(
        &self,
        request: &mut KmsRequestV1,
        ticks: u64,
    ) -> Result<KmsResponseV1, KmsClientError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(KmsClientError::Timeout);
        }
        let frame = Wiped(request.to_bytes());
        crate::session::zeroize(&mut request.payload);
        let reply = self.exchange(&frame.0, ticks);
        drop(frame);
        let bytes = match reply {
            Ok(bytes) => bytes,
            Err(KmsClientError::Timeout) => {
                self.poisoned.store(true, Ordering::Release);
                return Err(KmsClientError::Timeout);
            }
            Err(error) => return Err(error),
        };
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

    /// One request frame out, one reply frame in, or `Timeout`. The only place the SSH
    /// path touches the kernel, so host tests replace it with a script.
    fn exchange(
        &self,
        frame: &[u8; KMS_MESSAGE_LEN],
        ticks: u64,
    ) -> Result<[u8; KMS_MESSAGE_LEN], KmsClientError> {
        #[cfg(test)]
        if let Some(script) = &self.script {
            return script.exchange(frame, ticks);
        }
        match syscall::sys_send(self.tid, frame) {
            syscall::SyscallResult::Ok(_) => {}
            syscall::SyscallResult::Err(_) => return Err(KmsClientError::Ipc),
        }
        let mut bytes = [0u8; KMS_MESSAGE_LEN];
        match syscall::sys_recv_timeout(self.tid, &mut bytes, ticks) {
            syscall::SyscallResult::Ok(0) => {
                // One non-blocking drain: a reply that raced the deadline is taken here
                // so the service's own blocking send is not left waiting on this Cell.
                let mut late = [0u8; KMS_MESSAGE_LEN];
                let _ = syscall::sys_try_recv(self.tid, &mut late);
                Err(KmsClientError::Timeout)
            }
            syscall::SyscallResult::Ok(sender) if sender == self.tid => Ok(bytes),
            syscall::SyscallResult::Ok(_) => Err(KmsClientError::WrongSender),
            syscall::SyscallResult::Err(_) => Err(KmsClientError::Ipc),
        }
    }

    fn decode_payload<T>(
        &self,
        response: &KmsResponseV1,
        decode: fn(&[u8]) -> Option<T>,
    ) -> Result<T, KmsClientError> {
        let body = response.payload().map_err(KmsClientError::Wire)?;
        decode(body).ok_or(KmsClientError::InvalidPayload)
    }

    /// Require an empty success payload (opcodes 12 and 13).
    fn expect_empty(&self, response: &KmsResponseV1) -> Result<(), KmsClientError> {
        let body = response.payload().map_err(KmsClientError::Wire)?;
        if body.is_empty() {
            Ok(())
        } else {
            Err(KmsClientError::InvalidPayload)
        }
    }
}

/// A stack buffer wiped (volatile) on every exit path, built from an existing array.
struct Wiped<const N: usize>([u8; N]);

impl<const N: usize> Drop for Wiped<N> {
    fn drop(&mut self) {
        crate::session::zeroize(&mut self.0);
    }
}

/// Host-test seam: replaces the kernel round trip with a scripted service.
#[cfg(test)]
mod scripted {
    use super::{KmsClient, KmsClientError, AtomicBool, AtomicU32, AtomicU64, FIRST_PAYLOAD_ID, FIRST_REQUEST_ID};
    use super::KMS_MESSAGE_LEN;
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use core::cell::RefCell;

    /// Receives each request frame; `None` means the service stays silent.
    pub type Responder = Box<dyn FnMut(&[u8; KMS_MESSAGE_LEN]) -> Option<[u8; KMS_MESSAGE_LEN]>>;

    pub struct Script {
        pub sent: RefCell<Vec<([u8; KMS_MESSAGE_LEN], u64)>>,
        responder: RefCell<Responder>,
    }

    impl Script {
        pub fn exchange(
            &self,
            frame: &[u8; KMS_MESSAGE_LEN],
            ticks: u64,
        ) -> Result<[u8; KMS_MESSAGE_LEN], KmsClientError> {
            self.sent.borrow_mut().push((*frame, ticks));
            (*self.responder.borrow_mut())(frame).ok_or(KmsClientError::Timeout)
        }
    }

    impl KmsClient {
        /// A client whose round trips go to `responder`.
        pub fn scripted(responder: Responder) -> Self {
            Self {
                tid: 1,
                next_request_id: AtomicU32::new(FIRST_REQUEST_ID),
                next_payload_id: AtomicU64::new(FIRST_PAYLOAD_ID),
                poisoned: AtomicBool::new(false),
                script: Some(Script { sent: RefCell::new(Vec::new()), responder: RefCell::new(responder) }),
            }
        }

        /// Every request frame sent so far.
        pub fn sent_frames(&self) -> Vec<([u8; KMS_MESSAGE_LEN], u64)> {
            self.script.as_ref().map(|s| s.sent.borrow().clone()).unwrap_or_default()
        }
    }
}
