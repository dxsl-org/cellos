use api::caller_identity::CallerIdentity;
use types::kms::{
    BindingEpoch, KmsErrorCode, KmsOpcode, KmsRequestV1, KmsResponseV1,
    NoiseStaticDhRequestPayload, RotateNodeIdentityRequestPayload, KMS_NODE_KEY_ID_C2C,
};

use crate::auth::{authorize_supervisor, register_broker, BrokerBinding, ServiceRegistrySnapshot};
use crate::reply::SuccessPayload;
use crate::storage::{runtime_root, RootAssessment};

pub struct KmsService {
    binding: Option<BrokerBinding>,
    root: RootAssessment,
    next_binding_epoch: u64,
}

impl Default for KmsService {
    fn default() -> Self {
        Self::new()
    }
}

impl KmsService {
    pub fn new() -> Self {
        Self {
            binding: None,
            root: runtime_root(),
            next_binding_epoch: 1,
        }
    }

    /// Handle one canonical frame. Malformed envelopes are dropped fail-closed.
    pub fn handle(
        &mut self,
        frame: &[u8],
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Option<KmsResponseV1> {
        let request = KmsRequestV1::from_bytes(frame).ok()?;
        let opcode = request.opcode().ok()?;
        let result = match opcode {
            KmsOpcode::RegisterBrokerInstance => self.register(&request, sender, caller, registry),
            KmsOpcode::GetNodeIdentityStatus => self.status(&request, sender, caller, registry),
            KmsOpcode::AcquireNodeIdentity => {
                self.require_bound(&request, sender, caller, registry)
            }
            KmsOpcode::NoiseStaticDh => self.noise_dh(&request, sender, caller, registry),
            KmsOpcode::RotateNodeIdentity => self.rotate(&request, sender, caller, registry),
        };
        Some(match result {
            Ok(payload) => {
                KmsResponseV1::ok(opcode, request.request_id, payload.as_slice()).ok()?
            }
            Err(code) => KmsResponseV1::error(opcode, request.request_id, code),
        })
    }

    fn register(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        require_empty(request)?;
        let epoch = BindingEpoch(self.next_binding_epoch);
        let binding = register_broker(epoch, sender, caller, registry.net_broker_tid)?;
        self.next_binding_epoch = self
            .next_binding_epoch
            .checked_add(1)
            .filter(|next| *next != 0)
            .ok_or(KmsErrorCode::Busy)?;
        self.binding = Some(binding);
        Ok(SuccessPayload::new(&binding.payload().encode()))
    }

    fn status(
        &self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        require_empty(request)?;
        let broker_authorized = self.binding.is_some_and(|binding| {
            binding
                .authorizes(sender, caller, registry.net_broker_tid)
                .is_ok()
        });
        if !broker_authorized {
            authorize_supervisor(sender, caller, registry.supervisor_tid)?;
        }
        let binding_epoch = self
            .binding
            .map_or(BindingEpoch(0), |binding| binding.epoch);
        Ok(SuccessPayload::new(
            &self.root.status_payload(binding_epoch).encode(),
        ))
    }

    fn require_bound(
        &self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        require_empty(request)?;
        self.authorize_bound(sender, caller, registry)?;
        Err(KmsErrorCode::SecureRootRequired)
    }

    fn noise_dh(
        &self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        self.authorize_bound(sender, caller, registry)?;
        let payload = NoiseStaticDhRequestPayload::decode(
            request
                .payload()
                .map_err(|_| KmsErrorCode::InvalidPeerKey)?,
        )
        .ok_or(KmsErrorCode::InvalidPeerKey)?;
        let binding = self.binding.ok_or(KmsErrorCode::BindingRequired)?;
        if payload.handle.0 == 0 {
            return Err(KmsErrorCode::InvalidHandle);
        }
        if payload.key_id != KMS_NODE_KEY_ID_C2C
            || payload.binding_epoch != binding.epoch
            || payload.peer_public_key.iter().all(|byte| *byte == 0)
        {
            return Err(KmsErrorCode::InvalidPeerKey);
        }
        Err(KmsErrorCode::SecureRootRequired)
    }

    fn rotate(
        &self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        authorize_supervisor(sender, caller, registry.supervisor_tid)?;
        RotateNodeIdentityRequestPayload::decode(
            request
                .payload()
                .map_err(|_| KmsErrorCode::ProviderFailure)?,
        )
        .filter(|payload| payload.flags == 0)
        .ok_or(KmsErrorCode::ProviderFailure)?;
        Err(KmsErrorCode::SecureRootRequired)
    }

    fn authorize_bound(
        &self,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<(), KmsErrorCode> {
        self.binding
            .ok_or(KmsErrorCode::BindingRequired)?
            .authorizes(sender, caller, registry.net_broker_tid)
    }
}

fn require_empty(request: &KmsRequestV1) -> Result<(), KmsErrorCode> {
    request
        .payload()
        .ok()
        .filter(|payload| payload.is_empty())
        .map(|_| ())
        .ok_or(KmsErrorCode::ProviderFailure)
}
