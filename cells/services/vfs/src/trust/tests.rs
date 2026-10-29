//! `TrustLookup` end to end through `dispatch::handle_request` over a CellosFS `/srv`,
//! plus the store parser's refusal of every malformed form.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use std::sync::{Arc, Mutex as HostMutex};

use api::caller_identity::{CALLER_FLAG_NET_CONNECT, CALLER_FLAG_VFS_MUTATE};
use api::ipc::{ssh_trust, VfsRequest, VfsResponse, IPC_BUF_SIZE};
use types::CellId;

use super::{lookup, STORE_PATH};
use crate::caller::Caller;
use crate::manager::VfsManager;

const CLIENT: Caller = Caller { flags: CALLER_FLAG_NET_CONNECT, ..Caller::principal(CellId(91), 1) };
const PLAIN: Caller = Caller { flags: CALLER_FLAG_VFS_MUTATE, ..Caller::principal(CellId(92), 1) };
const OTHER_CLIENT: Caller = Caller { flags: CALLER_FLAG_NET_CONNECT, ..Caller::principal(CellId(93), 1) };

#[derive(Clone)]
struct Disk(Arc<HostMutex<Vec<[u8; cellos_fs::format::BLOCK_SIZE]>>>);

impl cellos_fs::disk::BlockDevice for Disk {
    fn total_blocks(&self) -> u64 { self.0.lock().unwrap().len() as u64 }
    fn read_block(&mut self, block: u64, out: &mut [u8; cellos_fs::format::BLOCK_SIZE]) -> Result<(), cellos_fs::disk::FsError> {
        out.copy_from_slice(&self.0.lock().unwrap()[block as usize]);
        Ok(())
    }
    fn write_block(&mut self, block: u64, data: &[u8; cellos_fs::format::BLOCK_SIZE]) -> Result<(), cellos_fs::disk::FsError> {
        self.0.lock().unwrap()[block as usize] = *data;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), cellos_fs::disk::FsError> { Ok(()) }
}

fn manager() -> VfsManager {
    let mut vfs = VfsManager::new();
    let disk = Disk(Arc::new(HostMutex::new(alloc::vec![[0; cellos_fs::format::BLOCK_SIZE]; 512])));
    let srv = crate::backend_cellosfs::CellosFsBackend::formatted_for_test("/srv", disk, 512);
    let index = vfs.mounts.add_backend(Box::new(srv));
    vfs.mounts.mount("/srv", index);
    for caller in [CLIENT, PLAIN, OTHER_CLIENT] {
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
    }
    vfs
}

/// Seed the store the way the provisioner does: straight into the backend.
fn seed(vfs: &mut VfsManager, text: &str) {
    assert!(vfs.mkdir("/srv/.cellos-trust"));
    assert!(vfs.write(STORE_PATH, text.as_bytes()));
}

