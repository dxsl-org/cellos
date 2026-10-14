//! CellosFS Native /srv integration tests.
//!
//! Preprovisioned first boot, existing-volume reboot, and open-only refusal:
//!
//! 1. `riscv64_cellosfs_srv_basic` — single boot with a P5-formatted disk;
//!    waits for all six srv-test scenarios, then runs the POSIX rename smoke.
//!
//! 2. `riscv64_cellosfs_srv_degrade_no_disk` — boot with no VirtIO-BLK; confirms
//!    the VFS service warns and degrades gracefully instead of panicking.
//!
//! 3. `riscv64_cellosfs_srv_persistence` — two sequential boots against the same
//!    temp disk; the srv-test cell writes a persist marker in boot 1 and the
//!    harness verifies it is announced as found in boot 2.
//!
//! 4–5. Blank/corrupt attached P5 volumes refuse real Stat/write scenarios and
//!    retain every selected partition byte; no runtime provisioning is permitted.
//!
//! Prerequisites:
//!   scripts/build-srv-test-ci.sh  →  target/.../cellos-kernel-srv-test
//!   scripts/mksrv-img.sh          →  build/disk_srv.img
//! Test-only artifact overrides: CELLOS_SRV_TEST_KERNEL and CELLOS_SRV_TEST_DISK.
//! The no-disk lane uses the same selected srv-test kernel without attaching a disk.
//!
//! Run:
//!   cargo test --manifest-path tests/integration/Cargo.toml --test srv-cellosfs

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn srv_test_kernel() -> String {
    if let Ok(path) = std::env::var("CELLOS_SRV_TEST_KERNEL") {
        return path;
    }
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-srv-test")
        .to_string_lossy()
        .into_owned()
}

fn srv_disk() -> String {
    if let Ok(path) = std::env::var("CELLOS_SRV_TEST_DISK") {
        return path;
    }
    repo_root()
        .join("build/disk_srv.img")
        .to_string_lossy()
        .into_owned()
}

fn retain_serial(name: &str, serial: &str) {
    let Some(directory) = std::env::var_os("CELLOS_G2_EVIDENCE_DIR") else { return };
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).expect("create evidence directory");
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true)
        .open(directory.join(format!("{name}.raw.log"))).expect("exclusive raw evidence");
    // QemuRunner stores each incoming byte as one Latin-1 code point.
    // Reverse that projection rather than UTF-8-encoding a mojibake string.
    let bytes: Vec<u8> = serial.chars()
        .map(|c| u8::try_from(c as u32).expect("serial byte projection")).collect();
    std::io::Write::write_all(&mut file, &bytes).expect("retain raw serial");
    file.sync_all().expect("sync raw evidence");
}

fn qemu_ok() -> bool {
    std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok()
}

