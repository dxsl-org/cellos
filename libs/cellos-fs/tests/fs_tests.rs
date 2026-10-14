//! Comprehensive host test suite for CellosFS Native.

use cellos_fs::{CellosFs, FsError, MemDisk};
#[test]
fn test_format_and_open() {
    let disk = MemDisk::new(1024); // 4 MiB
    let fs = CellosFs::format(disk, 1024).expect("format failed");
    assert_eq!(fs.superblock().sequence, 1);
    assert_eq!(fs.superblock().total_blocks, 1024);

    // Reopen
    let disk = fs.into_disk();
    let fs = CellosFs::open(disk).expect("reopen failed");
    assert_eq!(fs.superblock().sequence, 1);
}

#[test]
fn test_create_and_read_inline_file() {
    let disk = MemDisk::new(1024);
    let mut fs = CellosFs::format(disk, 1024).expect("format failed");

    fs.create_file("/hello.txt").expect("create file");

    let msg = b"Hello from CellosFS Native on Single Address Space!";
    let written = fs.write_file("/hello.txt", 0, msg).expect("write");
    assert_eq!(written, msg.len());

    let mut read_buf = [0u8; 128];
    let n = fs.read_file("/hello.txt", 0, &mut read_buf).expect("read");
    assert_eq!(n, msg.len());
    assert_eq!(&read_buf[..n], msg);
}

#[test]
fn test_create_and_read_large_extent_file() {
    let disk = MemDisk::new(1024);
    let mut fs = CellosFs::format(disk, 1024).expect("format failed");

    fs.create_file("/large.dat").expect("create file");

    // 16 KiB data (spans 4 blocks)
    let mut payload = vec![0u8; 16 * 1024];
    for (i, b) in payload.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }

    let written = fs.write_file("/large.dat", 0, &payload).expect("write");
    assert_eq!(written, payload.len());

    let mut read_buf = vec![0u8; 16 * 1024];
    let n = fs.read_file("/large.dat", 0, &mut read_buf).expect("read");
    assert_eq!(n, payload.len());
    assert_eq!(read_buf, payload);
}

#[test]
fn test_directory_hierarchy_and_listing() {
    let disk = MemDisk::new(1024);
    let mut fs = CellosFs::format(disk, 1024).expect("format failed");

    fs.create_dir("/logs").expect("create dir");
    fs.create_file("/logs/robot.log")
        .expect("create child file");

    let log_data = b"2026-09-05: LAB-01 carrier transfer OK";
    fs.write_file("/logs/robot.log", 0, log_data)
        .expect("write log");

    let entries = fs.list_dir("/logs").expect("list dir");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "robot.log");
    assert!(!entries[0].1); // is_dir == false
    assert_eq!(entries[0].2, log_data.len() as u64);
}

#[test]
fn test_unlink_and_reclaim() {
    let disk = MemDisk::new(1024);
    let mut fs = CellosFs::format(disk, 1024).expect("format failed");

    fs.create_file("/temp.bin").expect("create file");
    let payload = vec![0xAA; 8192];
    fs.write_file("/temp.bin", 0, &payload).expect("write");
    let free_before = fs.free_blocks();
    fs.unlink("/temp.bin").expect("unlink");
    fs.sync().expect("sync");

    let free_after = fs.free_blocks();
    assert!(free_after > free_before);
}

#[test]
fn test_sync_advances_cyclic_superblocks() {
    let disk = MemDisk::new(1024);
    let mut fs = CellosFs::format(disk, 1024).expect("format failed");
    assert_eq!(fs.superblock().sequence, 1);

    fs.create_file("/state.txt").expect("create");
    fs.write_file("/state.txt", 0, b"seq=1").expect("write");
    fs.sync().expect("sync 1");
    assert_eq!(fs.superblock().sequence, 2);

    fs.write_file("/state.txt", 0, b"seq=2").expect("write");
    fs.sync().expect("sync 2");
    assert_eq!(fs.superblock().sequence, 3);

    // Reopen and verify latest state is recovered
    let disk = fs.into_disk();
    let mut reopened = CellosFs::open(disk).expect("reopen");
    assert_eq!(reopened.superblock().sequence, 3);

    let mut buf = [0u8; 16];
    let n = reopened.read_file("/state.txt", 0, &mut buf).expect("read");
    assert_eq!(&buf[..n], b"seq=2");
}

