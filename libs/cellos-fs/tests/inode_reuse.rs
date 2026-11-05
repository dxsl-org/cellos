//! Inode-number reuse without ABA, and all-or-nothing request staging.

// The default-off host CoW experiment keeps its own allocation model.
#![cfg(not(feature = "g2-cow-experiment"))]

use cellos_fs::{CellosFs, Expect, FsError, MemDisk};

fn volume() -> CellosFs<MemDisk> {
    CellosFs::format(MemDisk::new(1024), 1024).unwrap()
}

#[test]
fn create_unlink_cycles_outlive_the_256_entry_inode_table() {
    let mut fs = volume();
    fs.create_dir("/d").unwrap();
    for i in 0..600 {
        let path = format!("/d/f{}", i % 5);
        fs.create_file(&path).unwrap();
        fs.write_file(&path, 0, &[i as u8; 5000]).unwrap();
        fs.sync().unwrap();
        fs.unlink(&path).unwrap();
        fs.create_dir("/e").unwrap();
        fs.rmdir("/e").unwrap();
        fs.sync().unwrap();
    }
    // Only `/d` and the two recycled numbers were ever handed out.
    assert!(fs.superblock().next_inode_num <= 5);
    let free = fs.free_blocks();
    let mut fs = CellosFs::open(fs.into_disk()).unwrap();
    assert_eq!(fs.free_blocks(), free);
    fs.create_file("/d/after").unwrap();
    assert!(fs.lookup("/d/after").unwrap().inode_num < 5);
}

#[test]
fn reused_number_never_matches_an_observation_of_its_previous_file() {
    let mut fs = volume();
    fs.create_file("/a").unwrap();
    fs.replace_file("/a", b"first").unwrap();
    fs.sync().unwrap();
    let old = fs.lookup("/a").unwrap();
    fs.unlink("/a").unwrap();
    fs.create_file("/b").unwrap();
    fs.replace_file("/b", b"second").unwrap();
    let reused = fs.lookup("/b").unwrap();
    assert_eq!(reused.inode_num, old.inode_num);
    assert!(reused.generation > old.generation);
    fs.create_file("/stage").unwrap();
    let stale = Expect::Present {
        inode_num: old.inode_num,
        generation: old.generation,
    };
    assert_eq!(
        fs.rename_replace_if("/stage", "/b", stale),
        Err(FsError::Conflict)
    );

    // The retirement survives a remount: the tombstone carries the count on.
    fs.sync().unwrap();
    let last = fs.lookup("/b").unwrap();
    fs.unlink("/b").unwrap();
    fs.sync().unwrap();
    let mut fs = CellosFs::open(fs.into_disk()).unwrap();
    fs.create_file("/c").unwrap();
    let again = fs.lookup("/c").unwrap();
    assert_eq!(again.inode_num, last.inode_num);
    assert!(again.generation > last.generation);
}

#[test]
fn refused_request_inside_atomically_stages_nothing() {
    let mut fs = volume();
    fs.create_dir("/full").unwrap();
    for i in 0..7 {
        fs.create_file(&format!("/full/{i}")).unwrap();
    }
    fs.sync().unwrap();
    let free = fs.free_blocks();
    let next = fs.superblock().next_inode_num;
    let refused = fs.atomically(|fs| {
        fs.create_file("/staged")?;
        fs.write_file("/staged", 0, &[7; 3 * 4096])?;
        fs.create_file("/full/eighth").map(|_| ())
    });
    assert_eq!(refused, Err(FsError::NoSpace));
    assert_eq!(fs.lookup("/staged").err(), Some(FsError::NotFound));
    assert_eq!(fs.free_blocks(), free);
    assert_eq!(fs.superblock().next_inode_num, next);
    fs.sync().unwrap();
    let mut fs = CellosFs::open(fs.into_disk()).unwrap();
    assert_eq!(fs.lookup("/staged").err(), Some(FsError::NotFound));
    assert_eq!(fs.free_blocks(), free);
}

#[test]
fn refused_extent_growth_inside_atomically_returns_its_blocks() {
    let mut fs = CellosFs::format(MemDisk::new(256), 256).unwrap();
    fs.create_file("/a").unwrap();
    fs.create_file("/b").unwrap();
    // Interleaving fragments `/a` into one extent per block.
    for block in 0..26 {
        fs.write_file("/a", block * 4096, &[0xAA; 4096]).unwrap();
        fs.write_file("/b", block * 4096, &[0xBB; 4096]).unwrap();
    }
    fs.sync().unwrap();
    let free = fs.free_blocks();
    let refused = fs.atomically(|fs| fs.write_file("/a", 26 * 4096, &[0xAA; 4096]));
    assert_eq!(refused, Err(FsError::NoSpace));
    assert_eq!(fs.free_blocks(), free);
    assert_eq!(fs.lookup("/a").unwrap().size, 26 * 4096);
}
