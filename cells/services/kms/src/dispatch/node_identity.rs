use api::caller_identity::CallerIdentity;
use types::kms::{
    BindingEpoch, KmsErrorCode, KmsRequestV1, NoiseStaticDhRequestPayload,
    RotateNodeIdentityRequestPayload, KMS_NODE_KEY_ID_C2C,
};

use crate::auth::{authorize_supervisor, ServiceRegistrySnapshot};
use crate::reply::SuccessPayload;

use super::{require_empty, KmsService};

impl KmsService {
    pub(super) fn status(
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
        SuccessPayload::new(&self.root.status_payload(binding_epoch).encode())
    }

    pub(super) fn require_bound(
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

    pub(super) fn noise_dh(
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

    pub(super) fn rotate(
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
