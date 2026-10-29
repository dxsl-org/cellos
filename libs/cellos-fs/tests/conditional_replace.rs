//! D03 engine primitive: generation-bound conditional publication.

// The default-off host CoW experiment does not provide the D03 primitive.
#![cfg(not(feature = "g2-cow-experiment"))]

use cellos_fs::{CellosFs, Expect, FsError, MemDisk};

fn volume() -> CellosFs<MemDisk> {
    CellosFs::format(MemDisk::new(1024), 1024).unwrap()
}

fn put(fs: &mut CellosFs<MemDisk>, path: &str, data: &[u8]) {
    if fs.lookup(path).is_err() {
        fs.create_file(path).unwrap();
    }
    fs.replace_file(path, data).unwrap();
    fs.sync().unwrap();
}

fn read(fs: &mut CellosFs<MemDisk>, path: &str) -> Vec<u8> {
    let size = fs.lookup(path).unwrap().size as usize;
    let mut buf = vec![0; size];
    assert_eq!(fs.read_file(path, 0, &mut buf).unwrap(), size);
    buf
}

fn observe(fs: &mut CellosFs<MemDisk>, path: &str) -> Expect {
    let inode = fs.lookup(path).unwrap();
    Expect::Present { inode_num: inode.inode_num, generation: inode.generation }
}

#[test]
fn generation_advances_on_every_mutation_and_survives_reopen() {
    let mut fs = volume();
    put(&mut fs, "/f", b"one");
    let first = fs.lookup("/f").unwrap().generation;
    put(&mut fs, "/f", b"one");
    let second = fs.lookup("/f").unwrap().generation;
    assert!(second > first, "identical rewrite must still advance the generation");
    let mut fs = CellosFs::open(fs.into_disk()).unwrap();
    assert_eq!(fs.lookup("/f").unwrap().generation, second);
}

#[test]
fn replace_if_matching_observation_publishes_and_frees_old_blocks() {
    let mut fs = volume();
    let big = vec![b'o'; 3 * 4096 + 7];
    put(&mut fs, "/cfg.toml", &big);
    let expect = observe(&mut fs, "/cfg.toml");
    let new = vec![b'n'; 4096 + 1];
    put(&mut fs, "/.stage", &new);
    let free_before = fs.free_blocks();

    fs.rename_replace_if("/.stage", "/cfg.toml", expect).unwrap();
    fs.sync().unwrap();

    assert_eq!(fs.lookup("/.stage").err(), Some(FsError::NotFound));
    assert_eq!(read(&mut fs, "/cfg.toml"), new);
    // The four old data blocks return (inode-table copy-on-write is block-neutral).
    assert_eq!(fs.free_blocks(), free_before + 4);
    let mut fs = CellosFs::open(fs.into_disk()).unwrap();
    assert_eq!(read(&mut fs, "/cfg.toml"), new);
    assert_ne!(observe(&mut fs, "/cfg.toml"), expect);
}

#[test]
fn stale_observation_conflicts_without_any_change() {
    let mut fs = volume();
    put(&mut fs, "/a", b"original");
    let expect = observe(&mut fs, "/a");
    // ABA: same bytes rewritten after observation.
    put(&mut fs, "/a", b"original");
    put(&mut fs, "/.stage", b"candidate");
    let free = fs.free_blocks();
    let seq = fs.superblock().sequence;

    assert_eq!(fs.rename_replace_if("/.stage", "/a", expect), Err(FsError::Conflict));
    fs.sync().unwrap();

    assert_eq!(read(&mut fs, "/a"), b"original");
    assert_eq!(read(&mut fs, "/.stage"), b"candidate");
    assert_eq!(fs.free_blocks(), free);
    // The sync published nothing staged: only the flip itself happened.
    assert_eq!(fs.superblock().sequence, seq + 1);
}

#[test]
fn absent_and_present_expectations_are_enforced_both_ways() {
    let mut fs = volume();
    put(&mut fs, "/.stage", b"new");
    // Destination created after an absent observation.
    put(&mut fs, "/b", b"raced");
    assert_eq!(fs.rename_replace_if("/.stage", "/b", Expect::Absent), Err(FsError::Conflict));
    assert_eq!(read(&mut fs, "/b"), b"raced");
    // Destination removed after a present observation.
    let expect = observe(&mut fs, "/b");
    fs.unlink("/b").unwrap();
    fs.sync().unwrap();
    assert_eq!(fs.rename_replace_if("/.stage", "/b", expect), Err(FsError::Conflict));
    assert_eq!(fs.lookup("/b").err(), Some(FsError::NotFound));
    // Still absent: creation publishes.
    fs.rename_replace_if("/.stage", "/b", Expect::Absent).unwrap();
    fs.sync().unwrap();
    assert_eq!(read(&mut fs, "/b"), b"new");
}

#[test]
fn directory_destination_and_cross_directory_publication() {
    let mut fs = volume();
    fs.create_dir("/d").unwrap();
    fs.create_dir("/e").unwrap();
    fs.sync().unwrap();
    put(&mut fs, "/d/.stage", b"x");
    let dir = fs.lookup("/e").unwrap();
    let as_dir = Expect::Present { inode_num: dir.inode_num, generation: dir.generation };
    assert_eq!(fs.rename_replace_if("/d/.stage", "/e", as_dir), Err(FsError::Conflict));
    put(&mut fs, "/e/t", b"old");
    let expect = observe(&mut fs, "/e/t");
    fs.rename_replace_if("/d/.stage", "/e/t", expect).unwrap();
    fs.sync().unwrap();
    let mut fs = CellosFs::open(fs.into_disk()).unwrap();
    assert_eq!(read(&mut fs, "/e/t"), b"x");
    assert!(fs.list_dir("/d").unwrap().is_empty());
}

/// S9-4a: the durable-commit skeleton refuses, so no caller can report
/// `CommittedDurable` from this engine, and the refusal leaves the volume usable.
#[test]
fn durable_commit_is_refused_and_the_volume_stays_usable() {
    let mut fs = volume();
    put(&mut fs, "/a", b"one");
    fs.replace_file("/a", b"two").unwrap();
    assert_eq!(fs.sync_durable(), Err(cellos_fs::DurabilityUnavailable));
    fs.sync().unwrap();
    assert_eq!(read(&mut fs, "/a"), b"two");
}
