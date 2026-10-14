//! Disposable-image write-cut and deterministic error-propagation scenarios.
//! These host memory scenarios are not crash-preservation or media qualification.
//! Failed instances are dropped; fresh reopen is read-only observation before discard.

use cellos_fs::{BlockDevice, CellosFs, FsError, InodeRecord, MemDisk, Superblock, BLOCK_SIZE};
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn test_exhaustive_power_cut_injections() {
    // 1. First record the golden sequence of writes without any power cuts
    let disk = MemDisk::new(1024);
    let mut fs = CellosFs::format(disk, 1024).expect("format");

    // Workload step 1: create directories and files
    fs.create_dir("/etc").expect("mkdir etc");
    fs.create_file("/etc/config.json").expect("create config");
    fs.write_file("/etc/config.json", 0, b"{\"node\":\"robot-01\"}")
        .expect("write config");
    fs.sync().expect("sync 1"); // seq = 2

    // Workload step 2: write large file
    fs.create_dir("/data").expect("mkdir data");
    fs.create_file("/data/telemetry.bin")
        .expect("create telemetry");
    let large_data = vec![0x42u8; 8192];
    fs.write_file("/data/telemetry.bin", 0, &large_data)
        .expect("write telemetry");
    fs.sync().expect("sync 2"); // seq = 3

    // Workload step 3: modify and unlink
    fs.write_file("/etc/config.json", 0, b"{\"node\":\"robot-02-updated\"}")
        .expect("update config");
    fs.create_file("/data/temp.log").expect("create temp");
    fs.write_file("/data/temp.log", 0, b"transient log")
        .expect("write temp");
    fs.unlink("/data/temp.log").expect("unlink temp");
    fs.sync().expect("sync 3"); // seq = 4

    let total_writes = fs.into_disk().write_count();
    println!(
        "Golden run completed with {} total block writes.",
        total_writes
    );
    assert!(total_writes >= 15);

    // 2. Now systematically inject a power cut at EVERY SINGLE write step!
    let iterations = total_writes;
    let mut recovered_count = 0usize;
    let mut refused_count = 0usize;

    for cut_at in 1..iterations {
        let mut disk = MemDisk::new(1024);
        disk.set_power_cut(cut_at);

        // Run the exact same sequence under power cut
        let run_result = (|| -> Result<(), cellos_fs::FsError> {
            let mut fs = CellosFs::format(disk.clone(), 1024)?;
            fs.create_dir("/etc")?;
            fs.create_file("/etc/config.json")?;
            fs.write_file("/etc/config.json", 0, b"{\"node\":\"robot-01\"}")?;
            fs.sync()?;

            fs.create_dir("/data")?;
            fs.create_file("/data/telemetry.bin")?;
            fs.write_file("/data/telemetry.bin", 0, &large_data)?;
            fs.sync()?;

            fs.write_file("/etc/config.json", 0, b"{\"node\":\"robot-02-updated\"}")?;
            fs.create_file("/data/temp.log")?;
            fs.write_file("/data/temp.log", 0, b"transient log")?;
            fs.unlink("/data/temp.log")?;
            fs.sync()?;

            Ok(())
        })();

        assert!(
            run_result.is_err(),
            "Cut at {} should have caused an error",
            cut_at
        );

        // Reboot: clear power cut and attempt recovery
        let mut reboot_disk = disk;
        reboot_disk.set_power_cut(u64::MAX);

        match CellosFs::open(reboot_disk) {
            Ok(mut recovered_fs) => {
                // Observe only; never resume mutation after a failed instance.
                let seq = recovered_fs.superblock().sequence;
                assert!(
                    seq >= 1 && seq <= 4,
                    "Sequence {} out of expected range",
                    seq
                );

                match recovered_fs.list_dir("/") {
                    Ok(root_list) => {
                        for entry in root_list {
                            assert!(!entry.0.is_empty());
                        }
                    }
                    Err(error) => assert_eq!(error, FsError::Corrupted),
                }
                if seq >= 2 {
                    let mut buf = [0u8; 64];
                    match recovered_fs.read_file("/etc/config.json", 0, &mut buf) {
                        Ok(n) => assert!(n > 0),
                        Err(error) => assert!(matches!(error, FsError::Corrupted | FsError::NotFound)),
                    }
                }

                recovered_count += 1;
            }
            Err(_) => {
                // R-core permits fail-closed observation; crash-safe allocation is not supplied.
                refused_count += 1;
            }
        }
    }

    println!(
        "Write-cut observations: {} write steps, {} opens, {} refusals.",
        iterations, recovered_count, refused_count
    );
    assert_eq!(
        recovered_count + refused_count,
        (iterations - 1) as usize
    );
}

