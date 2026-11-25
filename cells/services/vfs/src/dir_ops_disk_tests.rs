//! `Rename`/`RenameAt` on the secondary-disk FAT volumes (`/mnt/disk/<device>`)
//! end to end through `dispatch::handle_request`, and where rename stays
//! refused.

mod mem_fat;

use alloc::boxed::Box;

use abi::caller_identity::CALLER_FLAG_VFS_MUTATE;
use abi::dir_handles::ViDirHandle;
use types::CellId;
use wire::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use wire::proto::vfs::VfsError;

use crate::backend_fat::rename::tests::volume;
use crate::caller::Caller;
use crate::manager::VfsManager;
use mem_fat::MemFat;

const WRITER: Caller = Caller {
    flags: CALLER_FLAG_VFS_MUTATE,
    ..Caller::principal(CellId(81), 1)
};
const READER: Caller = Caller::principal(CellId(82), 1);

/// Two disk volumes, plus a FAT volume at `/mnt/sd` that rename must not reach.
fn manager() -> VfsManager {
    let mut vfs = VfsManager::new();
    for prefix in ["/mnt/disk/a", "/mnt/disk/b", "/mnt/sd"] {
        let fs = volume();
        let index = vfs.mounts.add_backend(Box::new(MemFat { prefix, fs }));
        vfs.mounts.mount(prefix, index);
    }
    for caller in [WRITER, READER] {
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
    }
    vfs
}

#[derive(Debug, PartialEq)]
enum Reply {
    Ok,
    Err(u8),
    Dir(ViDirHandle),
}

fn call(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>) -> Reply {
    let mut request = [0u8; IPC_BUF_SIZE];
    wire::ipc::encode(req, &mut request).expect("request fits");
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match crate::dispatch::handle_request(vfs, &request, Some(caller), &mut resp_buf) {
        VfsResponse::Ok => Reply::Ok,
        VfsResponse::Err(code) => Reply::Err(code),
        VfsResponse::DirHandle(dir) => Reply::Dir(dir),
        other => panic!("unexpected reply {other:?}"),
    }
}

fn write(vfs: &mut VfsManager, path: &str, content: &[u8]) {
    let req = VfsRequest::Write { path, content };
    assert_eq!(call(vfs, WRITER, &req), Reply::Ok, "write {path}");
}

fn mkdir(vfs: &mut VfsManager, path: &str) {
    assert_eq!(call(vfs, WRITER, &VfsRequest::Mkdir(path)), Reply::Ok);
}

fn rename(vfs: &mut VfsManager, caller: Caller, old: &str, new: &str) -> Reply {
    call(vfs, caller, &VfsRequest::Rename { old, new })
}

fn open_dir(vfs: &mut VfsManager, dir: ViDirHandle, name: &str) -> ViDirHandle {
    match call(vfs, WRITER, &VfsRequest::OpenDir { dir, name }) {
        Reply::Dir(sub) => sub,
        other => panic!("open {name}: {other:?}"),
    }
}

const DENIED: Reply = Reply::Err(VfsError::Denied as u8);

#[test]
fn a_disk_file_moves_and_replaces_with_its_charge_and_handles_following() {
    let mut vfs = manager();
    write(&mut vfs, "/mnt/disk/a/f", b"one");
    mkdir(&mut vfs, "/mnt/disk/a/d");
    write(&mut vfs, "/mnt/disk/a/old", b"old!");
    assert_eq!(
        rename(&mut vfs, WRITER, "/mnt/disk/a/f", "/mnt/disk/a/g"),
        Reply::Ok
    );
    assert_eq!(vfs.stat("/mnt/disk/a/f"), Ok(None));
    assert_eq!(
        rename(&mut vfs, WRITER, "/mnt/disk/a/g", "/mnt/disk/a/d/h"),
        Reply::Ok
    );
    assert_eq!(vfs.stat("/mnt/disk/a/d/h"), Ok(Some((3, false))));

    // A handle on the replaced file is marked: it now names the moved one.
    let held = vfs
        .files
        .insert(READER, "/mnt/disk/a/old", 0)
        .expect("handle");
    let used = vfs.quota.used(WRITER.cell);
    assert_eq!(
        rename(&mut vfs, WRITER, "/mnt/disk/a/d/h", "/mnt/disk/a/old"),
        Reply::Ok
    );
    assert_eq!(
        vfs.read_to_vec("/mnt/disk/a/old").as_deref(),
        Ok(&b"one"[..])
    );
    assert_eq!(vfs.stat("/mnt/disk/a/d/h"), Ok(None));
    assert_eq!(
        vfs.files.state_of(READER, held),
        Some(("/mnt/disk/a/old", true))
    );
    // The replaced bytes and entry go back; the moved charge stays.
    let replaced = 4 + vfs.quota.entry_charge();
    assert_eq!(vfs.quota.used(WRITER.cell), used - replaced);
    assert_eq!(vfs.quota.writer_of("/mnt/disk/a/old"), Some(WRITER.cell));
    assert_eq!(vfs.quota.writer_of("/mnt/disk/a/d/h"), None);
}

