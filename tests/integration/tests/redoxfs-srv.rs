//! RedoxFS /srv integration tests.
//!
//! Three test functions:
//!
//! 1. `riscv64_redoxfs_srv_basic` — single boot with a P5-formatted disk;
//!    waits for all six srv-test scenarios, then runs the POSIX rename smoke.
//!
//! 2. `riscv64_redoxfs_srv_degrade_no_disk` — boot with no VirtIO-BLK; confirms
//!    the VFS service warns and degrades gracefully instead of panicking.
//!
//! 3. `riscv64_redoxfs_srv_persistence` — two sequential boots against the same
//!    temp disk; the srv-test cell writes a persist marker in boot 1 and the
//!    harness verifies it is announced as found in boot 2.
//!
//! Prerequisites:
//!   scripts/build-srv-test-ci.sh  →  target/.../cellos-kernel-srv-test
//!   scripts/mksrv-img.sh          →  build/disk_srv.img
//!
//! Run:
//!   cargo test --manifest-path tests/integration/Cargo.toml --test redoxfs-srv

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
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-srv-test")
        .to_string_lossy()
        .into_owned()
}

/// Standard test-hooks kernel (no disk) — used by the degrade test to verify
/// VFS behaves gracefully when no VirtIO-BLK device is present.
fn test_hooks_kernel() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks")
        .to_string_lossy()
        .into_owned()
}

fn srv_disk() -> String {
    repo_root()
        .join("build/disk_srv.img")
        .to_string_lossy()
        .into_owned()
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
fn riscv64_redoxfs_srv_basic() {
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

    // Exercise live POSIX directory lifecycle and rename on RedoxFS P5 (/srv)
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
    let serial = runner.dump();
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
/// must NOT panic.  Uses the vfs-quota test-hooks kernel (smallest env).
#[test]
fn riscv64_redoxfs_srv_degrade_no_disk() {
    let kernel = PathBuf::from(test_hooks_kernel());
    if !kernel.exists() {
        eprintln!(
            "SKIP: test-hooks kernel not found ({}). Run scripts/build-test-hooks-ci.sh first.",
            test_hooks_kernel()
        );
        return;
    }
    if !qemu_ok() {
        eprintln!("SKIP: qemu-system-riscv64 not on PATH");
        return;
    }

    // boot_rv64 attaches NO block device — VFS falls back to None on P5 open.
    let runner = QemuRunner::boot_rv64(kernel.to_str().unwrap());
    runner
        .wait_for("[vfs] WARNING: RedoxFS P5 open failed", 60)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });
}

/// S7: write persist marker in boot 1, kill QEMU, boot 2 with same image →
/// confirm the marker is detected by the cell.
///
/// Both boots share one `NamedTempFile` for the disk image.  `boot_rv64_with_disk`
/// does not copy the disk, so RedoxFS writes from boot 1 survive into boot 2.
#[test]
fn riscv64_redoxfs_srv_persistence() {
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
    } // drop kills QEMU; tmp file stays intact

    // Boot 2: srv-test finds /srv/persist.txt from boot 1 and prints PERSIST_READ_OK.
    {
        let r = QemuRunner::boot_rv64_with_disk(&srv_test_kernel(), &tmp_path);
        r.wait_for("[srv-test] PERSIST_READ_OK", 120)
            .unwrap_or_else(|e| {
                eprintln!("--- boot-2 serial ---\n{}\n---", r.dump());
                panic!("boot 2 (persistence): {e}");
            });
    }
    // tmp dropped here; temp disk image is deleted.
}
