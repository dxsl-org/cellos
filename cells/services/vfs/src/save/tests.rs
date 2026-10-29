//! D03 end to end through `dispatch::handle_request` over a CellosFS `/srv`.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use std::sync::{Arc, Mutex as HostMutex};

use api::caller_identity::CALLER_FLAG_VFS_MUTATE;
use api::ipc::{save_refusal, Observation, SaveExpect, SaveOutcome, VfsRequest, VfsResponse, IPC_BUF_SIZE};
use sha2::{Digest, Sha256};
use types::CellId;

use crate::caller::Caller;
use crate::manager::VfsManager;
use crate::paths::{ERR_HANDLE, ERR_IO, ERR_UNSUPPORTED};

const ALICE: Caller = Caller { flags: CALLER_FLAG_VFS_MUTATE, ..Caller::principal(CellId(81), 1) };
const BOB: Caller = Caller { flags: CALLER_FLAG_VFS_MUTATE, ..Caller::principal(CellId(82), 1) };

#[derive(Debug, PartialEq)]
enum Reply {
    Ok,
    Err(u8),
    Id(u32),
    Seen(Observation),
    Outcome(SaveOutcome),
    Data(Vec<u8>),
    Other(String),
}

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

fn manager() -> (VfsManager, Disk) {
    let mut vfs = VfsManager::new();
    let disk = Disk(Arc::new(HostMutex::new(alloc::vec![[0; cellos_fs::format::BLOCK_SIZE]; 512])));
    let srv = crate::backend_cellosfs::CellosFsBackend::formatted_for_test("/srv", disk.clone(), 512);
    let index = vfs.mounts.add_backend(Box::new(srv));
    vfs.mounts.mount("/srv", index);
    for caller in [ALICE, BOB] {
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
    }
    (vfs, disk)
}

fn call(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>) -> Reply {
    let mut request = [0u8; IPC_BUF_SIZE];
    api::ipc::encode(req, &mut request).expect("request fits");
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let resp = crate::dispatch::handle_request(vfs, &request, Some(caller), &mut resp_buf);
    let mut wire = [0u8; IPC_BUF_SIZE];
    api::ipc::encode(&resp, &mut wire).expect("reply fits");
    match api::ipc::decode::<VfsResponse>(&wire).expect("reply decodes") {
        VfsResponse::Ok => Reply::Ok,
        VfsResponse::Err(code) => Reply::Err(code),
        VfsResponse::SaveId(id) => Reply::Id(id),
        VfsResponse::Observed(observation) => Reply::Seen(observation),
        VfsResponse::SaveOutcome(outcome) => Reply::Outcome(outcome),
        VfsResponse::Data(data) => Reply::Data(data.to_vec()),
        other => Reply::Other(format!("{other:?}")),
    }
}

fn observe(vfs: &mut VfsManager, path: &str) -> Observation {
    match call(vfs, ALICE, &VfsRequest::Observe { path }) {
        Reply::Seen(observation) => observation,
        other => panic!("observe {path}: {other:?}"),
    }
}

/// Begin, stage in protocol-sized chunks and seal `bytes` for `dst`.
fn staged(vfs: &mut VfsManager, caller: Caller, dst: &str, bytes: &[u8]) -> u32 {
    let Reply::Id(id) = call(vfs, caller, &VfsRequest::SaveBegin { dst }) else { panic!("begin {dst}") };
    for (index, piece) in bytes.chunks(api::ipc::VFS_SAVE_CHUNK).enumerate() {
        let offset = (index * api::ipc::VFS_SAVE_CHUNK) as u64;
        assert_eq!(call(vfs, caller, &VfsRequest::SaveChunk { id, offset, bytes: piece }), Reply::Ok);
    }
    let sha256: [u8; 32] = Sha256::digest(bytes).into();
    assert_eq!(call(vfs, caller, &VfsRequest::SaveSeal { id, len: bytes.len() as u64, sha256 }), Reply::Ok);
    id
}

fn commit(vfs: &mut VfsManager, caller: Caller, id: u32, expect: SaveExpect) -> Reply {
    call(vfs, caller, &VfsRequest::SaveCommit { id, expect })
}

fn contents(vfs: &mut VfsManager, path: &str) -> Vec<u8> {
    let size = vfs.file_size(path) as usize;
    let mut out = alloc::vec![0; size];
    assert_eq!(vfs.read_at(path, 0, &mut out), size);
    out
}

