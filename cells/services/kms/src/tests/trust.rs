//! SSH host trust (opcodes 25-29) end to end through `KmsService::handle`, over an
//! in-memory store with the VFS conditional-save semantics, plus the store parser's
//! refusal of every malformed form.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use abi::caller_identity::{CallerIdentity, CALLER_FLAG_NET_CONNECT, CALLER_FLAG_VFS_MUTATE};
use kms_codec::{
    KmsErrorCode, KmsOpcode, KmsRequestV1, KmsTrustQueryKind, SshAbortRequestPayload,
    SshChunkAckPayload, SshChunkRequestPayload, SshImportCommitRequestPayload,
    SshTransferBeginResponsePayload, TrustChallengeCommitRequestPayload, TrustOutcomePayload,
    TrustQueryBeginRequestPayload, KMS_SSH_CHUNK_LEN,
};
use sha2::{Digest, Sha256};
use wire::ipc::{ssh_trust, Observation, SaveExpect, SaveOutcome};

use crate::trust::{lookup, TrustIo};
use crate::{KmsService, ServiceRegistrySnapshot};

/// The store as KMS sees it through the VFS: `Observe` identity, a bounded read and a
/// conditional publication that conflicts unless the destination still matches.
pub struct MemStore {
    pub file: Option<Vec<u8>>,
    pub dir: bool,
    /// Every operation fails (a VFS that cannot be reached).
    pub broken: bool,
    inode: u64,
    generation: u64,
}

impl MemStore {
    pub fn new() -> Self {
        Self {
            file: None,
            dir: false,
            broken: false,
            inode: 0,
            generation: 0,
        }
    }

    /// Seed the store the way the provisioner does: straight onto the volume.
    pub fn seed(&mut self, text: &str) {
        self.dir = true;
        self.inode = 77;
        self.generation = 1;
        self.file = Some(text.as_bytes().to_vec());
    }

    fn fail(&self) -> Result<(), ()> {
        if self.broken {
            Err(())
        } else {
            Ok(())
        }
    }
}

impl TrustIo for MemStore {
    fn observe(&mut self) -> Result<Observation, ()> {
        self.fail()?;
        Ok(match &self.file {
            None => Observation::Absent,
            Some(bytes) => Observation::Present {
                inode: self.inode,
                generation: self.generation,
                size: bytes.len() as u64,
                regular: true,
            },
        })
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, ()> {
        self.fail()?;
        match &self.file {
            Some(bytes) if bytes.len() <= max => Ok(bytes.clone()),
            _ => Err(()),
        }
    }

    fn ensure_dir(&mut self) -> Result<(), ()> {
        self.fail()?;
        self.dir = true;
        Ok(())
    }

    fn publish(&mut self, bytes: &[u8], expect: SaveExpect) -> SaveOutcome {
        if self.broken || !self.dir {
            return SaveOutcome::NotPublished(wire::ipc::save_refusal::REFUSED);
        }
        let matches = match (expect, &self.file) {
            (SaveExpect::Absent, None) => true,
            (SaveExpect::Present { inode, generation }, Some(_)) => {
                (inode, generation) == (self.inode, self.generation)
            }
            _ => false,
        };
        if !matches {
            return SaveOutcome::Conflict;
        }
        if self.file.is_none() {
            self.inode += 100;
        }
        self.generation += 1;
        self.file = Some(bytes.to_vec());
        SaveOutcome::Committed {
            inode: self.inode,
            generation: self.generation,
        }
    }
}

/// The input owner as KMS sees it (`RegisterTrustChallenge` / `CollectTrustDecision`):
/// it keeps each registered descriptor and answers the decision a test scripts for it,
/// bound to the session the confirming client began on. A final decision is handed out
/// once, as the real owner does.
pub mod owner {
    use abi::cell_session::trust::{TrustChallengeV1, TrustDecisionV1, DECISION_PENDING};
    use abi::cell_session::{ObjectRefV1 as Ref, PrincipalV1, Status};
    use sha2::{Digest, Sha256};
    use std::cell::{Cell, RefCell};
    use std::string::String;
    use std::vec::Vec;

