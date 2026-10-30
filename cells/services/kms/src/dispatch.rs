mod enrollment;
mod node_identity;
mod relay;
#[cfg(feature = "ssh-dev-custody")]
mod ssh;

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
    #[cfg(feature = "ssh-dev-custody")]
    pub(crate) ssh: crate::ssh::SshLeaf<crate::ssh::NoPersist>,
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
            #[cfg(feature = "ssh-dev-custody")]
            ssh: crate::ssh::SshLeaf::serving(crate::ssh::NoPersist),
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
            // Opcodes 15-24 (single SSH identity custody). With the opt-in
            // `ssh-dev-custody` feature the leaf serves them (volatile T0
            // store); the default build has no leaf, so every one of them is
            // admitted through the attested-caller check and then answered
            // `SshKeyUnavailable`. There is deliberately no storage, parser,
            // KDF, signer, or key handling behind the default-build arm.
            #[cfg(feature = "ssh-dev-custody")]
            KmsOpcode::BeginSshKeyImport
            | KmsOpcode::ImportSshKeyChunk
            | KmsOpcode::CommitSshKeyImport
            | KmsOpcode::AbortSshKeyImport
            | KmsOpcode::GetSshKeyStatus
            | KmsOpcode::DeleteSshKey
            | KmsOpcode::SignSshUserauth
            | KmsOpcode::BeginSshUnlock
            | KmsOpcode::SshUnlockChunk
            | KmsOpcode::AbortSshUnlock => {
                self.ssh
                    .handle(opcode, request.payload().unwrap_or(&[]), sender, caller)
            }
            #[cfg(not(feature = "ssh-dev-custody"))]
            KmsOpcode::BeginSshKeyImport
            | KmsOpcode::ImportSshKeyChunk
            | KmsOpcode::CommitSshKeyImport
            | KmsOpcode::AbortSshKeyImport
            | KmsOpcode::GetSshKeyStatus
            | KmsOpcode::DeleteSshKey
            | KmsOpcode::SignSshUserauth
            | KmsOpcode::BeginSshUnlock
            | KmsOpcode::SshUnlockChunk
            | KmsOpcode::AbortSshUnlock => ssh_leaf_unavailable(sender, caller),
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

