//! Phase 05: Stateful Native Workload Outcome Integration Test.
//!
//! Boot QEMU with cellos-kernel-native-workload and disk_srv.img.
//! Wait for shell.
//! Launch hotswap-demo-v1 service: `hotswap-demo-v1 &`
//! Launch bench scenario: `bench native-stateful`
//! Wait for `[native-stateful] ALL CRITERIA PASSED`!

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const WORKLOAD_TIMEOUT: u64 = 180;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-native-workload")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root()
        .join("build/disk_srv.img")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

#[test]
fn riscv64_native_stateful_workload_1000_ops() {
    if !prerequisites_ok() {
        return;
    }

    let tmp = tempfile::Builder::new()
        .suffix(".img")
        .tempfile()
        .expect("create temp disk");
    std::fs::copy(disk_path(), tmp.path()).expect("copy srv disk");

    let mut qemu = QemuRunner::boot_rv64_with_disk(&kernel_path(), tmp.path().to_str().unwrap());

    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Spawn hotswap-demo-v1 background service
    qemu.send_line("hotswap-demo-v1 &");
    qemu.wait_for("[hotswap-demo-v1] ready", 30)
        .unwrap_or_else(|e| panic!("demo-v1 not ready: {e}\n{}", qemu.dump()));

    // Run the native stateful workload bench
    qemu.send_line("bench native-stateful");

    // Wait for all 1000 operations, checkpoints, hotswap, and VFS restart recovery
    qemu.wait_for("[native-stateful] ALL CRITERIA PASSED", WORKLOAD_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "workload failed or timed out: {e}\n--- serial output ---\n{}",
                qemu.dump()
            )
        });

    let serial = qemu.dump();
    assert!(serial.contains("[native-stateful] Op 301 reconciled via cached-TID witness: counter=301"));
    assert!(serial.contains("[native-stateful] VFS restart recovery verified: checkpoints 1..6 intact"));
    assert!(serial.contains("[native-stateful] all 10 checkpoints verified against independent oracle"));
    assert!(serial.contains("[native-stateful] failed hotswap preserved live provider and counter"));
    assert!(serial.contains("[native-stateful] Summary: 1000/1000 ops completed, 10 checkpoints verified, 0 errors"));
}