    pub struct Registered {
        pub confirmation: Ref,
        pub challenge: Ref,
        pub target: PrincipalV1,
        pub expiry: u64,
        pub host: String,
        pub port: u16,
        pub fingerprint: String,
        pub digest: [u8; 32],
        pub decision: u32,
        pub session: Ref,
        pub epoch: u64,
    }
    std::thread_local! {
        pub static NOW: Cell<u64> = const { Cell::new(1_000_000_000) };
        pub static OWNER: RefCell<Vec<Registered>> = const { RefCell::new(Vec::new()) };
    }
    pub fn now_ns() -> Result<u64, Status> {
        Ok(NOW.with(Cell::get))
    }
    pub fn register(d: &TrustChallengeV1<'_>) -> Result<Ref, Status> {
        let mut bytes = [0u8; TrustChallengeV1::MAX_LEN];
        let len = d.encode(&mut bytes)?;
        OWNER.with(|o| {
            let mut o = o.borrow_mut();
            let confirmation = Ref {
                id: 500 + o.len() as u64,
                generation: 7,
            };
            o.push(Registered {
                confirmation,
                challenge: d.challenge,
                target: d.target,
                expiry: d.expiry_ns,
                host: d.prompt.host.into(),
                port: d.prompt.port,
                fingerprint: d.prompt.fingerprint.into(),
                digest: Sha256::digest(&bytes[..len]).into(),
                decision: DECISION_PENDING,
                session: Ref::ZERO,
                epoch: 0,
            });
            Ok(confirmation)
        })
    }
    pub fn collect(confirmation: Ref) -> Result<TrustDecisionV1, Status> {
        OWNER.with(|o| {
            let mut o = o.borrow_mut();
            let i = o
                .iter()
                .position(|r| r.confirmation == confirmation)
                .ok_or(Status::Stale)?;
            let r = &o[i];
            let d = TrustDecisionV1 {
                decision: r.decision,
                challenge: r.challenge,
                target: r.target,
                session: r.session,
                epoch: r.epoch,
                descriptor_sha256: r.digest,
            };
            if d.is_final() {
                o.remove(i);
            }
            Ok(d)
        })
    }
    /// The client began `confirmation` on `session` at `epoch`; the user answered `decision`.
    pub fn answer(confirmation: Ref, session: Ref, epoch: u64, decision: u32) {
        edit(confirmation, |r| {
            r.session = session;
            r.epoch = epoch;
            r.decision = decision;
        });
    }
    pub fn edit(confirmation: Ref, f: impl FnOnce(&mut Registered)) {
        OWNER.with(|o| {
            f(o.borrow_mut()
                .iter_mut()
                .find(|r| r.confirmation == confirmation)
                .expect("registered"))
        });
    }
    pub fn registered() -> usize {
        OWNER.with(|o| o.borrow().len())
    }
}

use abi::cell_session::trust::{
    DECISION_ACCEPT, DECISION_CANCELLED, DECISION_EXPIRED, DECISION_REJECT,
};
use abi::cell_session::ObjectRefV1 as Ref;

const fn identity(cell_id: u64, tid: u64, flags: u32) -> CallerIdentity {
    CallerIdentity {
        flags,
        cell_id,
        generation: 1,
        sender_tid: tid,
    }
}

const CLIENT: CallerIdentity = identity(91, 61, CALLER_FLAG_NET_CONNECT);
const PLAIN: CallerIdentity = identity(92, 62, CALLER_FLAG_VFS_MUTATE);
const OTHER_CLIENT: CallerIdentity = identity(93, 63, CALLER_FLAG_NET_CONNECT);
const NO_SERVICES: ServiceRegistrySnapshot = ServiceRegistrySnapshot {
    net_broker_tid: None,
    supervisor_tid: None,
    net_tid: None,
};

const SESSION: Ref = Ref {
    id: 41,
    generation: 3,
};
const EPOCH: u64 = 4;

fn send(
    kms: &mut KmsService,
    caller: CallerIdentity,
    opcode: KmsOpcode,
    payload: &[u8],
) -> Result<Vec<u8>, KmsErrorCode> {
    let frame = KmsRequestV1::new(opcode, 41, payload).unwrap().to_bytes();
    let response = kms
        .handle(
            &frame,
            caller.sender_tid as usize,
            Some(caller),
            NO_SERVICES,
        )
        .expect("a typed reply");
    assert_eq!(response.opcode().unwrap(), opcode);
    match response.error_code().unwrap() {
        Some(code) => {
            assert!(response.payload().unwrap().is_empty());
            Err(code)
        }
        None => Ok(response.payload().unwrap().to_vec()),
    }
}

fn begin(
    kms: &mut KmsService,
    caller: CallerIdentity,
    kind: KmsTrustQueryKind,
    host: &str,
    port: u16,
    key: &[u8],
) -> Result<(SshTransferBeginResponsePayload, Vec<u8>), KmsErrorCode> {
    let algorithm = "ssh-ed25519";
    let mut staged = Vec::new();
    staged.extend_from_slice(host.as_bytes());
    staged.extend_from_slice(algorithm.as_bytes());
    staged.extend_from_slice(key);
    let (session, epoch) = match kind {
        KmsTrustQueryKind::Lookup => ((0, 0), 0),
        KmsTrustQueryKind::Challenge => ((SESSION.id, SESSION.generation), EPOCH),
    };
    let request = TrustQueryBeginRequestPayload {
        kind,
        algorithm_len: algorithm.len() as u8,
        port,
        host_len: host.len() as u16,
        key_len: key.len() as u16,
        epoch,
        session,
        query_sha256: Sha256::digest(&staged).into(),
    };
    let reply = send(kms, caller, KmsOpcode::BeginTrustQuery, &request.encode())?;
    let begin = SshTransferBeginResponsePayload::decode(&reply).expect("begin reply");
    assert_eq!(begin.total_len as usize, staged.len());
    Ok((begin, staged))
}

fn chunk(
    kms: &mut KmsService,
    caller: CallerIdentity,
    handle: u64,
    index: u32,
    data: &[u8],
) -> Result<SshChunkAckPayload, KmsErrorCode> {
    let mut chunk = SshChunkRequestPayload {
        handle,
        chunk_index: index,
        chunk_len: data.len() as u16,
        reserved: 0,
        chunk: [0; KMS_SSH_CHUNK_LEN],
    };
    chunk.chunk[..data.len()].copy_from_slice(data);
    let reply = send(kms, caller, KmsOpcode::TrustQueryChunk, &chunk.encode())?;
    Ok(SshChunkAckPayload::decode(&reply).expect("ack"))
}

fn commit_query(
    kms: &mut KmsService,
    caller: CallerIdentity,
    begin: &SshTransferBeginResponsePayload,
    sha256: [u8; 32],
) -> Result<TrustOutcomePayload, KmsErrorCode> {
    let request = SshImportCommitRequestPayload {
        handle: begin.handle,
        chunk_count: begin.chunk_count,
        reserved: 0,
        blob_sha256: sha256,
    };
    let reply = send(kms, caller, KmsOpcode::CommitTrustQuery, &request.encode())?;
    Ok(TrustOutcomePayload::decode(&reply).expect("outcome"))
}

/// Begin, every canonical chunk, commit.
fn query(
    kms: &mut KmsService,
    caller: CallerIdentity,
    kind: KmsTrustQueryKind,
    host: &str,
    port: u16,
    key: &[u8],
) -> Result<TrustOutcomePayload, KmsErrorCode> {
    let (begin, staged) = begin(kms, caller, kind, host, port, key)?;
    for (index, data) in staged.chunks(KMS_SSH_CHUNK_LEN).enumerate() {
        let ack = chunk(kms, caller, begin.handle, index as u32, data)?;
        assert_eq!(ack.next_chunk_index, index as u32 + 1);
    }
    commit_query(kms, caller, &begin, Sha256::digest(&staged).into())
}

/// `TrustLookup`: one `ssh_trust` code, or the service error.
fn ask(
    kms: &mut KmsService,
    caller: CallerIdentity,
    host: &str,
    port: u16,
    key: &[u8],
) -> Result<u8, KmsErrorCode> {
    let outcome = query(kms, caller, KmsTrustQueryKind::Lookup, host, port, key)?;
    assert_eq!(outcome.challenged, 0);
    Ok(outcome.outcome)
}

/// A first-trust challenge for the ssh-ed25519 key `blob(key)`: `(confirmation,
/// challenge)`, or the refusal code, or the service error.
fn challenge(
    kms: &mut KmsService,
    caller: CallerIdentity,
    host: &str,
    port: u16,
    key: u8,
) -> Result<(Ref, Ref), Result<u8, KmsErrorCode>> {
    let outcome = query(
        kms,
        caller,
        KmsTrustQueryKind::Challenge,
        host,
        port,
        &blob(key),
    )
    .map_err(Err)?;
    if outcome.challenged == 1 {
        let r = |(id, generation)| Ref { id, generation };
        Ok((r(outcome.confirmation), r(outcome.challenge)))
    } else {
        Err(Ok(outcome.outcome))
    }
}

fn commit(
    kms: &mut KmsService,
    caller: CallerIdentity,
    challenge: Ref,
) -> Result<u8, KmsErrorCode> {
    let request = TrustChallengeCommitRequestPayload {
        challenge: (challenge.id, challenge.generation),
    };
    let reply = send(
        kms,
        caller,
        KmsOpcode::CommitTrustChallenge,
        &request.encode(),
    )?;
    let outcome = TrustOutcomePayload::decode(&reply).expect("outcome");
    assert_eq!(outcome.challenged, 0);
    Ok(outcome.outcome)
}

fn blob(key: u8) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&11u32.to_be_bytes());
    b.extend_from_slice(b"ssh-ed25519");
    b.extend_from_slice(&32u32.to_be_bytes());
    b.extend_from_slice(&[key; 32]);
    b
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            s.push(if i <= chunk.len() {
                T[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    s
}

fn store(lines: &[String]) -> String {
    let mut s = String::from("cellos-ssh-trust v1\n");
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    s
}

fn line(host: &str, port: u16, key: u8) -> String {
    format!(
        "{host} {port} ssh-ed25519 {} preprovisioned",
        b64(&blob(key))
    )
}

fn interactive(host: &str, port: u16, key: u8) -> String {
    format!("{host} {port} ssh-ed25519 {} interactive", b64(&blob(key)))
}

fn stored(kms: &KmsService) -> Option<String> {
    kms.trust
        .io
        .file
        .as_ref()
        .map(|bytes| String::from_utf8(bytes.clone()).unwrap())
}

#[test]
fn lookup_answers_exact_unknown_mismatch_and_denies_without_connect_right() {
    let mut kms = KmsService::new();
    kms.trust
        .io
        .seed(&store(&[line("10.0.2.2", 2222, 7), line("host", 2222, 8)]));
    let exact = Ok(ssh_trust::KNOWN_EXACT);
    assert_eq!(ask(&mut kms, CLIENT, "10.0.2.2", 2222, &blob(7)), exact);
    assert_eq!(ask(&mut kms, CLIENT, "host", 2222, &blob(8)), exact);
    // Same key under another port or name is not cross-authorized.
    assert_eq!(
        ask(&mut kms, CLIENT, "10.0.2.2", 22, &blob(7)),
        Ok(ssh_trust::UNKNOWN)
    );
    assert_eq!(
        ask(&mut kms, CLIENT, "host", 2222, &blob(7)),
        Ok(ssh_trust::MISMATCH)
    );
    // A truncated or extended blob never matches.
    assert_eq!(
        ask(&mut kms, CLIENT, "10.0.2.2", 2222, &blob(7)[..50]),
        Ok(ssh_trust::MISMATCH)
    );
    assert_eq!(
        ask(&mut kms, PLAIN, "10.0.2.2", 2222, &blob(7)),
        Err(KmsErrorCode::PermissionDenied)
    );
}

#[test]
fn absent_store_is_unknown_and_an_unreadable_one_is_unavailable() {
    let mut kms = KmsService::new();
    assert_eq!(
        ask(&mut kms, CLIENT, "h", 22, &blob(1)),
        Ok(ssh_trust::UNKNOWN)
    );
    kms.trust.io.seed(&store(&[line("h", 22, 1)]));
    assert_eq!(
        ask(&mut kms, CLIENT, "h", 22, &blob(1)),
        Ok(ssh_trust::KNOWN_EXACT)
    );
    kms.trust.io.broken = true;
    assert_eq!(
        ask(&mut kms, CLIENT, "h", 22, &blob(1)),
        Ok(ssh_trust::UNAVAILABLE)
    );
    assert_eq!(
        challenge(&mut kms, CLIENT, "other", 22, 1),
        Err(Ok(ssh_trust::UNAVAILABLE))
    );
}

#[test]
fn every_malformed_store_is_unavailable() {
    let good = line("h", 22, 1);
    let key = b64(&blob(1));
    let cases: Vec<String> = alloc::vec![
        String::new(),
        String::from("cellos-ssh-trust v2\n"),
        store(&[good.clone()]).trim_end().into(),
        store(&[good.clone(), good.clone()]),
        store(&[format!("H 22 ssh-ed25519 {key} preprovisioned")]),
        store(&[format!("h 022 ssh-ed25519 {key} preprovisioned")]),
        store(&[format!("h 0 ssh-ed25519 {key} preprovisioned")]),
        store(&[format!("h 65536 ssh-ed25519 {key} preprovisioned")]),
        store(&[format!("h 22 ssh-rsa {key} preprovisioned")]),
        store(&[format!("h 22 ssh-ed25519 {key} trusted")]),
        store(&[format!("h 22 ssh-ed25519 {key}")]),
        store(&[format!("h 22 ssh-ed25519 {key} preprovisioned extra")]),
        store(&[format!(
            "h 22 ssh-ed25519 {} preprovisioned",
            &key[..key.len() - 1]
        )]),
        store(&[format!(
            "h 22 ssh-ed25519 {} preprovisioned",
            b64(b"\0\0\0\x07ssh-rsa")
        )]),
        store(&[format!("*.example 22 ssh-ed25519 {key} preprovisioned")]),
        store(&[good.clone(), String::new()]),
    ];
    for (i, text) in cases.iter().enumerate() {
        assert_eq!(
            lookup(text.as_bytes(), "h", 22, "ssh-ed25519", &blob(1)),
            ssh_trust::UNAVAILABLE,
            "case {i}: {text:?}"
        );
    }
    assert_eq!(
        lookup(store(&[good]).as_bytes(), "h", 22, "ssh-ed25519", &blob(1)),
        ssh_trust::KNOWN_EXACT
    );
    let many: Vec<String> = (0..1025u32)
        .map(|i| line(&format!("h{i}"), 22, 1))
        .collect();
    assert_eq!(
        lookup(store(&many).as_bytes(), "h", 22, "ssh-ed25519", &blob(1)),
        ssh_trust::UNAVAILABLE
    );
    // Through the service too: a malformed store is never a partial answer.
    let mut kms = KmsService::new();
    kms.trust.io.seed(&cases[3]);
    assert_eq!(
        ask(&mut kms, CLIENT, "h", 22, &blob(1)),
        Ok(ssh_trust::UNAVAILABLE)
    );
}

#[test]
fn an_accepted_first_trust_is_stored_exactly_and_then_known() {
    let mut kms = KmsService::new();
    let (confirmation, id) = challenge(&mut kms, CLIENT, "gateway", 2222, 5).unwrap();
    // The owner got the exact descriptor: the caller as target, host, port and the
    // fingerprint `vissh` shows, with a deadline 60 s ahead.
    owner::edit(confirmation, |r| {
        assert_eq!(
            (r.challenge, r.target.cell_id, r.target.generation),
            (id, 91, 1)
        );
        assert_eq!(
            (r.host.as_str(), r.port, r.expiry),
            ("gateway", 2222, 61_000_000_000)
        );
        let digest: [u8; 32] = Sha256::digest(blob(5)).into();
        let expected = format!("SHA256:{}", b64(&digest).trim_end_matches('='));
        assert_eq!(r.fingerprint, expected);
        assert!(!r.fingerprint.ends_with('='));
    });
    // Undecided: nothing is written and the challenge stays.
    assert_eq!(commit(&mut kms, CLIENT, id), Ok(ssh_trust::PENDING));
    assert_eq!(stored(&kms), None);
    owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
    assert_eq!(commit(&mut kms, CLIENT, id), Ok(ssh_trust::KNOWN_EXACT));
    assert_eq!(
        stored(&kms),
        Some(store(&[interactive("gateway", 2222, 5)]))
    );
    assert!(kms.trust.io.dir);
    assert_eq!(
        ask(&mut kms, CLIENT, "gateway", 2222, &blob(5)),
        Ok(ssh_trust::KNOWN_EXACT)
    );
    // The challenge is over: committing it again is refused.
    assert_eq!(commit(&mut kms, CLIENT, id), Ok(ssh_trust::DENIED));
    // A second first trust appends after the existing lines.
    let (confirmation, id) = challenge(&mut kms, CLIENT, "10.0.2.2", 22, 6).unwrap();
    owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
    assert_eq!(commit(&mut kms, CLIENT, id), Ok(ssh_trust::KNOWN_EXACT));
    assert_eq!(
        stored(&kms),
        Some(store(&[
            interactive("gateway", 2222, 5),
            interactive("10.0.2.2", 22, 6)
        ]))
    );
}

#[test]
fn refused_expired_or_cancelled_confirmations_store_nothing() {
    for (decision, code) in [
        (DECISION_REJECT, ssh_trust::REFUSED),
        (DECISION_CANCELLED, ssh_trust::REFUSED),
        (DECISION_EXPIRED, ssh_trust::EXPIRED),
    ] {
        let mut kms = KmsService::new();
        let (confirmation, id) = challenge(&mut kms, CLIENT, "gateway", 22, 5).unwrap();
        owner::answer(confirmation, SESSION, EPOCH, decision);
        assert_eq!(
            commit(&mut kms, CLIENT, id),
            Ok(code),
            "decision {decision}"
        );
        assert_eq!(stored(&kms), None);
        assert_eq!(commit(&mut kms, CLIENT, id), Ok(ssh_trust::DENIED));
        // The caller may challenge again.
        assert!(challenge(&mut kms, CLIENT, "gateway", 22, 5).is_ok());
    }
    // An acceptance KMS sees only after its own deadline is expired too.
    let mut kms = KmsService::new();
    let (confirmation, id) = challenge(&mut kms, CLIENT, "gateway", 22, 5).unwrap();
    owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
    owner::NOW.with(|n| n.set(n.get() + 60_000_000_000));
    assert_eq!(commit(&mut kms, CLIENT, id), Ok(ssh_trust::EXPIRED));
    assert_eq!(stored(&kms), None);
}

#[test]
fn a_known_or_changed_key_is_never_challenged() {
    let mut kms = KmsService::new();
    kms.trust.io.seed(&store(&[line("gateway", 22, 1)]));
    assert_eq!(
        challenge(&mut kms, CLIENT, "gateway", 22, 2),
        Err(Ok(ssh_trust::MISMATCH))
    );
    assert_eq!(
        challenge(&mut kms, CLIENT, "gateway", 22, 1),
        Err(Ok(ssh_trust::KNOWN_EXACT))
    );
    assert_eq!(
        challenge(&mut kms, PLAIN, "other", 22, 1),
        Err(Err(KmsErrorCode::PermissionDenied))
    );
    assert_eq!(owner::registered(), 0);
    assert_eq!(stored(&kms), Some(store(&[line("gateway", 22, 1)])));
}

#[test]
fn only_what_a_store_line_holds_is_ever_challenged() {
    let mut kms = KmsService::new();
    for host in ["Gateway", "gateway.", "*.example", "a b"] {
        assert_eq!(
            challenge(&mut kms, CLIENT, host, 22, 1),
            Err(Err(KmsErrorCode::InvalidRequest)),
            "{host}"
        );
    }
    // A key blob that does not name its algorithm.
    let mut key = blob(1);
    key[4] = b'x';
    assert_eq!(
        query(
            &mut kms,
            CLIENT,
            KmsTrustQueryKind::Challenge,
            "gateway",
            22,
            &key
        ),
        Err(KmsErrorCode::InvalidRequest)
    );
    assert_eq!(owner::registered(), 0);
}

#[test]
fn concurrent_first_trusts_never_overwrite_each_other() {
    for (second_key, second_answer) in [(5, ssh_trust::KNOWN_EXACT), (6, ssh_trust::MISMATCH)] {
        let mut kms = KmsService::new();
        let (first_confirmation, first) = challenge(&mut kms, CLIENT, "gateway", 22, 5).unwrap();
        let (second_confirmation, second) =
            challenge(&mut kms, OTHER_CLIENT, "gateway", 22, second_key).unwrap();
        owner::answer(first_confirmation, SESSION, EPOCH, DECISION_ACCEPT);
        owner::answer(second_confirmation, SESSION, EPOCH, DECISION_ACCEPT);
        assert_eq!(commit(&mut kms, CLIENT, first), Ok(ssh_trust::KNOWN_EXACT));
        // The second sees the first's line: the same key is known, a different one is refused.
        assert_eq!(commit(&mut kms, OTHER_CLIENT, second), Ok(second_answer));
        assert_eq!(stored(&kms), Some(store(&[interactive("gateway", 22, 5)])));
    }
}

#[test]
fn only_the_caller_with_matching_owner_bindings_commits() {
    let mut kms = KmsService::new();
    let (confirmation, id) = challenge(&mut kms, CLIENT, "gateway", 22, 5).unwrap();
    owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
    // Another caller, a caller without the connect right, a forged ref: refused, record kept.
    assert_eq!(commit(&mut kms, OTHER_CLIENT, id), Ok(ssh_trust::DENIED));
    assert_eq!(
        commit(&mut kms, PLAIN, id),
        Err(KmsErrorCode::PermissionDenied)
    );
    // The same cell under a new generation is another principal.
    let respawned = CallerIdentity {
        generation: 2,
        ..CLIENT
    };
    assert_eq!(commit(&mut kms, respawned, id), Ok(ssh_trust::DENIED));
    assert_eq!(
        commit(
            &mut kms,
            CLIENT,
            Ref {
                id: id.id + 1,
                generation: id.generation
            }
        ),
        Ok(ssh_trust::DENIED)
    );
    // A decision whose descriptor digest, session or epoch differs is refused.
    let tampered: [fn(&mut owner::Registered); 3] = [
        |r| r.digest[0] ^= 1,
        |r| r.session.id += 1,
        |r| r.epoch += 1,
    ];
    owner::edit(confirmation, tampered[0]);
    assert_eq!(commit(&mut kms, CLIENT, id), Ok(ssh_trust::REFUSED));
    for tamper in &tampered[1..] {
        let (confirmation, id) = challenge(&mut kms, CLIENT, "gateway", 22, 5).unwrap();
        owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
        owner::edit(confirmation, tamper);
        assert_eq!(commit(&mut kms, CLIENT, id), Ok(ssh_trust::REFUSED));
    }
    assert_eq!(stored(&kms), None);
}

#[test]
fn challenges_are_bounded_per_caller_and_in_total() {
    let mut kms = KmsService::new();
    assert!(challenge(&mut kms, CLIENT, "a", 22, 1).is_ok());
    let busy = Err(Err(KmsErrorCode::Busy));
    assert_eq!(challenge(&mut kms, CLIENT, "b", 22, 1), busy);
    for cell in [94, 95, 96] {
        let caller = identity(cell, cell + 100, CALLER_FLAG_NET_CONNECT);
        assert!(challenge(&mut kms, caller, "c", 22, 1).is_ok());
    }
    assert_eq!(challenge(&mut kms, OTHER_CLIENT, "d", 22, 1), busy);
    // After its deadline a caller's own challenge gives way to a new one.
    owner::NOW.with(|n| n.set(n.get() + 60_000_000_000));
    assert!(challenge(&mut kms, CLIENT, "b", 22, 1).is_ok());
}

#[test]
fn queries_are_strictly_ordered_and_verified() {
    let mut kms = KmsService::new();
    kms.trust.io.seed(&store(&[line("gateway", 22, 1)]));
    // A 4 KiB key: many canonical chunks.
    let mut key = blob(1);
    key.resize(4096, 0x33);
    let lookup = KmsTrustQueryKind::Lookup;
    assert_eq!(
        ask(&mut kms, CLIENT, "gateway", 22, &key),
        Ok(ssh_trust::MISMATCH)
    );

    // Out of order, then a short non-final chunk: each destroys the query.
    let (b, staged) = begin(&mut kms, CLIENT, lookup, "gateway", 22, &key).unwrap();
    assert_eq!(
        b.chunk_count as usize,
        staged.len().div_ceil(KMS_SSH_CHUNK_LEN)
    );
    assert_eq!(
        chunk(&mut kms, CLIENT, b.handle, 1, &staged[96..192]),
        Err(KmsErrorCode::TrustOrderInvalid)
    );
    assert_eq!(
        chunk(&mut kms, CLIENT, b.handle, 0, &staged[..96]),
        Err(KmsErrorCode::TrustHandleInvalid)
    );
    let (b, staged) = begin(&mut kms, CLIENT, lookup, "gateway", 22, &key).unwrap();
    assert_eq!(
        chunk(&mut kms, CLIENT, b.handle, 0, &staged[..95]),
        Err(KmsErrorCode::TrustOrderInvalid)
    );

    // Another caller cannot touch, commit or abort a query; the owner still can.
    let (b, staged) = begin(&mut kms, CLIENT, lookup, "gateway", 22, &key).unwrap();
    assert_eq!(
        chunk(&mut kms, OTHER_CLIENT, b.handle, 0, &staged[..96]),
        Err(KmsErrorCode::TrustHandleInvalid)
    );
    let digest: [u8; 32] = Sha256::digest(&staged).into();
    assert_eq!(
        commit_query(&mut kms, OTHER_CLIENT, &b, digest),
        Err(KmsErrorCode::TrustHandleInvalid)
    );
    let abort = SshAbortRequestPayload { handle: b.handle }.encode();
    assert_eq!(
        send(&mut kms, OTHER_CLIENT, KmsOpcode::AbortTrustQuery, &abort),
        Err(KmsErrorCode::TrustHandleInvalid)
    );
    assert!(chunk(&mut kms, CLIENT, b.handle, 0, &staged[..96]).is_ok());
    // An early commit consumes the query.
    assert_eq!(
        commit_query(&mut kms, CLIENT, &b, digest),
        Err(KmsErrorCode::TrustOrderInvalid)
    );
    assert_eq!(
        send(&mut kms, CLIENT, KmsOpcode::AbortTrustQuery, &abort),
        Err(KmsErrorCode::TrustHandleInvalid)
    );

    // Every chunk, but a digest that does not match the staged bytes.
    let (b, staged) = begin(&mut kms, CLIENT, lookup, "gateway", 22, &key).unwrap();
    for (index, data) in staged.chunks(KMS_SSH_CHUNK_LEN).enumerate() {
        chunk(&mut kms, CLIENT, b.handle, index as u32, data).unwrap();
    }
    assert_eq!(
        commit_query(&mut kms, CLIENT, &b, [9; 32]),
        Err(KmsErrorCode::InvalidRequest)
    );

    // Abort by the owner ends the query.
    let (b, _) = begin(&mut kms, CLIENT, lookup, "gateway", 22, &key).unwrap();
    let abort = SshAbortRequestPayload { handle: b.handle }.encode();
    assert_eq!(
        send(&mut kms, CLIENT, KmsOpcode::AbortTrustQuery, &abort),
        Ok(Vec::new())
    );
    assert_eq!(kms.trust.queries_open(), 0);
    // A malformed Begin is refused before anything is staged.
    assert_eq!(
        send(&mut kms, CLIENT, KmsOpcode::BeginTrustQuery, &[0; 64]),
        Err(KmsErrorCode::InvalidRequest)
    );
}

#[test]
fn queries_are_bounded_and_a_new_begin_replaces_the_callers_own() {
    let mut kms = KmsService::new();
    let lookup = KmsTrustQueryKind::Lookup;
    let (first, _) = begin(&mut kms, CLIENT, lookup, "h", 22, &blob(1)).unwrap();
    let (second, staged) = begin(&mut kms, CLIENT, lookup, "h", 22, &blob(1)).unwrap();
    assert_ne!(first.handle, second.handle);
    assert_eq!(kms.trust.queries_open(), 1);
    assert_eq!(
        chunk(&mut kms, CLIENT, first.handle, 0, &staged),
        Err(KmsErrorCode::TrustHandleInvalid)
    );
    for cell in [94, 95, 96] {
        let caller = identity(cell, cell + 100, CALLER_FLAG_NET_CONNECT);
        assert!(begin(&mut kms, caller, lookup, "h", 22, &blob(1)).is_ok());
    }
    assert_eq!(
        begin(&mut kms, OTHER_CLIENT, lookup, "h", 22, &blob(1)).map(|_| ()),
        Err(KmsErrorCode::Busy)
    );
    // Abandoned queries give way after their lifetime.
    owner::NOW.with(|n| n.set(n.get() + 10_000_000_000));
    assert!(begin(&mut kms, OTHER_CLIENT, lookup, "h", 22, &blob(1)).is_ok());
    assert_eq!(kms.trust.queries_open(), 1);
}

#[test]
fn trust_opcodes_need_an_attested_caller_and_serve_every_build() {
    let mut kms = KmsService::new();
    for opcode in [
        KmsOpcode::BeginTrustQuery,
        KmsOpcode::TrustQueryChunk,
        KmsOpcode::CommitTrustQuery,
        KmsOpcode::CommitTrustChallenge,
        KmsOpcode::AbortTrustQuery,
    ] {
        let frame = KmsRequestV1::new(opcode, 41, &[]).unwrap().to_bytes();
        for caller in [
            None,
            Some(CallerIdentity {
                generation: 0,
                ..CLIENT
            }),
            Some(CallerIdentity {
                cell_id: 0,
                ..CLIENT
            }),
        ] {
            let response = kms.handle(&frame, 61, caller, NO_SERVICES).unwrap();
            assert_eq!(
                response.error_code().unwrap(),
                Some(KmsErrorCode::CallerUnattested)
            );
        }
        // A sender that is not the attested task.
        let response = kms.handle(&frame, 7, Some(CLIENT), NO_SERVICES).unwrap();
        assert_eq!(
            response.error_code().unwrap(),
            Some(KmsErrorCode::CallerUnattested)
        );
        assert_eq!(
            send(&mut kms, PLAIN, opcode, &[]),
            Err(KmsErrorCode::PermissionDenied)
        );
        // Admitted, then an empty payload is malformed.
        assert_eq!(
            send(&mut kms, CLIENT, opcode, &[]),
            Err(KmsErrorCode::InvalidRequest)
        );
    }
}