fn staging_entries(vfs: &mut VfsManager) -> usize {
    let mut out = [0u8; 512];
    let written = vfs.list_dir("/srv/.cbx-save", &mut out);
    out[..written].iter().filter(|&&b| b == b'\n').count()
}
#[test]
fn create_then_replace_publish_exact_bytes_and_a_repeat_never_replays() {
    let (mut vfs, _) = manager();
    assert_eq!(observe(&mut vfs, "/srv/cfg"), Observation::Absent);
    // Three chunks, inside the 1,100-byte per-cell quota of `test-hooks` builds.
    let first: Vec<u8> = (0..1030u32).map(|i| (i % 251) as u8).collect();
    let id = staged(&mut vfs, ALICE, "/srv/cfg", &first);
    let Reply::Outcome(SaveOutcome::Committed { inode, generation }) = commit(&mut vfs, ALICE, id, SaveExpect::Absent) else {
        panic!("create");
    };
    assert_eq!(contents(&mut vfs, "/srv/cfg"), first);
    assert_eq!(observe(&mut vfs, "/srv/cfg"),
        Observation::Present { inode, generation, size: first.len() as u64, regular: true });
    // The destination changes after the first commit: a repeat answers the
    // recorded outcome instead of publishing again.
    assert_eq!(call(&mut vfs, BOB, &VfsRequest::Write { path: "/srv/cfg", content: b"bob" }), Reply::Ok);
    let recorded = Reply::Outcome(SaveOutcome::Committed { inode, generation });
    assert_eq!(commit(&mut vfs, ALICE, id, SaveExpect::Absent), recorded);
    assert_eq!(call(&mut vfs, ALICE, &VfsRequest::SaveQuery { id }), recorded);
    assert_eq!(contents(&mut vfs, "/srv/cfg"), b"bob");
    assert_eq!(call(&mut vfs, ALICE, &VfsRequest::SaveAbort { id }), Reply::Ok);
    assert_eq!(contents(&mut vfs, "/srv/cfg"), b"bob", "abort after commit keeps the destination");

    let expect = observe(&mut vfs, "/srv/cfg").expect().unwrap();
    let id = staged(&mut vfs, ALICE, "/srv/cfg", b"line\r\nlast");
    assert!(matches!(commit(&mut vfs, ALICE, id, expect), Reply::Outcome(SaveOutcome::Committed { .. })));
    assert_eq!(contents(&mut vfs, "/srv/cfg"), b"line\r\nlast");
    assert_eq!(staging_entries(&mut vfs), 0, "a published stage leaves nothing behind");
}

#[test]
fn stale_observations_conflict_and_leave_every_byte_in_place() {
    let (mut vfs, _) = manager();
    assert_eq!(call(&mut vfs, BOB, &VfsRequest::Write { path: "/srv/a", content: b"original" }), Reply::Ok);
    let expect = observe(&mut vfs, "/srv/a").expect().unwrap();
    let id = staged(&mut vfs, ALICE, "/srv/a", b"mine");
    // ABA: identical bytes written after the observation.
    assert_eq!(call(&mut vfs, BOB, &VfsRequest::Write { path: "/srv/a", content: b"original" }), Reply::Ok);
    assert_eq!(commit(&mut vfs, ALICE, id, expect), Reply::Outcome(SaveOutcome::Conflict));
    assert_eq!(contents(&mut vfs, "/srv/a"), b"original");
    // Save As to an absent destination that appears after confirmation.
    let id2 = staged(&mut vfs, ALICE, "/srv/b", b"mine");
    assert_eq!(call(&mut vfs, BOB, &VfsRequest::Write { path: "/srv/b", content: b"raced" }), Reply::Ok);
    assert_eq!(commit(&mut vfs, ALICE, id2, SaveExpect::Absent), Reply::Outcome(SaveOutcome::Conflict));
    assert_eq!(contents(&mut vfs, "/srv/b"), b"raced");
    assert_eq!(staging_entries(&mut vfs), 2);
    for id in [id, id2] {
        assert_eq!(call(&mut vfs, ALICE, &VfsRequest::SaveAbort { id }), Reply::Ok);
    }
    assert_eq!(staging_entries(&mut vfs), 0);
}