fn call(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>) -> String {
    let mut request = [0u8; IPC_BUF_SIZE];
    api::ipc::encode(req, &mut request).expect("request fits");
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let resp = crate::dispatch::handle_request(vfs, &request, Some(caller), &mut resp_buf);
    format!("{resp:?}")
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
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            s.push(if i <= chunk.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
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
    format!("{host} {port} ssh-ed25519 {} preprovisioned", b64(&blob(key)))
}

fn ask(vfs: &mut VfsManager, caller: Caller, host: &str, port: u16, key: &[u8]) -> String {
    call(vfs, caller, &VfsRequest::TrustLookup { host, port, algorithm: "ssh-ed25519", key })
}

#[test]
fn lookup_answers_exact_unknown_mismatch_and_denies_without_connect_right() {
    let mut vfs = manager();
    seed(&mut vfs, &store(&[line("10.0.2.2", 2222, 7), line("host", 2222, 8)]));
    let exact = format!("{:?}", VfsResponse::Trust(ssh_trust::KNOWN_EXACT));
    let unknown = format!("{:?}", VfsResponse::Trust(ssh_trust::UNKNOWN));
    let mismatch = format!("{:?}", VfsResponse::Trust(ssh_trust::MISMATCH));
    assert_eq!(ask(&mut vfs, CLIENT, "10.0.2.2", 2222, &blob(7)), exact);
    assert_eq!(ask(&mut vfs, CLIENT, "host", 2222, &blob(8)), exact);
    // Same key under another port or name is not cross-authorized.
    assert_eq!(ask(&mut vfs, CLIENT, "10.0.2.2", 22, &blob(7)), unknown);
    assert_eq!(ask(&mut vfs, CLIENT, "host", 2222, &blob(7)), mismatch);
    // A truncated or extended blob never matches.
    assert_eq!(ask(&mut vfs, CLIENT, "10.0.2.2", 2222, &blob(7)[..50]), mismatch);
    assert_eq!(ask(&mut vfs, PLAIN, "10.0.2.2", 2222, &blob(7)), format!("{:?}", VfsResponse::Trust(ssh_trust::DENIED)));
}

#[test]
fn absent_store_is_unknown_and_the_store_is_unreadable_by_callers() {
    let mut vfs = manager();
    assert_eq!(ask(&mut vfs, CLIENT, "h", 22, &blob(1)), format!("{:?}", VfsResponse::Trust(ssh_trust::UNKNOWN)));
    seed(&mut vfs, &store(&[line("h", 22, 1)]));
    for caller in [CLIENT, PLAIN] {
        let read = call(&mut vfs, caller, &VfsRequest::GetFile(STORE_PATH));
        assert!(read.starts_with("Err("), "{read}");
        let list = call(&mut vfs, caller, &VfsRequest::ListDir("/srv/.cellos-trust"));
        assert!(list.starts_with("Err("), "{list}");
        let unlink = call(&mut vfs, caller, &VfsRequest::Unlink(STORE_PATH));
        assert!(unlink.starts_with("Err("), "{unlink}");
    }
    assert_eq!(ask(&mut vfs, CLIENT, "h", 22, &blob(1)), format!("{:?}", VfsResponse::Trust(ssh_trust::KNOWN_EXACT)));
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
        store(&[format!("h 22 ssh-ed25519 {} preprovisioned", &key[..key.len() - 1])]),
        store(&[format!("h 22 ssh-ed25519 {} preprovisioned", b64(b"\0\0\0\x07ssh-rsa"))]),
        store(&[format!("*.example 22 ssh-ed25519 {key} preprovisioned")]),
        store(&[good.clone(), String::new()]),
    ];
    for (i, text) in cases.iter().enumerate() {
        assert_eq!(lookup(text.as_bytes(), "h", 22, "ssh-ed25519", &blob(1)), ssh_trust::UNAVAILABLE, "case {i}: {text:?}");
    }
    assert_eq!(lookup(store(&[good]).as_bytes(), "h", 22, "ssh-ed25519", &blob(1)), ssh_trust::KNOWN_EXACT);
    let many: Vec<String> = (0..1025u32).map(|i| line(&format!("h{i}"), 22, 1)).collect();
    assert_eq!(lookup(store(&many).as_bytes(), "h", 22, "ssh-ed25519", &blob(1)), ssh_trust::UNAVAILABLE);
}

/// The input owner as VFS sees it (`RegisterTrustChallenge` / `CollectTrustDecision`):
/// it keeps each registered descriptor and answers the decision a test scripts for it,
/// bound to the session the confirming client began on. A final decision is handed out
/// once, as the real owner does.
pub mod owner {
    use alloc::string::String;
    use api::cell_session::trust::{TrustChallengeV1, TrustDecisionV1, DECISION_PENDING};
    use api::cell_session::{ObjectRefV1 as Ref, PrincipalV1, Status};
    use sha2::{Digest, Sha256};
    use std::cell::{Cell, RefCell};
    use std::vec::Vec;