#[test]
fn a_sealed_cell_moves_a_directory_between_handles_but_not_onto_a_full_one() {
    let mut vfs = manager();
    let open = VfsRequest::OpenRootDir {
        path: "/mnt/disk/a",
    };
    let root = match call(&mut vfs, WRITER, &open) {
        Reply::Dir(dir) => dir,
        other => panic!("open root: {other:?}"),
    };
    assert_eq!(call(&mut vfs, WRITER, &VfsRequest::SealPaths), Reply::Ok);
    for name in ["src", "full", "dst"] {
        let mkdir = VfsRequest::MkdirAt { dir: root, name };
        assert_eq!(call(&mut vfs, WRITER, &mkdir), Reply::Ok);
    }
    let src = open_dir(&mut vfs, root, "src");
    let full = open_dir(&mut vfs, root, "full");
    let dst = open_dir(&mut vfs, root, "dst");
    for (dir, name) in [(src, "x"), (full, "y")] {
        let content = name.as_bytes();
        let req = VfsRequest::WriteAt { dir, name, content };
        assert_eq!(call(&mut vfs, WRITER, &req), Reply::Ok);
    }
    let rename_at = |new_dir, new| VfsRequest::RenameAt {
        old_dir: root,
        old: "src",
        new_dir,
        new,
    };
    let io = Reply::Err(VfsError::Io as u8);
    assert_eq!(call(&mut vfs, WRITER, &rename_at(root, "full")), io);
    assert_eq!(
        vfs.read_to_vec("/mnt/disk/a/full/y").as_deref(),
        Ok(&b"y"[..])
    );
    assert_eq!(call(&mut vfs, WRITER, &rename_at(dst, "moved")), Reply::Ok);
    assert_eq!(
        vfs.read_to_vec("/mnt/disk/a/dst/moved/x").as_deref(),
        Ok(&b"x"[..])
    );
    assert_eq!(vfs.stat("/mnt/disk/a/src"), Ok(None));
    // The handle on the old name is marked, not moved.
    let entry = &vfs.dirs.entries[&src.0];
    assert!(entry.changed);
    assert_eq!(entry.path, "/mnt/disk/a/src");
}

#[test]
fn rename_is_refused_outside_disk_and_srv_and_between_volumes() {
    let mut vfs = manager();
    write(&mut vfs, "/mnt/sd/f", b"sd");
    write(&mut vfs, "/mnt/disk/a/f", b"a");
    for (old, new) in [
        ("/mnt/sd/f", "/mnt/sd/g"),
        ("/tmp/f", "/tmp/g"),
        ("/mnt/disk/a/f", "/srv/f"),
        ("/srv/f", "/mnt/disk/a/f"),
        ("/mnt/disk/a/f", "/mnt/sd/f"),
        // The volume roots themselves.
        ("/mnt/disk/a", "/mnt/disk/c"),
        ("/mnt/disk/a/f", "/mnt/disk/a"),
        ("/mnt/disk/a/f", "/mnt/disk"),
    ] {
        assert_eq!(rename(&mut vfs, WRITER, old, new), DENIED, "{old} -> {new}");
    }
    // Without the mutate right nothing moves either.
    assert_eq!(
        rename(&mut vfs, READER, "/mnt/disk/a/f", "/mnt/disk/a/g"),
        DENIED
    );
    let across = rename(&mut vfs, WRITER, "/mnt/disk/a/f", "/mnt/disk/b/f");
    assert_eq!(across, Reply::Err(VfsError::Unsupported as u8));
    assert_eq!(vfs.read_to_vec("/mnt/sd/f").as_deref(), Ok(&b"sd"[..]));
    assert_eq!(vfs.read_to_vec("/mnt/disk/a/f").as_deref(), Ok(&b"a"[..]));
    assert_eq!(vfs.stat("/mnt/disk/b/f"), Ok(None));
}