fn prerequisites_ok_with_disk() -> bool {
    let kernel = PathBuf::from(srv_test_kernel());
    let disk = PathBuf::from(srv_disk());
    if !kernel.exists() {
        eprintln!(
            "SKIP: srv-test kernel not found ({}). Run scripts/build-srv-test-ci.sh first.",
            srv_test_kernel()
        );
    }
    if !disk.exists() {
        eprintln!(
            "SKIP: disk_srv.img not found ({}). Run scripts/mksrv-img.sh first.",
            srv_disk()
        );
    }
    if !qemu_ok() {
        eprintln!("SKIP: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel.exists() && disk.exists() && qemu_ok())
}

/// S1–S6: mount, write+read, listdir, mkdir, unlink, atomic rename; then live
/// POSIX mkdir/rmdir and rename.
/// The test creates a temp copy of the base disk image so repeated runs do not
/// accumulate state in `build/disk_srv.img`.
#[test]
fn riscv64_cellosfs_srv_basic() {
    if !prerequisites_ok_with_disk() {
        return;
    }

    // Fresh temp copy — writes by the cell do not pollute the base image.
    let tmp = tempfile::Builder::new()
        .suffix(".img")
        .tempfile()
        .expect("create temp disk");
    std::fs::copy(srv_disk(), tmp.path()).expect("copy srv disk");

    let mut runner =
        QemuRunner::boot_rv64_with_disk(&srv_test_kernel(), tmp.path().to_str().unwrap());

    runner
        .wait_for("[srv-test] ALL TESTS PASSED", 120)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });

    // Exercise live POSIX directory lifecycle and rename on the CellosFS P5 (/srv)
    // via posix-shim-test.
    std::thread::sleep(std::time::Duration::from_millis(500));
    runner.send_line("posix-shim-test");
    runner
        .wait_for("[posix-shim] POSIX-MKDIR-RMDIR: OK", 60)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });
    runner
        .wait_for("[posix-shim] POSIX-RENAME: OK", 60)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });
    runner
        .wait_for("[posix-shim] PORTING-SMOKE: OK", 60)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });
    let serial = runner.dump();
    retain_serial("basic", &serial);
    assert_eq!(
        serial
            .matches(
                "[selftest] IPC-PENDING: PASS (deferred, bounded, quota-safe, completion-wake)"
            )
            .count(),
        1,
        "IPC-PENDING marker must appear exactly once\n--- output ---\n{serial}"
    );
    assert_eq!(
        serial.matches("[posix-shim] RAW-RENAME: OK").count(),
        1,
        "RAW-RENAME marker must appear exactly once\n--- output ---\n{serial}"
    );
    assert_eq!(
        serial.matches("[posix-shim] POSIX-RENAME: OK").count(),
        1,
        "POSIX-RENAME marker must appear exactly once\n--- output ---\n{serial}"
    );
    assert_eq!(
        serial.matches("[posix-shim] POSIX-MKDIR-RMDIR: OK").count(),
        1,
        "POSIX-MKDIR-RMDIR marker must appear exactly once\n--- output ---\n{serial}"
    );
    assert!(
        !serial.contains("[selftest] IPC-PENDING: FAIL")
            && !serial.contains("RAW-RENAME: FAIL")
            && !serial.contains("POSIX-MKDIR-RMDIR: FAIL")
            && !serial.contains("POSIX-RENAME: FAIL")
            && !serial.contains("[KERNEL PANIC]")
            && !serial.contains("panicked at")
            && !serial.contains("[fault] Cell ")
            && !serial.contains("Load access fault")
            && !serial.contains("Store/AMO access fault")
            && !serial.contains("Instruction access fault"),
        "POSIX directory lifecycle or rename reported a failure, panic, or cell fault\n--- output ---\n{serial}"
    );
}

/// S6: boot with no VirtIO-BLK → VFS must warn that /srv is unavailable but
/// must NOT panic. Uses the same current srv-test kernel as the attached-disk lanes.
#[test]
fn riscv64_cellosfs_srv_degrade_no_disk() {
    let kernel = PathBuf::from(srv_test_kernel());
    if !vicell_integration_tests::ci_guard(kernel.exists() && qemu_ok()) {
        return;
    }

    // No block device: open-only native volumes stay unavailable, with no fallback.
    let runner = QemuRunner::boot_rv64(kernel.to_str().unwrap());
    runner
        .wait_for("[srv-test] Results: 0 PASS, 6 FAIL", 120)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });
    retain_serial("no-disk", &runner.dump());
}

/// S7: write persist marker in boot 1, kill QEMU, boot 2 with same image →
/// confirm the marker is detected by the cell.
///
/// Both boots share one `NamedTempFile` for the disk image.  `boot_rv64_with_disk`
/// does not copy the disk, so CellosFS writes from boot 1 survive into boot 2.
#[test]
fn riscv64_cellosfs_srv_persistence() {
    if !prerequisites_ok_with_disk() {
        return;
    }

    // Single temp file shared by both QEMU runs.
    let tmp = tempfile::Builder::new()
        .suffix(".img")
        .tempfile()
        .expect("create temp disk");
    std::fs::copy(srv_disk(), tmp.path()).expect("copy srv disk");
    let tmp_path = tmp.path().to_str().unwrap().to_owned();

    // Boot 1: srv-test runs all six scenarios and writes /srv/persist.txt.
    {
        let r = QemuRunner::boot_rv64_with_disk(&srv_test_kernel(), &tmp_path);
        r.wait_for("[srv-test] PERSIST_WRITE_DONE", 120)
            .unwrap_or_else(|e| {
                eprintln!("--- boot-1 serial ---\n{}\n---", r.dump());
                panic!("boot 1: {e}");
            });
        retain_serial("reboot-1", &r.dump());
    } // drop kills QEMU; tmp file stays intact

    // Boot 2: srv-test finds /srv/persist.txt from boot 1 and prints PERSIST_READ_OK.
    {
        let r = QemuRunner::boot_rv64_with_disk(&srv_test_kernel(), &tmp_path);
        r.wait_for("[srv-test] PERSIST_READ_OK", 120)
            .unwrap_or_else(|e| {
                eprintln!("--- boot-2 serial ---\n{}\n---", r.dump());
                panic!("boot 2 (persistence): {e}");
            });
        retain_serial("reboot-2", &r.dump());
    }
    // tmp dropped here; temp disk image is deleted.
}

