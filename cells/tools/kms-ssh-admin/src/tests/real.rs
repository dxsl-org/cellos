//! Integration: the admin cell (parse, readers, armor, chunk driver, commands) against the
//! real `service-kms` state machine built with `ssh-dev-custody`, in process.
//!
//! The K6 `KmsClient` cannot be used here: its test seam is `#[cfg(test)]` inside `ostd`
//! and is therefore not visible to another crate (K6 dossier section 6). The bridge below
//! plays the client's part instead: it encodes the K2 payloads with the shared `types`
//! codecs, wraps them in the 128-byte frames, hands them to `KmsService::handle` with a
//! synthetic attested caller, and applies the same envelope checks as `call_bounded`. What
//! is therefore exercised end to end is the cell's driver against the service; the client
//! methods themselves are covered by the K6 tests of `ostd`.

use core::cell::{Cell, RefCell};
use std::string::String;
use std::vec::Vec;

use api::caller_identity::{CallerIdentity, CALLER_FLAG_KMS_SSH_ADMIN, CALLER_FLAG_KMS_SSH_SIGN};
use ostd::clients::kms::{
    KmsClientError, SshChunkAckPayload, SshKeyStatusPayload, SshTransferBeginResponsePayload,
    KMS_SSH_CHUNK_LEN, KMS_SSH_PUBLIC_KEY_LEN,
};
use service_kms::{KmsService, ServiceRegistrySnapshot};
use types::kms::{
    KmsOpcode, KmsRequestV1, SshAbortRequestPayload, SshChunkRequestPayload,
    SshDeleteRequestPayload, SshImportBeginRequestPayload, SshImportCommitRequestPayload,
    SshStatusRequestPayload,
};

use super::{
    armor_text, key, pasted, plain_blob, public_blob, run_cell, text, wrapped_blob, TestConsole,
    ENTER,
};
use crate::kms::Kms;

pub struct Bridge<'s> {
    service: &'s RefCell<KmsService>,
    flags: u32,
    cell: u64,
    frame_id: Cell<u32>,
    payload_id: Cell<u64>,
    /// Every frame sent, as the service saw it.
    pub frames: RefCell<Vec<[u8; 128]>>,
    /// Frames after which the service stops answering (a timeout), if set.
    pub drop_after: Cell<Option<usize>>,
}

impl<'s> Bridge<'s> {
    fn new(service: &'s RefCell<KmsService>, flags: u32, cell: u64) -> Self {
        Self {
            service,
            flags,
            cell,
            frame_id: Cell::new(1),
            payload_id: Cell::new(1),
            frames: RefCell::new(Vec::new()),
            drop_after: Cell::new(None),
        }
    }

    fn admin(service: &'s RefCell<KmsService>) -> Self {
        Self::new(service, CALLER_FLAG_KMS_SSH_ADMIN, 100)
    }

    fn payload_id(&self) -> u64 {
        let id = self.payload_id.get();
        self.payload_id.set(id + 1);
        id
    }

    fn opcodes(&self) -> Vec<u8> {
        self.frames.borrow().iter().map(|f| f[1]).collect()
    }

    fn call(&self, opcode: KmsOpcode, payload: &[u8]) -> Result<Vec<u8>, KmsClientError> {
        let id = self.frame_id.get();
        self.frame_id.set(id + 1);
        let frame = KmsRequestV1::new(opcode, id, payload)
            .expect("payload fits")
            .to_bytes();
        let sent = self.frames.borrow().len();
        self.frames.borrow_mut().push(frame);
        if self.drop_after.get().is_some_and(|n| sent >= n) {
            return Err(KmsClientError::Timeout);
        }
        let identity = (self.flags != 0 || self.cell != 0).then_some(CallerIdentity {
            flags: self.flags,
            cell_id: self.cell,
            generation: 1,
            sender_tid: 10,
        });
        let registry = ServiceRegistrySnapshot {
            net_broker_tid: None,
            supervisor_tid: Some(8),
            net_tid: None,
        };
        let response = self
            .service
            .borrow_mut()
            .handle(&frame, 10, identity, registry)
            .ok_or(KmsClientError::Timeout)?;
        if response.opcode != opcode as u8 || response.request_id != id {
            return Err(KmsClientError::MismatchedResponse);
        }
        match response.error_code().map_err(KmsClientError::Wire)? {
            Some(code) => Err(KmsClientError::Service(code)),
            None => Ok(response.payload().map_err(KmsClientError::Wire)?.to_vec()),
        }
    }
}

