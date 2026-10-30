//! Handlers for opcodes 15-24. Order (`code-standards.md` KMS rules): attested
//! caller, class, leaf state, payload decode, replay floor, state machine,
//! floor advance only after success, reply. Nothing is read, parsed or written
//! before the class check passes.

use api::caller_identity::CallerIdentity;
use sha2::{Digest, Sha256};
use types::kms::{
    KmsCapabilityReadiness, KmsErrorCode, KmsOpcode, SshAbortRequestPayload,
    SshChunkRequestPayload, SshDeleteRequestPayload, SshImportBeginRequestPayload,
    SshImportCommitRequestPayload, SshSignRequestPayload, SshSignResponsePayload,
    SshStatusRequestPayload, SshUnlockBeginRequestPayload, KMS_SSH_SECRET_MAX,
};
use zeroize::Zeroizing;

use crate::auth::{attested_caller, classify_ssh, SshClass};
use crate::reply::SuccessPayload;
use crate::ssh::limits::{FAIL_LIMIT, SIGN_INPUT_MAX};
use crate::ssh::sign::{reconstruct, sign};
use crate::ssh::store::{key_status, leaf_status, Secret};
use crate::ssh::unwrap::unwrap_seed;
use crate::ssh::{scrub_stack, LeafState, Principal, SshLeaf, SshPersist, Stored};
use types::kms::KmsCustodyPosture;

/// Opcode to class matrix (`k2-cp1.md` section 1.9): 15-18 and 20 admin; 19
/// admin or login; 21-24 login.
fn admitted(opcode: KmsOpcode, class: SshClass) -> bool {
    match opcode {
        KmsOpcode::BeginSshKeyImport
        | KmsOpcode::ImportSshKeyChunk
        | KmsOpcode::CommitSshKeyImport
        | KmsOpcode::AbortSshKeyImport
        | KmsOpcode::DeleteSshKey => class == SshClass::Admin,
        KmsOpcode::GetSshKeyStatus => matches!(class, SshClass::Admin | SshClass::Login),
        KmsOpcode::SignSshUserauth
        | KmsOpcode::BeginSshUnlock
        | KmsOpcode::SshUnlockChunk
        | KmsOpcode::AbortSshUnlock => class == SshClass::Login,
        _ => false,
    }
}

fn is_import_op(opcode: KmsOpcode) -> bool {
    matches!(
        opcode,
        KmsOpcode::BeginSshKeyImport
            | KmsOpcode::ImportSshKeyChunk
            | KmsOpcode::CommitSshKeyImport
            | KmsOpcode::AbortSshKeyImport
    )
}

fn invalid<T>(value: Option<T>) -> Result<T, KmsErrorCode> {
    value.ok_or(KmsErrorCode::InvalidRequest)
}

