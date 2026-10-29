//! Boot self-test (`test-hooks`) of D03 conditional save on the target, through
//! the real dispatcher over a private in-memory CellosFS volume at `/s8`. No
//! user volume is touched and no kernel attestation is consumed.

use alloc::boxed::Box;
use alloc::vec::Vec;

use api::caller_identity::CALLER_FLAG_VFS_MUTATE;
use api::ipc::{save_refusal, Observation, SaveExpect, SaveOutcome, VfsRequest, VfsResponse, IPC_BUF_SIZE};
use cellos_fs::format::BLOCK_SIZE;
use sha2::{Digest, Sha256};
use types::CellId;

use crate::caller::Caller;
use crate::manager::VfsManager;

const ALICE: Caller = Caller { flags: CALLER_FLAG_VFS_MUTATE, ..Caller::principal(CellId(78), 1) };
const BOB: Caller = Caller { flags: CALLER_FLAG_VFS_MUTATE, ..Caller::principal(CellId(79), 1) };
const BLOCKS: usize = 64;

struct Ram(Vec<[u8; BLOCK_SIZE]>);

impl cellos_fs::disk::BlockDevice for Ram {
    fn total_blocks(&self) -> u64 { self.0.len() as u64 }
    fn read_block(&mut self, block: u64, out: &mut [u8; BLOCK_SIZE]) -> Result<(), cellos_fs::disk::FsError> {
        out.copy_from_slice(self.0.get(block as usize).ok_or(cellos_fs::disk::FsError::PartitionOverflow)?);
        Ok(())
    }
    fn write_block(&mut self, block: u64, data: &[u8; BLOCK_SIZE]) -> Result<(), cellos_fs::disk::FsError> {
        *self.0.get_mut(block as usize).ok_or(cellos_fs::disk::FsError::PartitionOverflow)? = *data;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), cellos_fs::disk::FsError> { Ok(()) }
}

fn call<'a>(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>, resp: &'a mut [u8; IPC_BUF_SIZE]) -> VfsResponse<'a> {
    let mut request = [0u8; IPC_BUF_SIZE];
    api::ipc::encode(req, &mut request).expect("request fits");
    crate::dispatch::handle_request(vfs, &request, Some(caller), resp)
}

fn ok(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>) -> bool {
    matches!(call(vfs, caller, req, &mut [0; IPC_BUF_SIZE]), VfsResponse::Ok)
}

fn observe(vfs: &mut VfsManager, path: &str) -> Option<Observation> {
    match call(vfs, ALICE, &VfsRequest::Observe { path }, &mut [0; IPC_BUF_SIZE]) {
        VfsResponse::Observed(observation) => Some(observation),
        _ => None,
    }
}

fn staged(vfs: &mut VfsManager, dst: &str, bytes: &[u8]) -> Option<u32> {
    let VfsResponse::SaveId(id) = call(vfs, ALICE, &VfsRequest::SaveBegin { dst }, &mut [0; IPC_BUF_SIZE]) else { return None };
    for (index, piece) in bytes.chunks(api::ipc::VFS_SAVE_CHUNK).enumerate() {
        let offset = (index * api::ipc::VFS_SAVE_CHUNK) as u64;
        if !ok(vfs, ALICE, &VfsRequest::SaveChunk { id, offset, bytes: piece }) { return None; }
    }
    let sha256: [u8; 32] = Sha256::digest(bytes).into();
    ok(vfs, ALICE, &VfsRequest::SaveSeal { id, len: bytes.len() as u64, sha256 }).then_some(id)
}

fn commit(vfs: &mut VfsManager, id: u32, expect: SaveExpect) -> Option<SaveOutcome> {
    match call(vfs, ALICE, &VfsRequest::SaveCommit { id, expect }, &mut [0; IPC_BUF_SIZE]) {
        VfsResponse::SaveOutcome(outcome) => Some(outcome),
        _ => None,
    }
}

fn contents(vfs: &mut VfsManager, path: &str) -> Vec<u8> {
    let mut out = alloc::vec![0; vfs.file_size(path) as usize];
    let read = vfs.read_at(path, 0, &mut out);
    out.truncate(read);
    out
}

