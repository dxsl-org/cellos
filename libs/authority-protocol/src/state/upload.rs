mod helpers;
use helpers::{expected_chunk_len, upload_matches};

use super::{
    AuthorityState, ProfileChunkIntent, ProfileChunkMode, ProfileUploadIntent, ProtectedStore,
    RelayProfileState,
};
use crate::{
    AuthorityFault, BeginRelayProfileUploadRequest, ValidatedRequest, WriteRelayProfileChunkRequest,
};
impl<S: ProtectedStore> AuthorityState<S> {
    pub fn authorize_profile_upload(
        &mut self,
        validated: &ValidatedRequest<BeginRelayProfileUploadRequest>,
    ) -> Result<ProfileUploadIntent, AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        if request.pending_slot > 1
            || request.upload_handle == 0
            || request.profile_len == 0
            || request.profile_len as usize > crate::PROFILE_MAX_LEN
        {
            return self.seal(AuthorityFault::ProfileRejected);
        }
        match self.relay {
            RelayProfileState::Pending {
                generation,
                csr_handle,
                pending_slot,
            } if generation == request.generation && pending_slot == request.pending_slot => {
                let intent = self.upload_intent(request, csr_handle);
                self.persist_value(intent)
            }
            RelayProfileState::Uploading(intent) if upload_matches(request, &intent) => {
                self.persist_value(intent)
            }
            RelayProfileState::Pending { .. } | RelayProfileState::Uploading(_) => {
                self.seal(AuthorityFault::ProfileRejected)
            }
            _ => self.seal(AuthorityFault::InvalidState),
        }
    }
    pub fn acknowledge_profile_upload(
        &mut self,
        intent: &ProfileUploadIntent,
    ) -> Result<ProfileUploadIntent, AuthorityFault> {
        match self.relay {
            RelayProfileState::Pending {
                generation,
                csr_handle,
                pending_slot,
            } if generation == intent.generation
                && csr_handle == intent.csr_handle
                && pending_slot == intent.pending_slot
                && self.upload_intent_valid(intent) =>
            {
                self.relay = RelayProfileState::Uploading(*intent);
                self.persist_value(*intent)
            }
            RelayProfileState::Uploading(current) if current == *intent => Ok(current),
            _ => self.seal(AuthorityFault::ProfileRejected),
        }
    }
    pub fn authorize_profile_chunk(
        &mut self,
        validated: &ValidatedRequest<WriteRelayProfileChunkRequest>,
    ) -> Result<ProfileChunkIntent, AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        let upload = match self.relay {
            RelayProfileState::Uploading(intent)
                if intent.upload_handle == request.upload_handle =>
            {
                intent
            }
            RelayProfileState::Uploading(_) => return self.seal(AuthorityFault::ProfileRejected),
            _ => return self.seal(AuthorityFault::InvalidState),
        };
        let expected = match expected_chunk_len(&upload, request.chunk_index) {
            Some(value) => value,
            None => return self.seal(AuthorityFault::ProfileRejected),
        };
        if request.chunk.len() != expected || request.chunk_index > upload.next_index {
            return self.seal(AuthorityFault::ProfileRejected);
        }
        let intent = ProfileChunkIntent {
            upload,
            chunk_index: request.chunk_index,
            chunk: request.chunk,
            mode: if request.chunk_index < upload.next_index {
                ProfileChunkMode::VerifyExisting
            } else {
                ProfileChunkMode::Write
            },
        };
        self.persist_value(intent)
    }

    pub fn acknowledge_profile_chunk(
        &mut self,
        intent: &ProfileChunkIntent,
    ) -> Result<ProfileUploadIntent, AuthorityFault> {
        if self.relay != RelayProfileState::Uploading(intent.upload) {
            return self.seal(AuthorityFault::ProfileRejected);
        }
        if intent.mode == ProfileChunkMode::VerifyExisting {
            return Ok(intent.upload);
        }
        if intent.chunk_index != intent.upload.next_index {
            return self.seal(AuthorityFault::ProfileRejected);
        }
        let mut next = intent.upload;
        next.next_index = match next.next_index.checked_add(1) {
            Some(value) if value <= next.chunk_count() => value,
            _ => return self.seal(AuthorityFault::ProfileRejected),
        };
        self.relay = RelayProfileState::Uploading(next);
        self.persist_value(next)
    }

    pub fn reject_profile_upload(
        &mut self,
        intent: &ProfileUploadIntent,
    ) -> Result<(), AuthorityFault> {
        let matches = match self.relay {
            RelayProfileState::Pending { generation, .. } => generation == intent.generation,
            RelayProfileState::Uploading(current) => current == *intent,
            _ => false,
        };
        if !matches {
            return self.seal(AuthorityFault::InvalidState);
        }
        self.seal(AuthorityFault::ProfileRejected)
    }

    pub fn reject_profile_chunk(
        &mut self,
        intent: &ProfileChunkIntent,
    ) -> Result<(), AuthorityFault> {
        if self.relay != RelayProfileState::Uploading(intent.upload) {
            return self.seal(AuthorityFault::InvalidState);
        }
        self.seal(AuthorityFault::ProfileRejected)
    }

    fn upload_intent(
        &self,
        request: &BeginRelayProfileUploadRequest,
        csr_handle: u64,
    ) -> ProfileUploadIntent {
        ProfileUploadIntent {
            device_id: self.device_id,
            authority_id: self.authority_id,
            authority_epoch: self.authority_epoch,
            boot_epoch: request.context.boot_epoch,
            generation: request.generation,
            csr_handle,
            policy_epoch: request.policy_epoch,
            pending_slot: request.pending_slot,
            pending_spki_digest: request.pending_spki_digest,
            profile_digest: request.profile_digest,
            tpm_public_digest: request.tpm_public_digest,
            upload_handle: request.upload_handle,
            profile_len: request.profile_len,
            next_index: 0,
        }
    }

    fn upload_intent_valid(&self, intent: &ProfileUploadIntent) -> bool {
        self.device_id == intent.device_id
            && self.authority_id == intent.authority_id
            && self.authority_epoch == intent.authority_epoch
            && self.boot
                == (super::BootState::Open {
                    epoch: intent.boot_epoch,
                })
            && intent.pending_slot <= 1
            && intent.csr_handle != 0
            && intent.upload_handle != 0
            && intent.profile_len != 0
            && intent.profile_len as usize <= crate::PROFILE_MAX_LEN
            && intent.next_index == 0
    }
}