#[test]
fn test_inline_metadata_write_cut_read_only_reopen() {
    let disk = MemDisk::new(1024);
    let mut fs = CellosFs::format(disk, 1024).expect("format");
    fs.create_file("/audit.log").expect("create");
    fs.write_file("/audit.log", 0, b"commit-1").expect("write");
    fs.sync().expect("sync 1");
    assert_eq!(fs.superblock().sequence, 2);

    let mut disk = fs.into_disk();
    let current_writes = disk.write_count();

    // Set power cut to trigger on the very next write (simulating crash mid-commit)
    disk.set_power_cut(current_writes + 1);

    // Try to open and write on the disk with power cut limit
    let mut crashed_fs = CellosFs::open(disk.clone()).expect("open");
    crashed_fs.write_file("/audit.log", 0, b"commit-2-torn-write").unwrap();
    let sync_result = crashed_fs.sync();
    assert!(sync_result.is_err()); // Crashed!
    drop(crashed_fs);

    // Now remount after crash (power-cut limit removed)
    disk.set_power_cut(u64::MAX);
    let mut recovered = CellosFs::open(disk).expect("recovery open");

    // This particular inline-only cut observes sequence 2; it is not a preservation guarantee.
    assert_eq!(recovered.superblock().sequence, 2);
    let mut buf = [0u8; 16];
    let n = recovered
        .read_file("/audit.log", 0, &mut buf)
        .expect("read");
    assert_eq!(&buf[..n], b"commit-1");
}

#[test]
fn test_rename_atomic() {
    let disk = MemDisk::new(1024);
    let mut fs = CellosFs::format(disk, 1024).expect("format");
    fs.create_file("/old.txt").expect("create old");
    fs.write_file("/old.txt", 0, b"rename-content")
        .expect("write old");
    fs.sync().expect("sync");

    fs.rename("/old.txt", "/new.txt").expect("rename");
    fs.sync().expect("sync after rename");

    assert!(fs.lookup("/old.txt").is_err());
    let mut buf = [0u8; 32];
    let n = fs.read_file("/new.txt", 0, &mut buf).expect("read new");
    assert_eq!(&buf[..n], b"rename-content");
}

#[test]
fn test_posix_mkdir_repro() {
    let disk = MemDisk::new(1024);
    let mut fs = CellosFs::format(disk, 1024).expect("format");
    fs.create_dir("/posix_mkdir_rmdir_nonempty").expect("mkdir");
    fs.create_file("/posix_mkdir_rmdir_nonempty/child")
        .expect("create child");
    fs.write_file("/posix_mkdir_rmdir_nonempty/child", 0, b"data")
        .expect("write child");
    fs.sync().expect("sync");
    let duplicate_res = fs.create_dir("/posix_mkdir_rmdir_nonempty");
    assert_eq!(duplicate_res.err(), Some(FsError::AlreadyExists));
}

#[test]
fn replacement_truncates_and_positional_writes_preserve_multiblock_data() {
    let mut fs = CellosFs::format(MemDisk::new(1024), 1024).unwrap();
    fs.create_file("/data").unwrap();
    fs.replace_file("/data", b"inline").unwrap();
    let payload: Vec<u8> = (0..40 * 4096 + 731).map(|i| (i % 251) as u8).collect();
    assert_eq!(fs.replace_file("/data", &payload).unwrap(), payload.len());
    assert_eq!(fs.lookup("/data").unwrap().extent_count, 1);
    let mut expected = payload;
    let patch = vec![0xA5; 2 * 4096 + 19];
    assert_eq!(fs.write_file("/data", 4091, &patch).unwrap(), patch.len());
    expected[4091..4091 + patch.len()].copy_from_slice(&patch);
    fs.write_file("/data", 1, b"short").unwrap();
    expected[1..6].copy_from_slice(b"short");
    assert!(!fs.lookup("/data").unwrap().is_inline());
    fs.sync().unwrap();
    let mut fs = CellosFs::open(fs.into_disk()).unwrap();
    let mut result = vec![0; expected.len()];
    assert_eq!(fs.read_file("/data", 0, &mut result).unwrap(), expected.len());
    assert_eq!(result, expected);
    let mut slice = vec![0; 9000];
    assert_eq!(fs.read_file("/data", 4089, &mut slice).unwrap(), slice.len());
    assert_eq!(slice, expected[4089..4089 + slice.len()]);
    let shorter = vec![0x33; 4096 + 7];
    fs.replace_file("/data", &shorter).unwrap();
    assert_eq!(fs.lookup("/data").unwrap().size, shorter.len() as u64);
    fs.replace_file("/data", b"tiny").unwrap();
    let inode = fs.lookup("/data").unwrap();
    assert!(inode.is_inline());
    assert_eq!(inode.extent_count, 0);
    assert!(inode.payload[4..].iter().all(|&b| b == 0));
    fs.replace_file("/data", b"").unwrap();
    fs.sync().unwrap();
    let mut fs = CellosFs::open(fs.into_disk()).unwrap();
    assert_eq!(fs.lookup("/data").unwrap().size, 0);
    assert_eq!(fs.read_file("/data", 0, &mut result).unwrap(), 0);
}