    pub struct Registered {
        pub confirmation: Ref, pub challenge: Ref, pub target: PrincipalV1, pub expiry: u64,
        pub host: String, pub port: u16, pub fingerprint: String, pub digest: [u8; 32],
        pub decision: u32, pub session: Ref, pub epoch: u64,
    }
    thread_local! {
        pub static NOW: Cell<u64> = const { Cell::new(1_000_000_000) };
        pub static OWNER: RefCell<Vec<Registered>> = const { RefCell::new(Vec::new()) };
    }
    pub fn now_ns() -> Result<u64, Status> { Ok(NOW.with(Cell::get)) }
    pub fn register(d: &TrustChallengeV1<'_>) -> Result<Ref, Status> {
        let mut bytes = [0u8; TrustChallengeV1::MAX_LEN];
        let len = d.encode(&mut bytes)?;
        OWNER.with(|o| {
            let mut o = o.borrow_mut();
            let confirmation = Ref { id: 500 + o.len() as u64, generation: 7 };
            o.push(Registered {
                confirmation, challenge: d.challenge, target: d.target, expiry: d.expiry_ns,
                host: d.prompt.host.into(), port: d.prompt.port, fingerprint: d.prompt.fingerprint.into(),
                digest: Sha256::digest(&bytes[..len]).into(), decision: DECISION_PENDING, session: Ref::ZERO, epoch: 0,
            });
            Ok(confirmation)
        })
    }
    pub fn collect(confirmation: Ref) -> Result<TrustDecisionV1, Status> {
        OWNER.with(|o| {
            let mut o = o.borrow_mut();
            let i = o.iter().position(|r| r.confirmation == confirmation).ok_or(Status::Stale)?;
            let r = &o[i];
            let d = TrustDecisionV1 { decision: r.decision, challenge: r.challenge, target: r.target, session: r.session, epoch: r.epoch, descriptor_sha256: r.digest };
            if d.is_final() { o.remove(i); }
            Ok(d)
        })
    }
    /// The client began `confirmation` on `session` at `epoch`; the user answered `decision`.
    pub fn answer(confirmation: Ref, session: Ref, epoch: u64, decision: u32) {
        edit(confirmation, |r| { r.session = session; r.epoch = epoch; r.decision = decision; });
    }
    pub fn edit(confirmation: Ref, f: impl FnOnce(&mut Registered)) {
        OWNER.with(|o| f(o.borrow_mut().iter_mut().find(|r| r.confirmation == confirmation).expect("registered")));
    }
    pub fn registered() -> usize { OWNER.with(|o| o.borrow().len()) }
}

use api::cell_session::trust::{DECISION_ACCEPT, DECISION_CANCELLED, DECISION_EXPIRED, DECISION_REJECT};
use api::cell_session::ObjectRefV1 as Ref;

const SESSION: Ref = Ref { id: 41, generation: 3 };
const EPOCH: u64 = 4;

fn trust(code: u8) -> String { format!("{:?}", VfsResponse::Trust(code)) }

/// `TrustChallenge` for the ssh-ed25519 key `blob(key)`: `(confirmation, challenge)` or the reply.
fn challenge(vfs: &mut VfsManager, caller: Caller, host: &str, port: u16, key: u8) -> Result<(Ref, Ref), String> {
    let mut request = [0u8; IPC_BUF_SIZE];
    let key = blob(key);
    api::ipc::encode(&VfsRequest::TrustChallenge { host, port, algorithm: "ssh-ed25519", key: &key, session: SESSION, epoch: EPOCH }, &mut request).expect("request fits");
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match crate::dispatch::handle_request(vfs, &request, Some(caller), &mut resp_buf) {
        VfsResponse::TrustChallenged { confirmation, challenge } => Ok((confirmation, challenge)),
        other => Err(format!("{other:?}")),
    }
}

fn commit(vfs: &mut VfsManager, caller: Caller, challenge: Ref) -> String {
    call(vfs, caller, &VfsRequest::TrustCommit { challenge })
}

fn stored(vfs: &VfsManager) -> Option<String> {
    vfs.stat(STORE_PATH).map(|_| String::from_utf8(vfs.read_to_vec(STORE_PATH)).unwrap())
}

fn interactive(host: &str, port: u16, key: u8) -> String {
    format!("{host} {port} ssh-ed25519 {} interactive", b64(&blob(key)))
}

