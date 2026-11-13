//! Phase 05: Stateful Native Workload Outcome Integration Test.
//!
//! Boot QEMU with cellos-kernel-native-workload and disk_srv.img.
//! Wait for shell.
//! hotswap-demo-v1 runs as init's supervised `hotswap-demo` service.
//! The shell drives `bench native-stateful` step by step (prime, hotswap,
//! restart, finish, failed hotswap, live) until `ALL CRITERIA PASSED`.

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

    // Init starts hotswap-demo-v1 as the supervised `hotswap-demo` service
    // (packaging feature `hotswap-demo`); the shell refuses `cmd &`.
    qemu.wait_for("[hotswap-demo-v1] ready", 30)
        .unwrap_or_else(|e| panic!("demo-v1 not ready: {e}\n{}", qemu.dump()));

    // Each step runs in the foreground; `step` waits for its marker and fails
    // fast on the scenario's own FAIL line. See the step list in
    // cells/tests/bench/src/scenarios/native_stateful.rs.
    let mut step = |line: &str, marker: &str| {
        let from = qemu.dump().len();
        qemu.send_line(line);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(WORKLOAD_TIMEOUT);
        loop {
            let output = qemu.dump();
            let new = &output[from..];
            assert!(
                !new.contains("[native-stateful] FAIL"),
                "`{line}` failed\n{output}"
            );
            if new.contains(marker) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "`{line}`: {marker:?} not seen in {WORKLOAD_TIMEOUT}s\n{output}"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    step("cd /srv", "Cellos > ");
    step(
        "bench native-stateful prime",
        "[native-stateful] primed: v1 counter=300, checkpoints 1-3, cached sender armed",
    );
    step(
        "hotswap hotswap-demo /bin/hotswap-demo-v2",
        "hotswap: success: supervisor swapped service 'hotswap-demo' to /bin/hotswap-demo-v2",
    );
    // The bench ends with the VFS it kills (its cwd was prepared by that VFS);
    // init restarts VFS on the persistent /srv volume.
    step("bench native-stateful restart", "Init: service restarted.");
    step(
        "bench native-stateful finish",
        "[native-stateful] 1000/1000 ops, 10 checkpoints verified",
    );
    step(
        "hotswap hotswap-demo /bin/hotswap-demo-nonexistent",
        "hotswap: supervisor could not spawn the replacement ELF",
    );
    step(
        "bench native-stateful live",
        "[native-stateful] ALL CRITERIA PASSED",
    );

    let serial = qemu.dump();
    assert!(serial.contains("[native-stateful] op 301 reconciled via the cached-TID witness"));
    assert!(serial.contains("HOSTILE_BACKEND_DISCONNECT service=vfs"));
    assert!(serial.contains("[native-stateful] checkpoints 1-6 intact after the VFS restart"));
}
