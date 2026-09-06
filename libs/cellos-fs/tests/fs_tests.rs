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
fn test_power_cut_clean_rollback() {
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
    let _ = crashed_fs.write_file("/audit.log", 0, b"commit-2-torn-write");
    let sync_result = crashed_fs.sync();
    assert!(sync_result.is_err()); // Crashed!

    // Now remount after crash (power-cut limit removed)
    disk.set_power_cut(u64::MAX);
    let mut recovered = CellosFs::open(disk).expect("recovery open");

    // Must have rolled back cleanly to sequence 2!
    assert_eq!(recovered.superblock().sequence, 2);
    let mut buf = [0u8; 16];
    let n = recovered
        .read_file("/audit.log", 0, &mut buf)
        .expect("read");
    assert_eq!(&buf[..n], b"commit-1"); // Clean, untorn state preserved!
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