#[test]
fn an_accepted_first_trust_is_stored_exactly_and_then_known() {
    let mut vfs = manager();
    let (confirmation, id) = challenge(&mut vfs, CLIENT, "gateway", 2222, 5).unwrap();
    // The owner got the exact descriptor: the caller as target, host, port and the
    // fingerprint `vissh` shows, with a deadline 60 s ahead.
    owner::edit(confirmation, |r| {
        assert_eq!((r.challenge, r.target.cell_id, r.target.generation), (id, 91, 1));
        assert_eq!((r.host.as_str(), r.port, r.expiry), ("gateway", 2222, 61_000_000_000));
        let mut expected = String::from("SHA256:");
        super::base64_encode(&<sha2::Sha256 as sha2::Digest>::digest(blob(5)), false, &mut expected);
        assert_eq!(r.fingerprint, expected);
        assert!(!r.fingerprint.ends_with('='));
    });
    // Undecided: nothing is written and the challenge stays.
    assert_eq!(commit(&mut vfs, CLIENT, id), trust(ssh_trust::PENDING));
    assert_eq!(stored(&vfs), None);
    owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
    assert_eq!(commit(&mut vfs, CLIENT, id), trust(ssh_trust::KNOWN_EXACT));
    assert_eq!(stored(&vfs), Some(store(&[interactive("gateway", 2222, 5)])));
    assert_eq!(ask(&mut vfs, CLIENT, "gateway", 2222, &blob(5)), trust(ssh_trust::KNOWN_EXACT));
    // The challenge is over: committing it again is refused.
    assert_eq!(commit(&mut vfs, CLIENT, id), trust(ssh_trust::DENIED));
    // A second first trust appends after the existing lines.
    let (confirmation, id) = challenge(&mut vfs, CLIENT, "10.0.2.2", 22, 6).unwrap();
    owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
    assert_eq!(commit(&mut vfs, CLIENT, id), trust(ssh_trust::KNOWN_EXACT));
    assert_eq!(stored(&vfs), Some(store(&[interactive("gateway", 2222, 5), interactive("10.0.2.2", 22, 6)])));
    // Still unreadable and unwritable by callers.
    for caller in [CLIENT, PLAIN] {
        assert!(call(&mut vfs, caller, &VfsRequest::GetFile(STORE_PATH)).starts_with("Err("));
        assert!(call(&mut vfs, caller, &VfsRequest::SaveBegin { dst: STORE_PATH }).starts_with("Err("));
    }
}

#[test]
fn refused_expired_or_cancelled_confirmations_store_nothing() {
    for (decision, code) in [(DECISION_REJECT, ssh_trust::REFUSED), (DECISION_CANCELLED, ssh_trust::REFUSED), (DECISION_EXPIRED, ssh_trust::EXPIRED)] {
        let mut vfs = manager();
        let (confirmation, id) = challenge(&mut vfs, CLIENT, "gateway", 22, 5).unwrap();
        owner::answer(confirmation, SESSION, EPOCH, decision);
        assert_eq!(commit(&mut vfs, CLIENT, id), trust(code), "decision {decision}");
        assert_eq!(stored(&vfs), None);
        assert_eq!(commit(&mut vfs, CLIENT, id), trust(ssh_trust::DENIED));
        // The caller may challenge again.
        assert!(challenge(&mut vfs, CLIENT, "gateway", 22, 5).is_ok());
    }
    // An acceptance VFS sees only after its own deadline is expired too.
    let mut vfs = manager();
    let (confirmation, id) = challenge(&mut vfs, CLIENT, "gateway", 22, 5).unwrap();
    owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
    owner::NOW.with(|n| n.set(n.get() + 60_000_000_000));
    assert_eq!(commit(&mut vfs, CLIENT, id), trust(ssh_trust::EXPIRED));
    assert_eq!(stored(&vfs), None);
}

