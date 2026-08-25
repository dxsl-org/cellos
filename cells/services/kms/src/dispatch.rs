mod node_identity;
mod relay;

use api::caller_identity::CallerIdentity;
use types::kms::{
    BindingEpoch, KmsErrorCode, KmsOpcode, KmsRequestV1, KmsResponseV1,
    ServiceNetBindingEpoch,
};

use crate::auth::{
    register_broker, register_service_net, BrokerBinding, ServiceNetBinding,
    ServiceRegistrySnapshot,
};
use crate::reply::SuccessPayload;
use crate::storage::{runtime_root, ProviderSlot, RootAssessment};

pub struct KmsService {
    binding: Option<BrokerBinding>,
    service_net_binding: Option<ServiceNetBinding>,
    provider: ProviderSlot,
    root: RootAssessment,
    next_binding_epoch: u64,
    next_service_binding_epoch: u64,
    last_tls_request_id: u64,
}

impl Default for KmsService {
    fn default() -> Self {
        Self::new()
    }
}

impl KmsService {
    pub fn new() -> Self {
        Self::from_provider(ProviderSlot::Unavailable)
    }

    fn from_provider(provider: ProviderSlot) -> Self {
        Self {
            binding: None,
            service_net_binding: None,
            root: runtime_root(&provider),
            provider,
            next_binding_epoch: 1,
            next_service_binding_epoch: 1,
            last_tls_request_id: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_provider_fixture(provider: crate::storage::FixtureRelayProvider) -> Self {
        Self::from_provider(ProviderSlot::Fixture(provider))
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
            KmsOpcode::RegisterServiceNetInstance => {
                self.register_service_net(&request, sender, caller, registry)
            }
            KmsOpcode::GetRelayP256Status => {
                self.relay_status(&request, sender, caller, registry)
            }
            KmsOpcode::SignTls13ClientCertificateVerify => {
                self.sign_tls13(&request, sender, caller, registry)
            }
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
        SuccessPayload::new(&binding.payload().encode())
    }

    fn register_service_net(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        require_empty(request)?;
        let epoch = ServiceNetBindingEpoch(self.next_service_binding_epoch);
        let binding = register_service_net(epoch, sender, caller, registry.net_tid)?;
        let next_epoch = self
            .next_service_binding_epoch
            .checked_add(1)
            .filter(|next| *next != 0)
            .ok_or(KmsErrorCode::Busy)?;
        let response = SuccessPayload::new(&binding.payload().encode())?;
        self.next_service_binding_epoch = next_epoch;
        self.service_net_binding = Some(binding);
        self.last_tls_request_id = 0;
        Ok(response)
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