impl<P: SshPersist> SshLeaf<P> {
    /// Handle one SSH opcode. `payload` is the canonical frame payload.
    pub(crate) fn handle(
        &mut self,
        opcode: KmsOpcode,
        payload: &[u8],
        sender: usize,
        caller: Option<CallerIdentity>,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let caller = attested_caller(sender, caller)?;
        if !admitted(opcode, classify_ssh(&caller)) {
            return Err(KmsErrorCode::PermissionDenied);
        }
        let principal = Principal {
            cell_id: caller.cell_id,
            generation: caller.generation,
        };
        // The one opcode that answers whatever the leaf state: the leaf query
        // reports the true readiness (its payload is decoded first).
        if opcode == KmsOpcode::GetSshKeyStatus {
            return self.status(payload);
        }
        self.ensure_loaded();
        match self.state {
            LeafState::Serving => {}
            LeafState::Repairable(_) if is_import_op(opcode) => {}
            _ => return Err(KmsErrorCode::SshKeyUnavailable),
        }
        match opcode {
            KmsOpcode::BeginSshKeyImport => self.begin_import(principal, sender, payload),
            KmsOpcode::ImportSshKeyChunk => {
                let chunk = invalid(SshChunkRequestPayload::decode(payload))?;
                let ack = self.import_chunk(principal, sender, &chunk)?;
                SuccessPayload::new(&ack.encode())
            }
            KmsOpcode::CommitSshKeyImport => {
                let request = invalid(SshImportCommitRequestPayload::decode(payload))?;
                let status = self.import_commit(principal, sender, &request)?;
                SuccessPayload::new(&status.encode())
            }
            KmsOpcode::AbortSshKeyImport => {
                let request = invalid(SshAbortRequestPayload::decode(payload))?;
                self.import_abort(principal, &request)?;
                SuccessPayload::new(&[])
            }
            KmsOpcode::DeleteSshKey => self.delete_key(principal, payload),
            KmsOpcode::SignSshUserauth => self.sign_userauth(principal, payload),
            KmsOpcode::BeginSshUnlock => self.begin_unlock(principal, sender, payload),
            KmsOpcode::SshUnlockChunk => {
                let chunk = invalid(SshChunkRequestPayload::decode(payload))?;
                let ack = self.unlock_chunk(principal, sender, &chunk)?;
                SuccessPayload::new(&ack.encode())
            }
            KmsOpcode::AbortSshUnlock => {
                let request = invalid(SshAbortRequestPayload::decode(payload))?;
                self.unlock_abort(principal, &request)?;
                SuccessPayload::new(&[])
            }
            _ => Err(KmsErrorCode::UnknownOpcode),
        }
    }

    fn check_floor(&self, principal: Principal, request_id: u64) -> Result<(), KmsErrorCode> {
        if self.floors.admits(principal, request_id) {
            Ok(())
        } else {
            Err(KmsErrorCode::InvalidRequest)
        }
    }

    fn begin_import(
        &mut self,
        principal: Principal,
        tid: usize,
        payload: &[u8],
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let request = invalid(SshImportBeginRequestPayload::decode(payload))?;
        self.check_floor(principal, request.request_id)?;
        let response = self.import_begin(principal, tid, &request)?;
        self.floors.advance(principal, request.request_id);
        SuccessPayload::new(&response.encode())
    }

    fn begin_unlock(
        &mut self,
        principal: Principal,
        tid: usize,
        payload: &[u8],
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let request = invalid(SshUnlockBeginRequestPayload::decode(payload))?;
        self.check_floor(principal, request.request_id)?;
        let response = self.unlock_begin(principal, tid, &request)?;
        self.floors.advance(principal, request.request_id);
        SuccessPayload::new(&response.encode())
    }

    /// Op 19. Admin or login. An empty name is the leaf query, answered even
    /// when the leaf is not serving; a named query needs a serving leaf.
    fn status(&mut self, payload: &[u8]) -> Result<SuccessPayload, KmsErrorCode> {
        let request = invalid(SshStatusRequestPayload::decode(payload))?;
        self.ensure_loaded();
        if request.name_len == 0 {
            let readiness = match self.state {
                LeafState::Serving => KmsCapabilityReadiness::Ready,
                LeafState::Repairable(readiness) => readiness,
                LeafState::Unavailable | LeafState::NotLoaded => {
                    KmsCapabilityReadiness::Unavailable
                }
            };
            let key_count = u16::from(self.identity.is_some());
            return SuccessPayload::new(
                &leaf_status(readiness, self.posture, key_count, self.revision).encode(),
            );
        }
        if self.state != LeafState::Serving {
            return Err(KmsErrorCode::SshKeyUnavailable);
        }
        let identity = self
            .identity
            .as_ref()
            .filter(|identity| identity.name() == request.name())
            .ok_or(KmsErrorCode::SshKeyNotFound)?;
        SuccessPayload::new(
            &key_status(
                identity,
                KmsCapabilityReadiness::Ready,
                self.posture,
                self.revision,
            )
            .encode(),
        )
    }

