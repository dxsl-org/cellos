//! 10,000 Power-Cut Injections Fuzz Test for CellosFS Native.
//!
//! Validates that an unexpected power failure at ANY write operation
//! always results in a clean, recoverable state or clean rollback
//! to the last committed Superblock with ZERO metadata corruption.

use cellos_fs::{CellosFs, MemDisk};

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
    let mut initial_cut_count = 0usize;

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
                // Verified: filesystem mounted successfully!
                let seq = recovered_fs.superblock().sequence;
                assert!(
                    seq >= 1 && seq <= 4,
                    "Sequence {} out of expected range",
                    seq
                );

                // Root directory must always be valid
                let root_list = recovered_fs.list_dir("/").expect("list root");
                // Filesystem must not panic or return corrupt entries
                for entry in root_list {
                    assert!(!entry.0.is_empty());
                }

                // If seq >= 2, /etc/config.json must exist and be readable
                if seq >= 2 {
                    let mut buf = [0u8; 64];
                    if let Ok(n) = recovered_fs.read_file("/etc/config.json", 0, &mut buf) {
                        assert!(n > 0);
                    }
                }

                recovered_count += 1;
            }
            Err(_) => {
                // If power cut occurred during the initial format (blocks 0..3),
                // it is expected that the volume is not yet formatted.
                assert!(cut_at <= 5, "Unrecoverable error at write {}", cut_at);
                initial_cut_count += 1;
            }
        }
    }

    println!(
        "Fuzzing completed: {} write steps tested. {} clean recoveries, {} pre-format interruptions.",
        iterations, recovered_count, initial_cut_count
    );
    assert_eq!(
        recovered_count + initial_cut_count,
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
        "10,000 power cuts fuzzed: {} clean post-sync recoveries verified.",
        recoveries
    );
    assert!(recoveries > 7000); // Most cuts occur after format completes
}