/// Literal marker text, so the lane can prove the binary carries this self-test.
macro_rules! report {
    ($name:literal, $pass:expr) => {
        ostd::io::println(if $pass { concat!("[vfs-save] ", $name, " PASS") } else { concat!("[vfs-save] ", $name, " FAIL") })
    };
}

pub fn run() {
    let [replaced, conflict, tampered, refused] = checks();
    report!("create-and-replace-publish-exact-bytes", replaced);
    report!("stale-observation-conflicts-without-change", conflict);
    report!("tampered-stage-not-published", tampered);
    report!("unsupported-backend-refused", refused);
}

/// The four results, in marker order. Sizes stay inside the 1,100-byte
/// per-cell quota of `test-hooks` builds.
fn checks() -> [bool; 4] {
    let mut vfs = VfsManager::new();
    // Mounted below `/srv` so the ordinary `/srv` access rule applies; the
    // longer prefix shadows the user volume, which is never touched.
    let volume = crate::backend_cellosfs::CellosFsBackend::formatted_for_test(
        "/srv/s8test", Ram(alloc::vec![[0; BLOCK_SIZE]; BLOCKS]), BLOCKS as u64);
    let index = vfs.mounts.add_backend(Box::new(volume));
    vfs.mounts.mount("/srv/s8test", index);
    for caller in [ALICE, BOB] {
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
    }
    let cfg = "/srv/s8test/cfg";

    // Multi-chunk create, then a replace bound to the fresh observation.
    let first: Vec<u8> = (0..700u32).map(|i| (i % 251) as u8).collect();
    let created = observe(&mut vfs, cfg) == Some(Observation::Absent)
        && staged(&mut vfs, cfg, &first)
            .and_then(|id| commit(&mut vfs, id, SaveExpect::Absent))
            .is_some_and(|o| o.published().is_some())
        && contents(&mut vfs, cfg) == first;
    let replaced = created && observe(&mut vfs, cfg).and_then(Observation::expect).is_some_and(|expect| {
        staged(&mut vfs, cfg, b"a\r\nb").and_then(|id| commit(&mut vfs, id, expect))
            .is_some_and(|o| o.published().is_some())
    }) && contents(&mut vfs, cfg) == b"a\r\nb";

    // Identical bytes rewritten after the observation still conflict.
    let expect = observe(&mut vfs, cfg).and_then(Observation::expect);
    let id = staged(&mut vfs, cfg, b"mine");
    let aba = ok(&mut vfs, BOB, &VfsRequest::Write { path: cfg, content: b"a\r\nb" });
    let conflict = match (expect, id) {
        (Some(expect), Some(id)) if aba => commit(&mut vfs, id, expect) == Some(SaveOutcome::Conflict)
            && commit(&mut vfs, id, expect) == Some(SaveOutcome::Conflict)
            && ok(&mut vfs, ALICE, &VfsRequest::SaveAbort { id }),
        _ => false,
    } && contents(&mut vfs, cfg) == b"a\r\nb";

    // Staged bytes changed after sealing never publish.
    let tampered = staged(&mut vfs, "/srv/s8test/new", b"sealed").is_some_and(|id| {
        let stage = alloc::format!("/srv/s8test/.cbx-save/{id:08x}");
        ok(&mut vfs, BOB, &VfsRequest::Write { path: &stage, content: b"evil!!" })
            && commit(&mut vfs, id, SaveExpect::Absent) == Some(SaveOutcome::NotPublished(save_refusal::STAGE_CHANGED))
    }) && vfs.stat("/srv/s8test/new").is_none();

    // A backend without conditional publication refuses before staging.
    let refused = matches!(call(&mut vfs, ALICE, &VfsRequest::SaveBegin { dst: "/tmp/s8" }, &mut [0; IPC_BUF_SIZE]),
        VfsResponse::Err(crate::paths::ERR_UNSUPPORTED)) && vfs.stat("/tmp/s8").is_none();
    [replaced, conflict, tampered, refused]
}

#[cfg(test)]
#[test]
fn boot_self_test_passes_under_test_hooks_quota() {
    assert_eq!(checks(), [true; 4]);
}