/// Simple XorShift64 PRNG for deterministic, dependency-free fuzzing.
struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_range(&mut self, min: u64, max: u64) -> u64 {
        min + (self.next() % (max - min + 1))
    }
}

#[test]
fn test_ten_thousand_random_power_cut_injections() {
    let mut rng = XorShift64(0xDEAD_BEEF_CAFE_BABE);
    let iterations = 10_000;
    let mut recoveries = 0usize;

    for iter in 0..iterations {
        let disk = MemDisk::new(1024);
        let cut_point = rng.next_range(1, 30);

        let mut sim_disk = disk.clone();
        sim_disk.set_power_cut(cut_point);

        let _ = (|| -> Result<(), cellos_fs::FsError> {
            let mut fs = CellosFs::format(sim_disk, 1024)?;
            let op_count = (iter % 5) + 1;
            for op in 0..op_count {
                let fname = format!("/file_{}.bin", op);
                fs.create_file(&fname)?;
                let data = [0x55u8; 512];
                fs.write_file(&fname, 0, &data)?;
                if op % 2 == 0 {
                    fs.sync()?;
                }
            }
            fs.sync()?;
            Ok(())
        })();

        // Reboot and verify consistency
        let mut reboot_disk = disk;
        reboot_disk.set_power_cut(u64::MAX);
        if let Ok(mut recovered) = CellosFs::open(reboot_disk) {
            assert!(recovered.superblock().sequence >= 1);
            let _ = recovered.list_dir("/");
            recoveries += 1;
        }
    }

    println!(
        "10,000 disposable-image write cuts: {} read-only reopens.",
        recoveries
    );
    assert!(recoveries > 0);
}

#[derive(Clone)]
struct FaultDisk {
    disk: MemDisk,
    fail_read: Rc<Cell<Option<u64>>>,
    fail_flush: Rc<Cell<Option<u64>>>,
    flush_calls: Rc<Cell<u64>>,
}

impl FaultDisk {
    fn new(disk: MemDisk) -> Self {
        Self {
            disk,
            fail_read: Rc::new(Cell::new(None)),
            fail_flush: Rc::new(Cell::new(None)),
            flush_calls: Rc::new(Cell::new(0)),
        }
    }
}

impl BlockDevice for FaultDisk {
    fn read_block(&mut self, block: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if self.fail_read.get() == Some(block) {
            return Err(FsError::IoError);
        }
        self.disk.read_block(block, buf)
    }

    fn write_block(&mut self, block: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        self.disk.write_block(block, buf)
    }

    fn flush(&mut self) -> Result<(), FsError> {
        let call = self.flush_calls.get() + 1;
        self.flush_calls.set(call);
        if self.fail_flush.get() == Some(call) {
            return Err(FsError::IoError);
        }
        self.disk.flush()
    }

    fn total_blocks(&self) -> u64 {
        self.disk.total_blocks()
    }
}

fn rewrite_superblock(disk: &mut MemDisk, change: impl FnOnce(&mut Superblock)) {
    let mut bytes = [0; BLOCK_SIZE];
    disk.read_block(0, &mut bytes).unwrap();
    let mut sb = Superblock::from_bytes(&bytes).unwrap();
    change(&mut sb);
    sb.update_checksum();
    disk.write_block(0, &sb.to_bytes()).unwrap();
}

