mod enrollment;
mod node_identity;
mod relay;

use api::caller_identity::CallerIdentity;
use types::kms::{
    BindingEpoch, KmsErrorCode, KmsOpcode, KmsRequestV1, KmsResponseV1, ServiceNetBindingEpoch,
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
    pub(super) lifecycle: crate::lifecycle::RelayLifecycle,
    #[cfg(test)]
    protected_lifecycle: Option<crate::lifecycle::ProtectedRelayState>,
}

impl Default for KmsService {
    fn default() -> Self {
        Self::new()
    }
}

impl KmsService {
    pub fn new() -> Self {
        Self::from_provider(ProviderSlot::development_runtime())
    }

    fn from_provider(provider: ProviderSlot) -> Self {
        Self {
            binding: None,
            service_net_binding: None,
            root: runtime_root(&provider),
            lifecycle: boot_lifecycle(),
            provider,
            next_binding_epoch: 1,
            next_service_binding_epoch: 1,
            last_tls_request_id: 0,
            #[cfg(test)]
            protected_lifecycle: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_provider_fixture(provider: crate::storage::FixtureRelayProvider) -> Self {
        static RESTARTS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
        let restart_epoch = RESTARTS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let mut service = Self::from_provider(ProviderSlot::Fixture(provider));
        service.lifecycle = crate::lifecycle::RelayLifecycle::with_entropy(restart_epoch);
        // The fixture lane starts serving the pinned development generation.
        service
            .lifecycle
            .activate_for_tests(crate::lifecycle::ActiveRelayGeneration {
                generation: crate::storage::FIXTURE_RELAY_GENERATION,
                policy_epoch: 11,
                profile_digest: crate::storage::FIXTURE_PROFILE_DIGEST,
                revoked: false,
            });
        service.protected_lifecycle = service.lifecycle.protected_state().ok();
        service
    }

    #[cfg(test)]
    pub(crate) fn with_recovered_provider_fixture(
        provider: crate::storage::FixtureRelayProvider,
        protected: crate::lifecycle::ProtectedRelayState,
        restart_epoch: u64,
    ) -> Result<Self, KmsErrorCode> {
        if let Some(active) = protected.active {
            provider.active_generation.set(active.generation);
            provider.active_profile_digest.set(active.profile_digest);
            provider
                .authenticated_time_floor
                .set(protected.authenticated_time_floor);
        }
        let mut service = Self::from_provider(ProviderSlot::Fixture(provider));
        service.lifecycle = crate::lifecycle::RelayLifecycle::recover(restart_epoch, protected)?;
        service.protected_lifecycle = Some(service.lifecycle.protected_state()?);
        Ok(service)
    }

    pub(super) fn persist_protected_lifecycle(&mut self) -> Result<(), KmsErrorCode> {
        #[cfg(test)]
        {
            self.protected_lifecycle = Some(self.lifecycle.protected_state()?);
            Ok(())
        }
        #[cfg(not(test))]
        {
            let protected = self.lifecycle.protected_state()?;
            if crate::storage::persist_runtime_protected_relay_state(protected).is_err() {
                self.lifecycle.seal();
                return Err(KmsErrorCode::RelayUnavailable);
            }
            Ok(())
        }
    }

    #[cfg(test)]
    pub(crate) fn protected_lifecycle_for_tests(
        &self,
    ) -> Option<crate::lifecycle::ProtectedRelayState> {
        self.protected_lifecycle
    }

    /// Exercise the development Silo through the single relay provider seam.
    ///
    /// This boot-only probe is compiled only for the AArch64 reference lane and
    /// succeeds only for a self-verified, already-low-S TLS signature.
    #[cfg(all(
        feature = "development-silo-provider",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    pub fn development_silo_boot_probe(&mut self) -> bool {
        let status = self.provider.relay_p256_status();
        if status.metadata.provider != types::kms::KmsProviderKind::SiloWrapped
            || status.metadata.assessment
                != types::kms::RelayProviderAssessment::DevelopmentReference
            || status.metadata.readiness != types::kms::KmsCapabilityReadiness::Ready
        {
            return false;
        }
        let transcript_hash = [0x5a; 32];
        let Ok(signature) = self.provider.sign_tls13_client_certificate_verify(
            transcript_hash,
            status.metadata.relay_generation,
            status.metadata.active_profile_digest,
            1,
        ) else {
            return false;
        };
        matches!(
            crate::storage::normalize_and_verify_tls13_signature(
                transcript_hash,
                &status.verifying_key_sec1,
                signature,
            ),
            Ok(verified) if verified == signature
        )
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
            KmsOpcode::GetRelayP256Status => self.relay_status(&request, sender, caller, registry),
            KmsOpcode::SignTls13ClientCertificateVerify => {
                self.sign_tls13(&request, sender, caller, registry)
            }
            KmsOpcode::BeginRelayEnrollment => {
                self.begin_enrollment(&request, sender, caller, registry)
            }
            KmsOpcode::ReadRelayCsrChunk => {
                self.read_relay_csr_chunk(&request, sender, caller, registry)
            }
            KmsOpcode::CommitRelayGeneration => {
                self.commit_relay_generation(&request, sender, caller, registry)
            }
            KmsOpcode::AbortRelayEnrollment => {
                self.abort_relay_enrollment(&request, sender, caller, registry)
            }
            KmsOpcode::StageRelayProfile => {
                self.stage_relay_profile(&request, sender, caller, registry)
            }
            KmsOpcode::GetRelayActivePublicKey => {
                self.get_relay_active_public_key(&request, sender, caller, registry)
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

/// Boot only from authenticated protected lifecycle state and a strictly
/// newer protected restart epoch. Unavailable, torn, or regressed state seals
/// enrollment and serving; no process-local counter substitutes for it.
fn boot_lifecycle() -> crate::lifecycle::RelayLifecycle {
    #[cfg(not(test))]
    {
        match crate::storage::load_runtime_protected_relay_state() {
            Ok((restart_epoch, protected)) => {
                crate::lifecycle::RelayLifecycle::recover(restart_epoch, protected)
                    .unwrap_or_else(|_| crate::lifecycle::RelayLifecycle::sealed())
            }
            Err(_) => crate::lifecycle::RelayLifecycle::sealed(),
        }
    }
    #[cfg(test)]
    {
        crate::lifecycle::RelayLifecycle::sealed()
    }
}