#[test]
fn positional_inline_transition_and_reused_blocks_zero_fill_gaps() {
    let mut fs = CellosFs::format(MemDisk::new(128), 128).unwrap();
    fs.create_file("/old").unwrap();
    fs.replace_file("/old", &vec![0xCC; 3 * 4096]).unwrap();
    fs.unlink("/old").unwrap();
    fs.create_file("/new").unwrap();
    fs.write_file("/new", 0, b"prefix").unwrap();
    fs.write_file("/new", 9001, b"tail").unwrap();
    let mut result = vec![0xFF; 9005];
    assert_eq!(fs.read_file("/new", 0, &mut result).unwrap(), result.len());
    assert_eq!(&result[..6], b"prefix");
    assert!(result[6..9001].iter().all(|&b| b == 0));
    assert_eq!(&result[9001..], b"tail");
    fs.replace_file("/new", b"x").unwrap();
    fs.write_file("/new", 10, b"z").unwrap();
    let mut inline = [0xFF; 11];
    assert_eq!(fs.read_file("/new", 0, &mut inline).unwrap(), 11);
    assert_eq!(inline, [b'x', 0, 0, 0, 0, 0, 0, 0, 0, 0, b'z']);
    fs.write_file("/new", 999, b"").unwrap();
    assert_eq!(fs.lookup("/new").unwrap().size, 11);
}

#[test]
fn creation_and_mapping_limits_refuse_without_counter_or_disk_changes() {
    for scenario in 0..4 {
        let disk = MemDisk::new(128);
        let observer = disk.clone();
        let mut fs = CellosFs::format(disk, 128).unwrap();
        if scenario != 0 {
            for i in 0..7 {
                fs.create_file(&format!("/f{i}")).unwrap();
            }
        }
        let next = fs.superblock().next_inode_num;
        let writes = observer.write_count();
        match scenario {
            0 => assert_eq!(fs.create_file(&format!("/{}", "x".repeat(47))).err(), Some(FsError::InvalidName)),
            1 => assert_eq!(fs.create_file("/full").err(), Some(FsError::NoSpace)),
            2 => assert_eq!(fs.write_file("/f0", u64::MAX, b"x"), Err(FsError::InvalidOffset)),
            _ => assert_eq!(fs.write_file("/f0", u32::MAX as u64 * 4096, b"x"), Err(FsError::InvalidOffset)),
        }
        assert_eq!(fs.superblock().next_inode_num, next);
        assert_eq!(observer.write_count(), writes);
    }
}

#[test]
fn rename_refuses_duplicates_and_normalizes_parent_identity() {
    for duplicate in [true, false] {
        let mut fs = CellosFs::format(MemDisk::new(128), 128).unwrap();
        fs.create_dir("/dir").unwrap();
        fs.create_file("/dir/old").unwrap();
        fs.create_file("/dir/taken").unwrap();
        if duplicate {
            assert_eq!(fs.rename("/dir/old", "/dir//taken"), Err(FsError::AlreadyExists));
        } else {
            fs.rename("/dir/old", "/dir//new").unwrap();
            assert_eq!(fs.lookup("/dir/old").err(), Some(FsError::NotFound));
            assert!(fs.lookup("/dir/new").is_ok());
            assert_eq!(fs.list_dir("/dir").unwrap().len(), 2);
        }
    }
}

#[test]
fn allocator_rejects_reserved_and_duplicate_releases() {
    let mut allocator = cellos_fs::allocator::BitmapAllocator::new(2, 1, 128).unwrap();
    assert_eq!(allocator.free_block(2), Err(FsError::Corrupted));
    assert_eq!(allocator.free_block(128), Err(FsError::PartitionOverflow));
    let block = allocator.allocate_block().unwrap();
    allocator.free_block(block).unwrap();
    assert_eq!(allocator.free_block(block), Err(FsError::Corrupted));
    assert!(cellos_fs::allocator::BitmapAllocator::new(2, 0, 128).is_err());
}

#[cfg(not(feature = "g2-cow-experiment"))]
#[test]
fn fragmented_mapping_capacity_refuses_instead_of_omitting_extents() {
    let disk = MemDisk::new(256);
    let observer = disk.clone();
    let mut fs = CellosFs::format(disk, 256).unwrap();
    fs.create_file("/a").unwrap();
    fs.create_file("/b").unwrap();
    for block in 0..26 {
        fs.write_file("/a", block * 4096, &[0xAA; 4096]).unwrap();
        fs.write_file("/b", block * 4096, &[0xBB; 4096]).unwrap();
    }
    assert_eq!(fs.lookup("/a").unwrap().extent_count, 26);
    let writes = observer.write_count();
    assert_eq!(fs.write_file("/a", 26 * 4096, &[0xAA; 4096]), Err(FsError::NoSpace));
    assert_eq!(observer.write_count(), writes);
    // Failed engine entry ends this disposable instance; no retry or sync.
}