/// Answer for opcodes 15-24 in a build without the SSH leaf.
///
/// Admission is the attested-caller validation only (missing trailer, zero
/// generation, a zero cell id, or a `sender_tid` that is not the receive
/// sender is `CallerUnattested`). No class is checked and the payload is not
/// parsed. Nothing is read or written before the typed `SshKeyUnavailable`
/// answer.
#[cfg(not(feature = "ssh-dev-custody"))]
fn ssh_leaf_unavailable(
    sender: usize,
    caller: Option<CallerIdentity>,
) -> Result<SuccessPayload, KmsErrorCode> {
    crate::auth::attested_caller(sender, caller)?;
    Err(KmsErrorCode::SshKeyUnavailable)
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

#[cfg(all(test, not(feature = "ssh-dev-custody")))]
mod ssh_unavailable_tests {
    use super::*;
    use crate::storage::FixtureRelayProvider;
    use types::kms::{KmsCapabilityReadiness, RelayP256StatusPayload, KMS_PAYLOAD_LEN};

    const SSH_OPCODES: [KmsOpcode; 10] = [
        KmsOpcode::BeginSshKeyImport,
        KmsOpcode::ImportSshKeyChunk,
        KmsOpcode::CommitSshKeyImport,
        KmsOpcode::AbortSshKeyImport,
        KmsOpcode::GetSshKeyStatus,
        KmsOpcode::DeleteSshKey,
        KmsOpcode::SignSshUserauth,
        KmsOpcode::BeginSshUnlock,
        KmsOpcode::SshUnlockChunk,
        KmsOpcode::AbortSshUnlock,
    ];
    const NO_SERVICES: ServiceRegistrySnapshot = ServiceRegistrySnapshot {
        net_broker_tid: None,
        supervisor_tid: None,
        net_tid: None,
    };

    fn identity(cell_id: u64, generation: u64, tid: usize, flags: u32) -> CallerIdentity {
        CallerIdentity {
            flags,
            cell_id,
            generation,
            sender_tid: tid as u64,
        }
    }

    fn call(
        service: &mut KmsService,
        opcode: KmsOpcode,
        payload: &[u8],
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> KmsResponseV1 {
        let frame = KmsRequestV1::new(opcode, 41, payload).unwrap().to_bytes();
        service.handle(&frame, sender, caller, registry).unwrap()
    }

    fn assert_error(response: &KmsResponseV1, opcode: KmsOpcode, expected: KmsErrorCode) {
        assert_eq!(response.opcode().unwrap(), opcode);
        assert_eq!(response.request_id, 41);
        assert_eq!(response.error_code().unwrap(), Some(expected));
        assert!(response.payload().unwrap().is_empty());
    }

    #[test]
    fn unattested_callers_are_refused_before_anything_else() {
        let garbage = [0xA5u8; KMS_PAYLOAD_LEN];
        for opcode in SSH_OPCODES {
            for caller in [
                None,
                Some(identity(20, 0, 7, 0)),
                Some(identity(20, 30, 70, 0)),
            ] {
                let mut service = KmsService::new();
                let response = call(&mut service, opcode, &garbage, 7, caller, NO_SERVICES);
                assert_error(&response, opcode, KmsErrorCode::CallerUnattested);
            }
        }
    }

    #[test]
    fn zero_cell_id_is_unattested() {
        let mut service = KmsService::new();
        for opcode in SSH_OPCODES {
            let response = call(
                &mut service,
                opcode,
                &[],
                7,
                Some(identity(0, 30, 7, 0)),
                NO_SERVICES,
            );
            assert_error(&response, opcode, KmsErrorCode::CallerUnattested);
        }
    }

    #[test]
    fn attested_callers_get_the_unavailable_status_without_payload_parsing() {
        let valid_shaped = [0x11u8; KMS_PAYLOAD_LEN];
        for opcode in SSH_OPCODES {
            let mut service = KmsService::new();
            let mut reference: Option<[u8; 128]> = None;
            for (payload, flags) in [
                (&[][..], 0u32),
                (&valid_shaped[..], 0),
                (&valid_shaped[..8], u32::MAX),
            ] {
                let response = call(
                    &mut service,
                    opcode,
                    payload,
                    7,
                    Some(identity(20, 30, 7, flags)),
                    NO_SERVICES,
                );
                assert_error(&response, opcode, KmsErrorCode::SshKeyUnavailable);
                // Identical bytes whatever the payload: nothing was parsed.
                let bytes = response.to_bytes();
                assert_eq!(*reference.get_or_insert(bytes), bytes);
            }
            // Every attested identity gets the same answer, including the
            // registered supervisor and broker tids: no class exists yet.
            let registry = ServiceRegistrySnapshot {
                net_broker_tid: Some(7),
                supervisor_tid: Some(7),
                net_tid: Some(7),
            };
            let response = call(
                &mut service,
                opcode,
                &[],
                7,
                Some(identity(20, 30, 7, 0)),
                registry,
            );
            assert_error(&response, opcode, KmsErrorCode::SshKeyUnavailable);
        }
    }

    #[test]
    fn ssh_arms_change_no_service_state() {
        let mut service = KmsService::new();
        for opcode in SSH_OPCODES {
            for caller in [None, Some(identity(20, 30, 7, 0))] {
                call(&mut service, opcode, &[0u8; 8], 7, caller, NO_SERVICES);
            }
        }
        assert!(service.binding.is_none());
        assert!(service.service_net_binding.is_none());
        assert_eq!(service.next_binding_epoch, 1);
        assert_eq!(service.next_service_binding_epoch, 1);
        assert_eq!(service.last_tls_request_id, 0);
    }

    #[test]
    fn undefined_opcodes_and_noncanonical_frames_get_no_reply() {
        let mut service = KmsService::new();
        let attested = Some(identity(20, 30, 7, 0));
        let valid = KmsRequestV1::new(KmsOpcode::SignSshUserauth, 41, &[])
            .unwrap()
            .to_bytes();
        assert!(service.handle(&valid, 7, attested, NO_SERVICES).is_some());
        // Opcode 25 is not defined, a nonzero flags/reserved word or tail byte
        // is noncanonical, and version 2 is unsupported: all are dropped.
        for (at, value) in [(1, 25u8), (2, 1), (10, 1), (15, 1), (127, 1), (0, 2)] {
            let mut frame = valid;
            frame[at] = value;
            assert!(
                service.handle(&frame, 7, attested, NO_SERVICES).is_none(),
                "byte {at}"
            );
        }
    }

    #[test]
    fn relay_readiness_never_makes_the_ssh_leaf_available() {
        let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
        let net = ServiceRegistrySnapshot {
            net_broker_tid: None,
            supervisor_tid: Some(8),
            net_tid: Some(7),
        };
        let net_caller = Some(identity(50, 4, 7, 0));
        let bound = call(
            &mut service,
            KmsOpcode::RegisterServiceNetInstance,
            &[],
            7,
            net_caller,
            net,
        );
        assert_eq!(bound.error_code().unwrap(), None);
        let relay_status = |service: &mut KmsService| {
            let response = call(
                service,
                KmsOpcode::GetRelayP256Status,
                &[],
                7,
                net_caller,
                net,
            );
            RelayP256StatusPayload::decode(response.payload().unwrap()).unwrap()
        };
        let before = relay_status(&mut service);
        assert_eq!(before.readiness, KmsCapabilityReadiness::Ready);

        // The relay leaf is Ready; every SSH opcode still reports the SSH leaf
        // unavailable, even for the very identity that is the live net service.
        for opcode in SSH_OPCODES {
            let response = call(&mut service, opcode, &[], 7, net_caller, net);
            assert_error(&response, opcode, KmsErrorCode::SshKeyUnavailable);
        }
        // And the SSH calls did not disturb the relay leaf.
        assert_eq!(relay_status(&mut service), before);
    }
}