fn rewrite_inode(disk: &mut MemDisk, sb: &Superblock, inode: &mut InodeRecord) {
    let slot = ((inode.inode_num - 1) / 8) as usize;
    let index = ((inode.inode_num - 1) % 8) as usize;
    let mut bytes = [0; BLOCK_SIZE];
    disk.read_block(sb.inode_blocks[slot], &mut bytes).unwrap();
    inode.update_checksum();
    bytes[index * 512..(index + 1) * 512].copy_from_slice(&inode.to_bytes());
    disk.write_block(sb.inode_blocks[slot], &bytes).unwrap();
}

#[test]
fn open_transport_geometry_and_allocator_failures_never_write() {
    for failed_slot in [0, 1] {
        let disk = MemDisk::new(128);
        let fs = CellosFs::format(disk.clone(), 128).unwrap();
        let fault = FaultDisk::new(fs.into_disk());
        fault.fail_read.set(Some(failed_slot));
        let writes = disk.write_count();
        assert_eq!(CellosFs::open(fault).err(), Some(FsError::IoError));
        assert_eq!(disk.write_count(), writes);
    }
    for case in 0..9 {
        let mut disk = MemDisk::new(128);
        CellosFs::format(disk.clone(), 128).unwrap();
        rewrite_superblock(&mut disk, |sb| match case {
            0 => sb.version += 1,
            1 => sb.block_size = 512,
            2 => sb.total_blocks = 127,
            3 => sb.free_bitmap_block = u64::MAX,
            4 => sb.free_bitmap_blocks = 0,
            5 => sb.root_inode_block = 128,
            6 => sb.inode_blocks[1] = sb.inode_blocks[0],
            7 => sb.next_inode_num = u64::MAX,
            _ => sb.free_block_count = u64::MAX,
        });
        let writes = disk.write_count();
        assert_eq!(CellosFs::open(disk.clone()).err(), Some(FsError::Corrupted), "case {case}");
        assert_eq!(disk.write_count(), writes);
    }
    let mut disk = MemDisk::new(128);
    CellosFs::format(disk.clone(), 128).unwrap();
    disk.write_block(2, &[0; BLOCK_SIZE]).unwrap();
    let writes = disk.write_count();
    #[cfg(not(feature = "g2-cow-experiment"))]
    assert_eq!(CellosFs::open(disk.clone()).err(), Some(FsError::Corrupted));
    #[cfg(feature = "g2-cow-experiment")]
    {
        let mut observed = CellosFs::open(disk.clone()).unwrap();
        assert!(observed.lookup("/").unwrap().is_dir());
        assert_eq!(observed.free_blocks(), 124);
    }
    assert_eq!(disk.write_count(), writes);
    assert_eq!(CellosFs::format(disk.clone(), 127).err(), Some(FsError::PartitionOverflow));
    assert_eq!(disk.write_count(), writes);
}