#[cfg(feature = "g2-cow-experiment")]
#[test]
fn fragmented_mapping_capacity_refuses_instead_of_omitting_extents() {
    use cellos_fs::{BlockDevice, DirEntryRecord, InodeRecord, Superblock, BLOCK_SIZE,
        INODE_MODE_DIR, INODE_MODE_FILE, INODE_SIZE};
    for blocks in [26, 27] {
        // Fresh offline V1 fixture: 32 valid inode tables alternate with free
        // blocks. Empty table slots are legal and still owned by the root.
        // After reserving the changed table, >=27 isolated data blocks remain.
        let mut disk = MemDisk::new(66);
        let mut sb = Superblock::new(66, 1);
        sb.next_inode_num = 3;
        sb.free_block_count = 31;
        for slot in 1..32 { sb.inode_blocks[slot] = 3 + 2 * slot as u64; }
        sb.update_checksum();
        let mut root = InodeRecord::new(1, INODE_MODE_DIR);
        let entry = DirEntryRecord::new(2, 1, "a").unwrap();
        root.payload[..DirEntryRecord::SIZE].copy_from_slice(&entry.to_bytes());
        root.extent_count = 1;
        root.update_checksum();
        let file = InodeRecord::new(2, INODE_MODE_FILE);
        let mut table = [0; BLOCK_SIZE];
        table[..INODE_SIZE].copy_from_slice(&root.to_bytes());
        table[INODE_SIZE..2 * INODE_SIZE].copy_from_slice(&file.to_bytes());
        disk.write_block(3, &table).unwrap();
        disk.write_block(0, &sb.to_bytes()).unwrap();
        disk.write_block(2, &[0xff; BLOCK_SIZE]).unwrap();
        let observer = disk.clone();
        let mut fs = CellosFs::open(disk).unwrap();
        assert_eq!(fs.free_blocks(), 31);
        let data = vec![0xAA; blocks * BLOCK_SIZE];
        let writes = observer.write_count();
        if blocks == 27 {
            assert_eq!(fs.replace_file("/a", &data), Err(FsError::NoSpace));
            assert_eq!(observer.write_count(), writes);
            // Refusal closes the instance; no sync or mutation retry.
        } else {
            fs.replace_file("/a", &data).unwrap();
            assert_eq!(fs.lookup("/a").unwrap().extent_count, 26);
            fs.sync().unwrap();
            let mut observed = CellosFs::open(fs.into_disk()).unwrap();
            let mut actual = vec![0; data.len()];
            assert_eq!(observed.read_file("/a", 0, &mut actual).unwrap(), data.len());
            assert_eq!(actual, data);
        }
    }
}

#[cfg(feature = "g2-cow-experiment")]
#[test]
fn published_free_count_matches_displacement_and_reopen() {
    use cellos_fs::BLOCK_SIZE;
    let mut fs = CellosFs::format(MemDisk::new(64), 64).unwrap();
    fs.create_file("/file").unwrap();
    fs.replace_file("/file", &vec![0x11; 3 * BLOCK_SIZE]).unwrap();
    fs.sync().unwrap();
    // Union: three fixed reserved blocks, bootstrap table, candidate table,
    // and three candidate data blocks.
    assert_eq!(fs.free_blocks(), 56);
    assert_eq!(fs.superblock().free_block_count, 56);

    fs.replace_file("/file", &vec![0x22; 2 * BLOCK_SIZE]).unwrap();
    fs.sync().unwrap();
    // Bootstrap table was displaced: two tables and five data blocks remain.
    assert_eq!(fs.free_blocks(), 54);
    assert_eq!(fs.superblock().free_block_count, 54);

    // A three-block unpublished candidate is superseded by inline content.
    // Its reservation must be retained before FS2, then excluded from the
    // published count along with the displaced oldest table/data graph.
    fs.replace_file("/file", &vec![0x33; 3 * BLOCK_SIZE]).unwrap();
    fs.replace_file("/file", b"final").unwrap();
    assert_eq!(fs.free_blocks(), 50);
    fs.sync().unwrap();
    assert_eq!(fs.free_blocks(), 57);
    assert_eq!(fs.superblock().free_block_count, 57);
    let mut observed = CellosFs::open(fs.into_disk()).unwrap();
    assert_eq!(observed.free_blocks(), 57);
    assert_eq!(observed.superblock().free_block_count, 57);
    let mut data = [0; 5];
    assert_eq!(observed.read_file("/file", 0, &mut data).unwrap(), 5);
    assert_eq!(&data, b"final");
}