fn name_field(name: &[u8]) -> [u8; 64] {
    let mut field = [0u8; 64];
    field[..name.len()].copy_from_slice(name);
    field
}

impl Kms for Bridge<'_> {
    fn status(&self, name: &[u8]) -> Result<SshKeyStatusPayload, KmsClientError> {
        let payload = SshStatusRequestPayload {
            name_len: name.len() as u8,
            name: name_field(name),
        }
        .encode();
        let reply = self.call(KmsOpcode::GetSshKeyStatus, &payload)?;
        let status = SshKeyStatusPayload::decode(&reply).ok_or(KmsClientError::InvalidPayload)?;
        if !name.is_empty() && status.key_present != 1 {
            return Err(KmsClientError::MismatchedResponse);
        }
        Ok(status)
    }

    fn import_begin(
        &self,
        name: &[u8],
        total_len: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshTransferBeginResponsePayload, KmsClientError> {
        let payload = SshImportBeginRequestPayload {
            request_id: self.payload_id(),
            total_len,
            name_len: name.len() as u8,
            reserved: [0; 3],
            blob_sha256: *blob_sha256,
            name: name_field(name),
        }
        .encode();
        let reply = self.call(KmsOpcode::BeginSshKeyImport, &payload)?;
        SshTransferBeginResponsePayload::decode(&reply).ok_or(KmsClientError::InvalidPayload)
    }

    fn import_chunk(
        &self,
        handle: u64,
        index: u32,
        data: &[u8],
    ) -> Result<SshChunkAckPayload, KmsClientError> {
        let mut chunk = [0u8; KMS_SSH_CHUNK_LEN];
        chunk[..data.len()].copy_from_slice(data);
        let payload = SshChunkRequestPayload {
            handle,
            chunk_index: index,
            chunk_len: data.len() as u16,
            reserved: 0,
            chunk,
        }
        .encode();
        let reply = self.call(KmsOpcode::ImportSshKeyChunk, &payload)?;
        SshChunkAckPayload::decode(&reply).ok_or(KmsClientError::InvalidPayload)
    }

    fn import_commit(
        &self,
        handle: u64,
        chunk_count: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshKeyStatusPayload, KmsClientError> {
        let payload = SshImportCommitRequestPayload {
            handle,
            chunk_count,
            reserved: 0,
            blob_sha256: *blob_sha256,
        }
        .encode();
        let reply = self.call(KmsOpcode::CommitSshKeyImport, &payload)?;
        SshKeyStatusPayload::decode(&reply).ok_or(KmsClientError::InvalidPayload)
    }

    fn import_abort(&self, handle: u64) -> Result<(), KmsClientError> {
        let reply = self.call(
            KmsOpcode::AbortSshKeyImport,
            &SshAbortRequestPayload { handle }.encode(),
        )?;
        if reply.is_empty() {
            Ok(())
        } else {
            Err(KmsClientError::MismatchedResponse)
        }
    }

    fn delete(
        &self,
        name: &[u8],
        public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN],
    ) -> Result<(), KmsClientError> {
        let payload = SshDeleteRequestPayload {
            request_id: self.payload_id(),
            public_key: *public_key,
            name_len: name.len() as u8,
            name: name_field(name),
        }
        .encode();
        let reply = self.call(KmsOpcode::DeleteSshKey, &payload)?;
        if reply.is_empty() {
            Ok(())
        } else {
            Err(KmsClientError::MismatchedResponse)
        }
    }
}

