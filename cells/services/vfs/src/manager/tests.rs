use crate::caller::Caller;
use crate::manager::VfsManager;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use types::CellId;

const CELL_OLD: Caller = Caller {
    cell: CellId(44),
    generation: 1,
};
const CELL_OTHER: Caller = Caller {
    cell: CellId(55),
    generation: 1,
};
const CELL_OLD_RESPAWNED: Caller = Caller {
    cell: CellId(44),
    generation: 2,
};

#[test]
fn handle_traversal_can_open_mount_parent_directory() {
    let mut vfs = VfsManager::new();
    assert!(vfs.is_mount_ancestor("/mnt"));
    assert!(!vfs.is_mount_ancestor("/mn"));
    assert!(!vfs.is_mount_ancestor("/mnt/sd"));
    let _ = vfs.dirs.on_contact(CELL_OLD);
    vfs.dirs.mark_attested(CELL_OLD);

    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let root = match crate::dispatch_dirs::handle(
        &mut vfs,
        CELL_OLD,
        &VfsRequest::OpenRootDir { path: "/" },
        &mut resp_buf,
    ) {
        VfsResponse::DirHandle(handle) => handle,
        other => panic!("open root failed: {:?}", other),
    };

    let mnt = match crate::dispatch_dirs::handle(
        &mut vfs,
        CELL_OLD,
        &VfsRequest::OpenDir {
            dir: root,
            name: "mnt",
        },
        &mut resp_buf,
    ) {
        VfsResponse::DirHandle(handle) => handle,
        other => panic!("open /mnt failed: {:?}", other),
    };

    assert_eq!(vfs.dirs.dir_path(CELL_OLD, mnt), Some("/mnt"));
}

#[test]
fn owner_death_purges_only_the_watched_owner() {
    let mut vfs = VfsManager::new();
    let _ = vfs.dirs.on_contact(CELL_OLD);
    vfs.dirs.mark_attested(CELL_OLD);
    let _ = vfs.dirs.on_contact(CELL_OTHER);
    vfs.dirs.mark_attested(CELL_OTHER);
    let old_pending = vfs.pending.insert(CELL_OLD, "/data/old", vec![1u8]);
    let other_pending = vfs.pending.insert(CELL_OTHER, "/data/new", vec![2u8]);
    let old_dir = vfs.dirs.open_root(CELL_OLD, "/tmp").expect("old dir");
    let other_dir = vfs.dirs.open_root(CELL_OTHER, "/tmp").expect("other dir");
    let old_file = vfs
        .files
        .insert(CELL_OLD, "/tmp/old.txt", old_dir.0)
        .expect("old file");
    let other_file = vfs
        .files
        .insert(CELL_OTHER, "/tmp/new.txt", other_dir.0)
        .expect("other file");

    assert_eq!(
        vfs.should_watch_after_response(CELL_OLD, &api::ipc::VfsResponse::FileHandle(old_file)),
        Some(CELL_OLD.cell.0 as usize)
    );
    assert_eq!(
        vfs.should_watch_after_response(CELL_OTHER, &api::ipc::VfsResponse::PendingHandle(2)),
        Some(CELL_OTHER.cell.0 as usize)
    );

    assert!(vfs.handle_unattributed_owner_death(CELL_OLD.cell.0 as usize));
    assert_eq!(vfs.pending.poll(CELL_OLD, old_pending), None);
    assert_eq!(vfs.pending.poll(CELL_OTHER, other_pending), Some(vec![2u8]));
    assert_eq!(vfs.dirs.dir_path(CELL_OLD, old_dir), None);
    assert_eq!(vfs.dirs.dir_path(CELL_OTHER, other_dir), Some("/tmp"));
    assert!(!vfs.files.close(CELL_OLD, old_file));
    assert!(vfs.files.close(CELL_OTHER, other_file));
}

#[test]
fn higher_generation_reaps_predecessor_and_cross_cell_anchored_files() {
    let mut vfs = VfsManager::new();
    let _ = vfs.dirs.on_contact(CELL_OLD);
    vfs.dirs.mark_attested(CELL_OLD);
    let _ = vfs.dirs.on_contact(CELL_OTHER);
    vfs.dirs.mark_attested(CELL_OTHER);
    let old_root = vfs.dirs.open_root(CELL_OLD, "/tmp").expect("old root");
    let inherited = vfs
        .dirs
        .insert(
            CELL_OTHER,
            alloc::string::String::from("/tmp"),
            Some(old_root.0),
        )
        .expect("inherited dir");
    let old_file = vfs
        .files
        .insert(CELL_OLD, "/tmp/old.txt", old_root.0)
        .expect("old file");
    let cross_file = vfs
        .files
        .insert(CELL_OTHER, "/tmp/cross.txt", inherited.0)
        .expect("cross file");

    assert!(crate::dir_admission::prepare_contact(
        &mut vfs,
        CELL_OLD_RESPAWNED
    ));

    assert!(!vfs.files.close(CELL_OLD, old_file));
    assert!(!vfs.files.close(CELL_OTHER, cross_file));
    assert!(vfs.dirs.dir_path(CELL_OTHER, inherited).is_none());
}