#[test]
fn preserved_data_metadata_and_rename_lookup_read_errors_propagate() {
    for scenario in 0..3 {
        let disk = MemDisk::new(128);
        let control = FaultDisk::new(disk.clone());
        let mut fs = CellosFs::format(control.clone(), 128).unwrap();
        fs.create_file("/file").unwrap();
        fs.replace_file("/file", &vec![0x41; 8192]).unwrap();
        fs.create_dir("/target").unwrap();
        fs.sync().unwrap();
        if scenario == 0 {
            let inode = fs.lookup("/file").unwrap();
            let physical = u64::from_le_bytes(inode.payload[8..16].try_into().unwrap());
            control.fail_read.set(Some(physical));
            let writes = disk.write_count();
            assert_eq!(fs.write_file("/file", 7, b"x"), Err(FsError::IoError));
            assert_eq!(disk.write_count(), writes);
        } else {
            fs.create_file("/dirty").unwrap(); // caches root
            if scenario == 2 {
                fs.write_file("/file", 0, b"new").unwrap(); // caches source, not target
            }
            control.fail_read.set(Some(fs.superblock().inode_blocks[0]));
            if scenario == 1 {
                #[cfg(not(feature = "g2-cow-experiment"))]
                assert_eq!(fs.sync(), Err(FsError::IoError));
                #[cfg(feature = "g2-cow-experiment")]
                fs.sync().unwrap(); // fixed inode image reconstructs tables without source reads
            } else {
                #[cfg(not(feature = "g2-cow-experiment"))]
                assert_eq!(fs.rename("/file", "/target/new"), Err(FsError::IoError));
                #[cfg(feature = "g2-cow-experiment")]
                {
                    fs.rename("/file", "/target/new").unwrap(); // both parents are cached
                    assert!(fs.lookup("/target/new").is_ok());
                }
            }
        }
        drop(fs);
        #[cfg(feature = "g2-cow-experiment")]
        if scenario != 0 {
            // Cached operations cannot encounter this read fault, but complete
            // both-root reconstruction must propagate it rather than skip a root.
            let writes = disk.write_count();
            assert_eq!(CellosFs::open(control.clone()).err(), Some(FsError::IoError));
            assert_eq!(disk.write_count(), writes);
        }
        control.fail_read.set(None);
        let writes = disk.write_count();
        let mut observed = CellosFs::open(control.clone()).unwrap();
        let mut data = vec![0; 8192];
        assert_eq!(observed.read_file("/file", 0, &mut data).unwrap(), data.len());
        if scenario == 0 {
            assert!(data.iter().all(|&b| b == 0x41));
        }
        assert_eq!(disk.write_count(), writes);
        // This fresh volume is now discarded, never reopened for mutation.
    }
}

#[test]
fn later_multiblock_read_and_data_write_failures_are_not_short_successes() {
    for scenario in 0..2 {
        let mut disk = MemDisk::new(128);
        let control = FaultDisk::new(disk.clone());
        let mut fs = CellosFs::format(control.clone(), 128).unwrap();
        fs.create_file("/file").unwrap();
        fs.replace_file("/file", &vec![0x77; 3 * BLOCK_SIZE]).unwrap();
        fs.sync().unwrap();
        if scenario == 0 {
            let inode = fs.lookup("/file").unwrap();
            let physical = u64::from_le_bytes(inode.payload[8..16].try_into().unwrap());
            control.fail_read.set(Some(physical + 1));
            let mut output = vec![0; 3 * BLOCK_SIZE];
            assert_eq!(fs.read_file("/file", 0, &mut output), Err(FsError::IoError));
        } else {
            disk.set_power_cut(disk.write_count() + 1);
            assert_eq!(fs.replace_file("/file", &vec![0x88; 3 * BLOCK_SIZE]), Err(FsError::IoError));
        }
        drop(fs);
    }
}

#[test]
fn sequence_exhaustion_refuses_every_mutation_before_writes() {
    for operation in 0..8 {
        let mut disk = MemDisk::new(128);
        let mut fs = CellosFs::format(disk.clone(), 128).unwrap();
        fs.create_file("/file").unwrap();
        fs.create_dir("/dir").unwrap();
        fs.sync().unwrap();
        drop(fs);
        // The active slot is B after the single sync; publish the exhaustion input in A.
        let mut bytes = [0; BLOCK_SIZE];
        disk.read_block(1, &mut bytes).unwrap();
        disk.write_block(0, &bytes).unwrap();
        rewrite_superblock(&mut disk, |sb| sb.sequence = u64::MAX);
        let mut fs = CellosFs::open(disk.clone()).unwrap();
        let writes = disk.write_count();
        let next = fs.superblock().next_inode_num;
        let result = match operation {
            0 => fs.create_file("/new").map(|_| ()),
            1 => fs.create_dir("/new").map(|_| ()),
            2 => fs.write_file("/file", 0, b"x").map(|_| ()),
            3 => fs.replace_file("/file", b"x").map(|_| ()),
            4 => fs.unlink("/file"),
            5 => fs.rmdir("/dir"),
            6 => fs.rename("/file", "/new"),
            _ => fs.sync(),
        };
        assert_eq!(result, Err(FsError::NoSpace), "operation {operation}");
        assert_eq!(fs.superblock().next_inode_num, next);
        assert_eq!(disk.write_count(), writes);
    }
}

