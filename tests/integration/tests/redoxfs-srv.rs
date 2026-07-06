//! RedoxFS /srv integration tests.
//!
//! Three test functions:
//!
//! 1. `riscv64_redoxfs_srv_basic` — single boot with a P5-formatted disk;
//!    waits for the srv-test cell to complete all 5 scenarios.
//!
//! 2. `riscv64_redoxfs_srv_degrade_no_disk` — boot with no VirtIO-BLK; confirms
//!    the VFS service warns and degrades gracefully instead of panicking.
//!
//! 3. `riscv64_redoxfs_srv_persistence` — two sequential boots against the same
//!    temp disk; the srv-test cell writes a persist marker in boot 1 and the
//!    harness verifies it is announced as found in boot 2.
//!
//! Prerequisites:
//!   scripts/build-srv-test-ci.sh  →  target/.../vicell-kernel-srv-test
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
        .join("target/riscv64gc-unknown-none-elf/release/vicell-kernel-srv-test")
        .to_string_lossy()
        .into_owned()
}

/// Standard test-hooks kernel (no disk) — used by the degrade test to verify
/// VFS behaves gracefully when no VirtIO-BLK device is present.
fn test_hooks_kernel() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/vicell-kernel-test-hooks")
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

/// S1–S5: mount, write+read, listdir, mkdir, unlink.
///
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

    let runner = QemuRunner::boot_rv64_with_disk(
        &srv_test_kernel(),
        tmp.path().to_str().unwrap(),
    );

    runner
        .wait_for("[srv-test] ALL TESTS PASSED", 120)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });
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

    // Boot 1: srv-test runs all 5 scenarios and writes /srv/persist.txt.
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