#[test]
fn a_known_or_changed_key_is_never_challenged() {
    let mut vfs = manager();
    seed(&mut vfs, &store(&[line("gateway", 22, 1)]));
    assert_eq!(challenge(&mut vfs, CLIENT, "gateway", 22, 2), Err(trust(ssh_trust::MISMATCH)));
    assert_eq!(challenge(&mut vfs, CLIENT, "gateway", 22, 1), Err(trust(ssh_trust::KNOWN_EXACT)));
    assert_eq!(challenge(&mut vfs, PLAIN, "other", 22, 1), Err(trust(ssh_trust::DENIED)));
    assert_eq!(owner::registered(), 0);
    assert_eq!(stored(&vfs), Some(store(&[line("gateway", 22, 1)])));
}

#[test]
fn concurrent_first_trusts_never_overwrite_each_other() {
    for (second_key, second_answer) in [(5, ssh_trust::KNOWN_EXACT), (6, ssh_trust::MISMATCH)] {
        let mut vfs = manager();
        let (first_confirmation, first) = challenge(&mut vfs, CLIENT, "gateway", 22, 5).unwrap();
        let (second_confirmation, second) = challenge(&mut vfs, OTHER_CLIENT, "gateway", 22, second_key).unwrap();
        owner::answer(first_confirmation, SESSION, EPOCH, DECISION_ACCEPT);
        owner::answer(second_confirmation, SESSION, EPOCH, DECISION_ACCEPT);
        assert_eq!(commit(&mut vfs, CLIENT, first), trust(ssh_trust::KNOWN_EXACT));
        // The second sees the first's line: the same key is known, a different one is refused.
        assert_eq!(commit(&mut vfs, OTHER_CLIENT, second), trust(second_answer));
        assert_eq!(stored(&vfs), Some(store(&[interactive("gateway", 22, 5)])));
    }
}

#[test]
fn only_the_caller_with_matching_owner_bindings_commits() {
    let mut vfs = manager();
    let (confirmation, id) = challenge(&mut vfs, CLIENT, "gateway", 22, 5).unwrap();
    owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
    // Another caller, a caller without the connect right, a forged ref: refused, record kept.
    assert_eq!(commit(&mut vfs, OTHER_CLIENT, id), trust(ssh_trust::DENIED));
    assert_eq!(commit(&mut vfs, PLAIN, id), trust(ssh_trust::DENIED));
    assert_eq!(commit(&mut vfs, CLIENT, Ref { id: id.id + 1, generation: id.generation }), trust(ssh_trust::DENIED));
    // A decision whose descriptor digest, session or epoch differs is refused.
    let tampered: [fn(&mut owner::Registered); 3] = [|r| r.digest[0] ^= 1, |r| r.session.id += 1, |r| r.epoch += 1];
    owner::edit(confirmation, tampered[0]);
    assert_eq!(commit(&mut vfs, CLIENT, id), trust(ssh_trust::REFUSED));
    for tamper in &tampered[1..] {
        let (confirmation, id) = challenge(&mut vfs, CLIENT, "gateway", 22, 5).unwrap();
        owner::answer(confirmation, SESSION, EPOCH, DECISION_ACCEPT);
        owner::edit(confirmation, tamper);
        assert_eq!(commit(&mut vfs, CLIENT, id), trust(ssh_trust::REFUSED));
    }
    assert_eq!(stored(&vfs), None);
}

#[test]
fn challenges_are_bounded_per_caller_and_in_total() {
    let mut vfs = manager();
    assert!(challenge(&mut vfs, CLIENT, "a", 22, 1).is_ok());
    let quota = format!("{:?}", VfsResponse::Err(crate::paths::ERR_QUOTA));
    assert_eq!(challenge(&mut vfs, CLIENT, "b", 22, 1), Err(quota.clone()));
    let more: [Caller; 3] = [94, 95, 96].map(|cell| Caller { flags: CALLER_FLAG_NET_CONNECT, ..Caller::principal(CellId(cell), 1) });
    for caller in more {
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
        assert!(challenge(&mut vfs, caller, "c", 22, 1).is_ok());
    }
    assert_eq!(challenge(&mut vfs, OTHER_CLIENT, "d", 22, 1), Err(quota));
    // After its deadline a caller's own challenge gives way to a new one.
    owner::NOW.with(|n| n.set(n.get() + 60_000_000_000));
    assert!(challenge(&mut vfs, CLIENT, "b", 22, 1).is_ok());
}