fn service() -> RefCell<KmsService> {
    RefCell::new(KmsService::new())
}

fn import(bridge: &Bridge<'_>, name: &str, blob: &[u8]) -> (i32, TestConsole) {
    let mut console = TestConsole::new().with_block(pasted(&armor_text(blob)));
    let code = run_cell(&["import", name], &mut console, bridge);
    (code, console)
}

fn expected_fingerprint() -> String {
    // SHA-256 of the 51-byte SSH wire public key blob, unpadded base64 (`ssh-keygen -lf`).
    crate::report::fingerprint(&ostd::session::sha256(&public_blob()))
}

#[test]
fn an_admin_imports_a_plain_key_and_status_reports_it() {
    let service = service();
    let bridge = Bridge::admin(&service);
    let blob = plain_blob(7);
    let (code, console) = import(&bridge, "lane", &blob);
    assert_eq!(code, 0, "{}{}", console.out, console.err);
    let fingerprint = expected_fingerprint();
    assert!(
        console.out.contains("imported key 'lane'"),
        "{}",
        console.out
    );
    assert!(console.out.contains("custody: plain"));
    assert!(
        console
            .out
            .contains(&std::format!("fingerprint: {fingerprint}\n")),
        "{}",
        console.out
    );
    assert!(console
        .out
        .contains("posture: DEV_POSTURE volatile: lost when the key service restarts"));
    assert!(console.out.contains(&std::format!(
        "blob: {} bytes, sha256 prefix {}",
        blob.len(),
        crate::report::digest_prefix(&ostd::session::sha256(&blob))
    )));
    assert!(console
        .out
        .contains("WARNING: unencrypted key held by the key service"));
    // Begin, ceil(len/96) chunks, Commit.
    let chunks = blob.len().div_ceil(96);
    let mut want = std::vec![15u8];
    want.extend(std::iter::repeat_n(16u8, chunks));
    want.push(17);
    assert_eq!(bridge.opcodes(), want);

    // status: leaf and named.
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["status"], &mut console, &bridge), 0);
    assert!(
        console.out.contains("readiness: Ready\n  keys: 1\n"),
        "{}",
        console.out
    );
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["status", "lane"], &mut console, &bridge), 0);
    assert!(console.out.contains(&std::format!(
        "kms-ssh-admin: key 'lane'\n  custody: plain\n  fingerprint: {fingerprint}\n"
    )));
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["status", "other"], &mut console, &bridge), 1);
    assert_eq!(console.err, "kms-ssh-admin: no such key\n");
}

#[test]
fn a_wrapped_key_is_imported_as_is_without_a_warning() {
    let service = service();
    let bridge = Bridge::admin(&service);
    let (code, console) = import(&bridge, "lane", &wrapped_blob());
    assert_eq!(code, 0, "{}{}", console.out, console.err);
    assert!(console.out.contains("custody: passphrase-wrapped"));
    assert!(!console.out.contains("WARNING"));
    assert!(console.out.contains(&expected_fingerprint()));
}

#[test]
fn a_maximal_blob_takes_685_round_trips_and_is_stored() {
    let service = service();
    let bridge = Bridge::admin(&service);
    // The largest container reachable: the private section is padded to a multiple of 8, so
    // exactly 65,536 bytes may not be hit; anything above 65,472 takes 683 chunks.
    let comment = (65_000..=65_536)
        .rev()
        .find(|c| plain_blob(*c).len() <= 65_536)
        .expect("a size exists");
    let blob = plain_blob(comment);
    assert!((65_529..=65_536).contains(&blob.len()), "{}", blob.len());
    let (code, console) = import(&bridge, "lane", &blob);
    assert_eq!(code, 0, "{}{}", console.out, console.err);
    assert_eq!(bridge.frames.borrow().len(), 685);
    assert!(console
        .out
        .contains(&std::format!("blob: {} bytes", blob.len())));
}