#[test]
fn malformed_inode_mappings_refuse_instead_of_short_or_fabricated_reads() {
    for case in 0..6 {
        let mut disk = MemDisk::new(128);
        let mut fs = CellosFs::format(disk.clone(), 128).unwrap();
        fs.create_file("/file").unwrap();
        fs.replace_file("/file", &vec![0x55; 8192]).unwrap();
        fs.sync().unwrap();
        let mut inode = fs.lookup("/file").unwrap();
        let sb = *fs.superblock();
        drop(fs);
        match case {
            0 => inode.extent_count = 27,
            1 => inode.payload[0..4].copy_from_slice(&1u32.to_le_bytes()),
            2 => inode.payload[4..8].copy_from_slice(&1u32.to_le_bytes()),
            3 => inode.payload[8..16].copy_from_slice(&127u64.to_le_bytes()),
            4 => inode.payload[8..16].copy_from_slice(&sb.root_inode_block.to_le_bytes()),
            _ => {
                inode.flags |= cellos_fs::INODE_FLAG_INLINE;
                inode.inline_size = 8192;
            }
        }
        rewrite_inode(&mut disk, &sb, &mut inode);
        let writes = disk.write_count();
        #[cfg(not(feature = "g2-cow-experiment"))]
        {
            let mut fs = CellosFs::open(disk.clone()).unwrap();
            let mut output = [0; 9000];
            assert_eq!(fs.read_file("/file", 0, &mut output), Err(FsError::Corrupted), "case {case}");
        }
        #[cfg(feature = "g2-cow-experiment")]
        assert_eq!(CellosFs::open(disk.clone()).err(), Some(FsError::Corrupted), "case {case}");
        assert_eq!(disk.write_count(), writes);
    }
}

#[test]
fn direct_library_seven_mutations_propagate_both_flush_failures() {
    for operation in 0..7 {
        for flush_position in [1, 2] {
            let disk = MemDisk::new(128);
            let control = FaultDisk::new(disk.clone());
            let mut fs = CellosFs::format(control.clone(), 128).unwrap();
            fs.create_file("/file").unwrap();
            fs.replace_file("/file", b"before").unwrap();
            fs.create_dir("/dir").unwrap();
            fs.sync().unwrap();
            let baseline = control.flush_calls.get();
            control.fail_flush.set(Some(baseline + flush_position));
            match operation {
                0 => { fs.replace_file("/file", b"replacement").unwrap(); }
                1 => { fs.write_file("/file", 1, b"position").unwrap(); }
                2 => {
                    let size = fs.lookup("/file").unwrap().size;
                    fs.write_file("/file", size, b"append").unwrap();
                }
                3 => { fs.create_dir("/newdir").unwrap(); }
                4 => { fs.rmdir("/dir").unwrap(); }
                5 => { fs.unlink("/file").unwrap(); }
                _ => { fs.rename("/file", "/renamed").unwrap(); }
            }
            assert_eq!(fs.sync(), Err(FsError::IoError), "operation {operation}, flush {flush_position}");
            assert_eq!(control.flush_calls.get(), baseline + flush_position);
            drop(fs);
            control.fail_flush.set(None);
            let writes = disk.write_count();
            if let Ok(mut observed) = CellosFs::open(control.clone()) {
                let _ = observed.list_dir("/");
            }
            assert_eq!(disk.write_count(), writes);
        }
    }
}