/// Compare the complete P5 window after QEMU has exited; checking just the
/// superblocks could miss writes to data/allocator blocks during refusal.
fn assert_p5_unchanged(path: &std::path::Path, mut before: std::fs::File) {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).expect("open P5 image");
    file.seek(SeekFrom::Start(931_072 * 512)).expect("seek P5");
    before.seek(SeekFrom::Start(0)).expect("rewind P5 snapshot");
    let mut actual = [0; 65536];
    let mut expected = [0; 65536];
    for _ in 0..1024 {
        file.read_exact(&mut actual).expect("read P5");
        before.read_exact(&mut expected).expect("read P5 snapshot");
        assert_eq!(actual, expected, "refused P5 bytes changed");
    }
}

fn assert_attached_p5_refused(corrupt: bool) {
    use std::io::{Read, Seek, SeekFrom, Write};
    if !prerequisites_ok_with_disk() {
        return;
    }
    let mut tmp = tempfile::Builder::new().suffix(".img").tempfile().expect("create refusal image");
    std::fs::copy(srv_disk(), tmp.path()).expect("copy preprovisioned image");
    tmp.as_file_mut().seek(SeekFrom::Start(931_072 * 512)).expect("seek P5");
    if corrupt {
        // Destroy both recovery slots while leaving formatted allocator/root bytes
        // in place: runtime cannot reinterpret this as blank/newly authorized media.
        tmp.as_file_mut().write_all(&[0xA5; 8192]).expect("corrupt both P5 slots");
    } else {
        let zeros = [0; 65536];
        for _ in 0..1024 {
            tmp.as_file_mut().write_all(&zeros).expect("blank entire P5");
        }
    }
    tmp.as_file().sync_all().expect("sync refusal fixture");
    tmp.as_file_mut().seek(SeekFrom::Start(931_072 * 512)).expect("snapshot P5 seek");
    let mut before = tempfile::tempfile().expect("create P5 snapshot");
    assert_eq!(
        std::io::copy(&mut tmp.as_file_mut().take(131_072 * 512), &mut before)
            .expect("snapshot complete P5"),
        131_072 * 512
    );
    {
        let runner = QemuRunner::boot_rv64_with_disk(&srv_test_kernel(), tmp.path().to_str().unwrap());
        // Live Stat refuses rather than synthesizing /srv. The fixture collapses
        // error numbers; exact ERR_IO is covered by host dispatch fault scenarios.
        runner.wait_for("[srv-test] S1 mount: FAIL (Err)", 120)
            .unwrap_or_else(|e| panic!("{e}\n{}", runner.dump()));
        runner.wait_for("[srv-test] S2 write+read: FAIL (write failed)", 120)
            .unwrap_or_else(|e| panic!("{e}\n{}", runner.dump()));
        runner.wait_for("[srv-test] FAILURES DETECTED", 120)
            .unwrap_or_else(|e| panic!("{e}\n{}", runner.dump()));
        let serial = runner.dump();
        retain_serial(if corrupt { "corrupt" } else { "blank" }, &serial);
        assert!(!serial.contains("[srv-test] S1 mount: PASS"), "{serial}");
        assert!(!serial.contains("[srv-test] S2 write+read: PASS"), "{serial}");
        assert!(!serial.contains("[srv-test] ALL TESTS PASSED"), "{serial}");
        assert!(!serial.contains("[KERNEL PANIC]") && !serial.contains("panicked at"), "{serial}");
    } // QemuRunner drop kills/waits before host reads the image.
    assert_p5_unchanged(tmp.path(), before);
}

#[test]
fn riscv64_cellosfs_srv_blank_refuses_without_format() {
    assert_attached_p5_refused(false);
}

#[test]
fn riscv64_cellosfs_srv_corrupt_refuses_without_format() {
    assert_attached_p5_refused(true);
}