#[test]
fn begin_chunk_and_commit_frames_are_the_k2_layouts() {
    let service = service();
    let bridge = Bridge::admin(&service);
    let blob = plain_blob(7);
    let (code, _) = import(&bridge, "lane", &blob);
    assert_eq!(code, 0);
    let frames = bridge.frames.borrow();
    let begin = KmsRequestV1::from_bytes(&frames[0]).unwrap();
    let begin = SshImportBeginRequestPayload::decode(begin.payload().unwrap()).unwrap();
    assert_eq!(
        (begin.request_id, begin.total_len as usize, begin.name_len),
        (1, blob.len(), 4)
    );
    assert_eq!(begin.blob_sha256, ostd::session::sha256(&blob));
    assert_eq!(&begin.name[..4], b"lane");
    let mut rebuilt = Vec::new();
    for (i, frame) in frames[1..frames.len() - 1].iter().enumerate() {
        let payload = KmsRequestV1::from_bytes(frame).unwrap();
        let chunk = SshChunkRequestPayload::decode(payload.payload().unwrap()).unwrap();
        assert_eq!(chunk.chunk_index as usize, i);
        assert!(chunk.chunk[chunk.chunk_len as usize..]
            .iter()
            .all(|b| *b == 0));
        rebuilt.extend_from_slice(&chunk.chunk[..chunk.chunk_len as usize]);
    }
    assert_eq!(rebuilt, blob);
    let commit = KmsRequestV1::from_bytes(frames.last().unwrap()).unwrap();
    let commit = SshImportCommitRequestPayload::decode(commit.payload().unwrap()).unwrap();
    assert_eq!(commit.chunk_count as usize, blob.len().div_ceil(96));
    assert_eq!(commit.blob_sha256, begin.blob_sha256);
}

#[test]
fn a_second_import_is_refused_until_the_identity_is_deleted() {
    let service = service();
    let bridge = Bridge::admin(&service);
    assert_eq!(import(&bridge, "lane", &plain_blob(7)).0, 0);
    // Same name, then another name.
    for name in ["lane", "other"] {
        let (code, console) = import(&bridge, name, &plain_blob(9));
        assert_eq!(code, 1, "{name}");
        assert!(
            console.err.contains("delete the existing identity first"),
            "{}",
            console.err
        );
        assert!(console.out.is_empty());
    }
    // Delete (typed confirmation), then the leaf is empty and an import works again.
    let mut console = TestConsole::new().with_word(std::vec![text("delete"), key(ENTER)]);
    assert_eq!(
        run_cell(&["delete", "lane"], &mut console, &bridge),
        0,
        "{}{}",
        console.out,
        console.err
    );
    assert!(console.out.contains(&expected_fingerprint()));
    assert!(console.out.ends_with("kms-ssh-admin: deleted key 'lane'\n"));
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["status"], &mut console, &bridge), 0);
    assert!(console.out.contains("keys: 0"), "{}", console.out);
    assert_eq!(import(&bridge, "again", &plain_blob(7)).0, 0);
}

#[test]
fn a_delete_without_the_typed_word_leaves_the_key() {
    let service = service();
    let bridge = Bridge::admin(&service);
    assert_eq!(import(&bridge, "lane", &plain_blob(7)).0, 0);
    let before = bridge.frames.borrow().len();
    let mut console = TestConsole::new().with_word(std::vec![text("no"), key(ENTER)]);
    assert_eq!(run_cell(&["delete", "lane"], &mut console, &bridge), 130);
    // Only the status frame was sent: no op 20.
    let sent: Vec<u8> = bridge.frames.borrow()[before..]
        .iter()
        .map(|f| f[1])
        .collect();
    assert_eq!(sent, [19]);
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["status", "lane"], &mut console, &bridge), 0);
}