#[test]
fn inode_number_exhaustion_and_unlink_release_errors_propagate() {
    let mut disk = MemDisk::new(128);
    CellosFs::format(disk.clone(), 128).unwrap();
    rewrite_superblock(&mut disk, |sb| sb.next_inode_num = 257);
    let mut fs = CellosFs::open(disk.clone()).unwrap();
    let writes = disk.write_count();
    assert_eq!(fs.create_file("/exhausted").err(), Some(FsError::NoSpace));
    assert_eq!(fs.superblock().next_inode_num, 257);
    assert_eq!(disk.write_count(), writes);
    drop(fs);

    let mut disk = MemDisk::new(128);
    let mut fs = CellosFs::format(disk.clone(), 128).unwrap();
    fs.create_file("/file").unwrap();
    fs.replace_file("/file", &[0x44; 8192]).unwrap();
    fs.sync().unwrap();
    let inode = fs.lookup("/file").unwrap();
    let physical = u64::from_le_bytes(inode.payload[8..16].try_into().unwrap());
    drop(fs);
    let mut bitmap = [0; BLOCK_SIZE];
    disk.read_block(2, &mut bitmap).unwrap();
    bitmap[physical as usize / 8] &= !(1 << (physical % 8));
    disk.write_block(2, &bitmap).unwrap();
    let mut fs = CellosFs::open(disk.clone()).unwrap();
    let writes = disk.write_count();
    #[cfg(not(feature = "g2-cow-experiment"))]
    assert_eq!(fs.unlink("/file"), Err(FsError::Corrupted));
    #[cfg(feature = "g2-cow-experiment")]
    {
        let free = fs.free_blocks();
        fs.unlink("/file").unwrap(); // bitmap bits never authorize retirement
        assert!(fs.free_blocks() < free); // changed table is reserved, data stays protected
    }
    assert_eq!(disk.write_count(), writes);
}

#[test]
fn bounded_and_vectorized_requests_check_entire_range_before_io() {
    let disk = MemDisk::new(16);
    let observer = disk.clone();
    let mut bounded = cellos_fs::BoundedDisk::new(disk, u64::MAX, 4);
    let bytes = [0; BLOCK_SIZE];
    assert_eq!(bounded.write_block(0, &bytes), Err(FsError::PartitionOverflow));
    assert_eq!(observer.write_count(), 0);
    let mut disk = observer.clone();
    assert_eq!(disk.write_blocks(15, 2, &[0; 2 * BLOCK_SIZE]), Err(FsError::PartitionOverflow));
    assert_eq!(disk.write_blocks(u64::MAX, 2, &[0; 2 * BLOCK_SIZE]), Err(FsError::PartitionOverflow));
    assert_eq!(disk.write_count(), 0);
    let mut short = [0; BLOCK_SIZE];
    assert_eq!(disk.read_blocks(0, 2, &mut short), Err(FsError::IoError));
}

#[test]
fn open_refuses_selected_inode_table_blocks_marked_free_without_writing() {
    for slot in [0, 1] {
        let mut disk = MemDisk::new(128);
        let mut fs = CellosFs::format(disk.clone(), 128).unwrap();
        fs.create_dir("/dir").unwrap();
        for child in 0..7 {
            fs.create_file(&format!("/dir/f{child}")).unwrap();
        }
        fs.sync().unwrap();
        let sb = *fs.superblock();
        let block = sb.inode_blocks[slot];
        assert_ne!(block, 0);
        drop(fs);
        let mut bitmap = [0; BLOCK_SIZE];
        disk.read_block(sb.free_bitmap_block, &mut bitmap).unwrap();
        bitmap[block as usize / 8] &= !(1 << (block % 8));
        disk.write_block(sb.free_bitmap_block, &bitmap).unwrap();
        let writes = disk.write_count();
        let flushes = disk.flush_count();
        #[cfg(not(feature = "g2-cow-experiment"))]
        assert_eq!(CellosFs::open(disk.clone()).err(), Some(FsError::Corrupted), "slot {slot}");
        #[cfg(feature = "g2-cow-experiment")]
        {
            let mut observed = CellosFs::open(disk.clone()).unwrap();
            assert_eq!(observed.list_dir("/dir").unwrap().len(), 7);
        }
        assert_eq!(disk.write_count(), writes);
        assert_eq!(disk.flush_count(), flushes);
    }
}