#[test]
fn unsealed_tampered_and_misdigested_stages_never_publish() {
    let (mut vfs, _) = manager();
    // Unsealed.
    let Reply::Id(id) = call(&mut vfs, ALICE, &VfsRequest::SaveBegin { dst: "/srv/u" }) else { panic!() };
    assert_eq!(call(&mut vfs, ALICE, &VfsRequest::SaveChunk { id, offset: 0, bytes: b"x" }), Reply::Ok);
    assert_eq!(commit(&mut vfs, ALICE, id, SaveExpect::Absent),
        Reply::Outcome(SaveOutcome::NotPublished(save_refusal::NOT_SEALED)));
    assert_eq!(vfs.stat("/srv/u"), None);
    // Digest of different bytes than storage holds.
    let Reply::Id(id) = call(&mut vfs, ALICE, &VfsRequest::SaveBegin { dst: "/srv/u" }) else { panic!() };
    assert_eq!(call(&mut vfs, ALICE, &VfsRequest::SaveChunk { id, offset: 0, bytes: b"abc" }), Reply::Ok);
    let wrong: [u8; 32] = Sha256::digest(b"abd").into();
    assert_eq!(call(&mut vfs, ALICE, &VfsRequest::SaveSeal { id, len: 3, sha256: wrong }), Reply::Err(ERR_IO));
    // Out-of-order chunk and another caller's id are refused.
    assert_eq!(call(&mut vfs, ALICE, &VfsRequest::SaveChunk { id, offset: 9, bytes: b"x" }), Reply::Err(ERR_HANDLE));
    assert_eq!(call(&mut vfs, BOB, &VfsRequest::SaveChunk { id, offset: 3, bytes: b"x" }), Reply::Err(ERR_HANDLE));
    assert_eq!(call(&mut vfs, BOB, &VfsRequest::SaveAbort { id }), Reply::Err(ERR_HANDLE));
    // Staged bytes rewritten after sealing.
    let id = staged(&mut vfs, ALICE, "/srv/u", b"sealed");
    let stage = format!("/srv/.cbx-save/{id:08x}");
    assert_eq!(call(&mut vfs, BOB, &VfsRequest::Write { path: &stage, content: b"evil!!" }), Reply::Ok);
    assert_eq!(commit(&mut vfs, ALICE, id, SaveExpect::Absent),
        Reply::Outcome(SaveOutcome::NotPublished(save_refusal::STAGE_CHANGED)));
    assert_eq!(vfs.stat("/srv/u"), None);
}

#[test]
fn backends_without_conditional_publication_refuse_before_staging() {
    let (mut vfs, _) = manager();
    assert_eq!(call(&mut vfs, ALICE, &VfsRequest::Observe { path: "/tmp/x" }), Reply::Err(ERR_UNSUPPORTED));
    assert_eq!(call(&mut vfs, ALICE, &VfsRequest::SaveBegin { dst: "/tmp/x" }), Reply::Err(ERR_UNSUPPORTED));
    assert_eq!(vfs.stat("/tmp/x"), None);
}

#[test]
fn transactions_are_bounded_per_caller_and_a_successor_generation_inherits_none() {
    let (mut vfs, _) = manager();
    for n in 0..crate::save::MAX_SAVES_PER_CALLER {
        assert!(matches!(call(&mut vfs, ALICE, &VfsRequest::SaveBegin { dst: &format!("/srv/f{n}") }), Reply::Id(_)));
    }
    assert_eq!(call(&mut vfs, ALICE, &VfsRequest::SaveBegin { dst: "/srv/over" }), Reply::Err(crate::paths::ERR_QUOTA));
    let successor = Caller { generation: 2, ..ALICE };
    let _ = vfs.dirs.on_contact(successor);
    vfs.dirs.mark_attested(successor);
    assert!(matches!(call(&mut vfs, successor, &VfsRequest::SaveBegin { dst: "/srv/next" }), Reply::Id(_)));
    // The dead generation's stages were removed with its transactions.
    assert_eq!(staging_entries(&mut vfs), 1);
}

#[test]
fn mount_removes_stages_a_previous_boot_left() {
    let (mut vfs, disk) = manager();
    let _ = staged(&mut vfs, ALICE, "/srv/left", b"orphan");
    assert_eq!(vfs.sync("/srv/left"), true);
    let mut fs = cellos_fs::CellosFs::open(disk.clone()).unwrap();
    crate::backend_cellosfs::clear_staging(&mut fs).unwrap();
    assert!(fs.list_dir("/.cbx-save").unwrap().is_empty());
    assert_eq!(fs.lookup("/left").err(), Some(cellos_fs::FsError::NotFound));
}