#[test]
fn a_blob_kms_refuses_is_aborted_and_the_store_stays_empty() {
    let service = service();
    let bridge = Bridge::admin(&service);
    // Armor-valid, not an OpenSSH container.
    let (code, console) = import(&bridge, "lane", &super::filler(250));
    assert_eq!(code, 1);
    assert!(
        console.err.contains("key format refused"),
        "{}",
        console.err
    );
    let ops = bridge.opcodes();
    assert_eq!(ops[ops.len() - 2..], [17, 18], "Commit refused, then Abort");
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["status"], &mut console, &bridge), 0);
    assert!(console.out.contains("keys: 0"));
    // The same run still works with a real container.
    assert_eq!(import(&bridge, "lane", &plain_blob(7)).0, 0);
}

#[test]
fn a_caller_without_the_admin_flag_is_refused_on_every_command() {
    for (flags, cell) in [
        (0, 100),
        (CALLER_FLAG_KMS_SSH_SIGN, 200),
        (CALLER_FLAG_KMS_SSH_SIGN | CALLER_FLAG_KMS_SSH_ADMIN, 300),
    ] {
        let service = service();
        let bridge = Bridge::new(&service, flags, cell);
        let (code, console) = import(&bridge, "lane", &plain_blob(7));
        assert_eq!(code, 1);
        assert_eq!(
            console.err,
            std::format!(
                "{}\nkms-ssh-admin: not an admin caller (admin flag or policy missing)\n",
                crate::report::IMPORT_PROMPT
            )
        );
        assert_eq!(bridge.opcodes(), [15], "the first refusal ends the run");
        let mut console = TestConsole::new().with_word(std::vec![text("delete"), key(ENTER)]);
        let code = run_cell(&["delete", "lane"], &mut console, &bridge);
        assert_eq!(code, 1);
        assert_eq!(
            console.word_reads, 0,
            "no prompt before the status is admitted"
        );
    }
    // The login flag alone may read status (op 19 is admin-or-login) but the cell never signs:
    // no frame of the whole matrix has an opcode outside 15..=20.
    let service = service();
    let bridge = Bridge::new(&service, CALLER_FLAG_KMS_SSH_SIGN, 200);
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["status"], &mut console, &bridge), 0);
}

#[test]
fn no_run_ever_sends_an_opcode_outside_15_to_20() {
    let service = service();
    let bridge = Bridge::admin(&service);
    let _ = import(&bridge, "lane", &plain_blob(7));
    let _ = import(&bridge, "lane", &plain_blob(7));
    let _ = import(&bridge, "x", &super::filler(10));
    let mut console = TestConsole::new().with_word(std::vec![text("delete"), key(ENTER)]);
    let _ = run_cell(&["delete", "lane"], &mut console, &bridge);
    let mut console = TestConsole::new();
    let _ = run_cell(&["status", "lane"], &mut console, &bridge);
    let _ = run_cell(&["status"], &mut console, &bridge);
    for opcode in bridge.opcodes() {
        assert!((15..=20).contains(&opcode), "opcode {opcode}");
    }
}

#[test]
fn a_silent_service_is_exit_124_and_sends_no_abort() {
    let service = service();
    let bridge = Bridge::admin(&service);
    bridge.drop_after.set(Some(2));
    let (code, console) = import(&bridge, "lane", &plain_blob(300));
    assert_eq!(code, 124);
    assert!(console
        .err
        .ends_with("kms-ssh-admin: the key service did not answer in time\n"));
    assert_eq!(
        bridge.opcodes(),
        [15, 16, 16],
        "the third frame went unanswered; nothing was sent after it"
    );
}

#[test]
fn the_service_error_codes_the_cell_can_see_are_the_documented_ones() {
    // Every refusal produced by the real service during the scenarios above maps to a row of
    // the table, not to the generic "refused: <name>" fallback.
    let service = service();
    let bridge = Bridge::admin(&service);
    assert_eq!(import(&bridge, "lane", &plain_blob(7)).0, 0);
    for (name, blob) in [
        ("lane", plain_blob(7)),
        ("other", plain_blob(7)),
        ("x", super::filler(40)),
    ] {
        let (_, console) = import(&bridge, name, &blob);
        assert!(
            !console.err.contains("key service refused:"),
            "{}",
            console.err
        );
    }
}