    /// Op 20. The admin names the key and supplies the public key it believes
    /// is stored; any mismatch leaves the key intact. The tombstone is
    /// persisted before the RAM state changes.
    fn delete_key(
        &mut self,
        principal: Principal,
        payload: &[u8],
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let request = invalid(SshDeleteRequestPayload::decode(payload))?;
        self.check_floor(principal, request.request_id)?;
        self.identity
            .as_ref()
            .filter(|identity| {
                identity.name() == request.name() && *identity.public_key() == request.public_key
            })
            .ok_or(KmsErrorCode::SshKeyNotFound)?;
        let revision = self.next_revision()?;
        let stored = self
            .persist
            .tombstone(revision)
            .map_err(|_| KmsErrorCode::PersistFailed)?;
        self.posture = posture_of(stored);
        self.identity = None;
        self.wipe_unlock();
        self.failures = 0;
        self.revision = revision;
        self.floors.advance(principal, request.request_id);
        SuccessPayload::new(&[])
    }

    /// Op 21.
    fn sign_userauth(
        &mut self,
        principal: Principal,
        payload: &[u8],
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let request = invalid(SshSignRequestPayload::decode(payload))?;
        self.check_floor(principal, request.request_id)?;
        let public = *self
            .identity
            .as_ref()
            .filter(|identity| *identity.public_key() == request.public_key)
            .ok_or(KmsErrorCode::SshKeyNotFound)?
            .public_key();

        let wrapped = matches!(
            self.identity.as_ref().map(|identity| identity.secret()),
            Some(Secret::Wrapped { .. })
        );
        let seed = if wrapped {
            let mut passphrase = Zeroizing::new([0u8; KMS_SSH_SECRET_MAX]);
            let len = self.take_staged_secret(principal, &public, &mut passphrase)?;
            let outcome = match self.identity.as_ref().map(|identity| identity.secret()) {
                Some(Secret::Wrapped {
                    salt,
                    salt_len,
                    rounds,
                    ciphertext,
                    ciphertext_len,
                }) => match (
                    passphrase.get(..len),
                    salt.get(..usize::from(*salt_len)),
                    ciphertext.get(..usize::from(*ciphertext_len)),
                ) {
                    (Some(secret), Some(salt), Some(ciphertext)) => {
                        unwrap_seed(secret, salt, *rounds, ciphertext, &public).ok()
                    }
                    _ => None,
                },
                _ => None,
            };
            drop(passphrase);
            match outcome {
                Some(seed) => seed,
                None => {
                    // One code and one byte pattern for a wrong passphrase, a
                    // corrupt blob and a public key mismatch.
                    self.failures = self.failures.saturating_add(1).min(FAIL_LIMIT);
                    scrub_stack();
                    return Err(KmsErrorCode::SshUnlockFailed);
                }
            }
        } else {
            match self.identity.as_ref().map(|identity| identity.secret()) {
                Some(Secret::Plain(seed)) => Zeroizing::new(**seed),
                _ => return Err(KmsErrorCode::SshKeyNotFound),
            }
        };

        let mut input = Zeroizing::new([0u8; SIGN_INPUT_MAX]);
        let len = reconstruct(&request.session_id, request.username(), &public, &mut input);
        let Some(input_bytes) = input.get(..len) else {
            scrub_stack();
            return Err(KmsErrorCode::InvalidRequest);
        };
        let signature = match sign(&seed, input_bytes, &public) {
            Ok(signature) => signature,
            Err(_) => {
                // Not a passphrase failure: the counter is untouched and no
                // signature byte leaves the service.
                drop(seed);
                scrub_stack();
                return Err(KmsErrorCode::InvalidSignature);
            }
        };
        let message_sha256: [u8; 32] = Sha256::digest(input_bytes).into();
        drop(seed);
        drop(input);
        self.failures = 0;
        self.floors.advance(principal, request.request_id);
        scrub_stack();
        SuccessPayload::new(
            &SshSignResponsePayload {
                request_id: request.request_id,
                signature,
                message_sha256,
            }
            .encode(),
        )
    }
}

fn posture_of(stored: Stored) -> KmsCustodyPosture {
    match stored {
        Stored::Written => KmsCustodyPosture::DevPosturePersistent,
        Stored::VolatileOnly => KmsCustodyPosture::DevPostureVolatile,
    }
}
