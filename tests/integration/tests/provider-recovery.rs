//! Phase 09B: software provider crash/restore at a provider-owned state
//! authority (`.agents/261030-1945-ginux-lessons-hardening/phase-09b-cp1.md` §7).
//!
//! The image carries the `recovery-demo` packaging feature: init starts
//! `/bin/recovery-authority`, which spawns and restarts `/bin/recovery-worker`;
//! `/bin/recovery-client` (fault-inject build) drives E1–E12 from the shell and
//! asserts what a consumer observes. Build it with
//! `CELLOS_INIT_FEATURES=input,recovery-demo bash scripts/gen-disk-ci.sh`.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const CORE_TIMEOUT: u64 = 900;
const STEP_TIMEOUT: u64 = 180;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").canonicalize().expect("repo root")
}

fn kernel_path() -> String {
    repo_root().join("target/riscv64gc-unknown-none-elf/release/cellos-kernel").to_string_lossy().into_owned()
}

fn disk_path() -> String {
    repo_root().join("disk_v3.img").to_string_lossy().into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary()).arg("--version").output().is_ok();
    if !(kernel_ok && disk_ok) {
        eprintln!("SKIP: build the image with CELLOS_INIT_FEATURES=input,recovery-demo bash scripts/gen-disk-ci.sh");
    }
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

fn boot(harts: usize) -> QemuRunner {
    let mut qemu = QemuRunner::boot_with_fresh_disk_smp(&kernel_path(), &disk_path(), harts);
    expect(&mut qemu, "[recovery-authority] READY worker=", BOOT_TIMEOUT, "authority never published its first worker");
    expect(&mut qemu, "Cellos >", BOOT_TIMEOUT, "shell not reached");
    qemu
}

fn expect(qemu: &mut QemuRunner, marker: &str, timeout: u64, what: &str) {
    qemu.wait_for(marker, timeout).unwrap_or_else(|e| panic!("{what}: {e}\n--- serial ---\n{}", qemu.dump()));
}

fn run(qemu: &mut QemuRunner, command: &str, group: &str, timeout: u64) {
    let start = qemu.output_checkpoint();
    qemu.send_line(command);
    let pass = format!("RECOVERY-CLIENT {group}: PASS");
    qemu.wait_for_after(&pass, start, timeout)
        .unwrap_or_else(|e| panic!("{command}: {e}\n--- serial ---\n{}", qemu.dump()));
    let log = qemu.dump();
    assert!(!log[start..].contains("[recovery-client] FAIL"), "{command} failed\n{log}");
    // The next command must reach the shell, not the exiting client.
    let after_pass = start + log[start..].find(&pass).expect("pass marker");
    qemu.wait_for_after("Cellos >", after_pass, STEP_TIMEOUT)
        .unwrap_or_else(|e| panic!("prompt after {command}: {e}\n{}", qemu.dump()));
}

fn remote_waits(log: &str) -> u32 {
    let marker = "[recovery-client] remote-hart retirement waits: ";
    let at = log.find(marker).expect("core reports remote-hart waits") + marker.len();
    log[at..].split_whitespace().next().and_then(|n| n.parse().ok()).expect("count")
}

/// E1–E12 on two harts: crash points, conflicts, fencing, restore refusal,
/// whole-Cell retirement across CPUs, quarantine of a missing borrower
/// acknowledgement, ten reconciled cycles, then an untracked export that must
/// leave the provider down (node recovery required).
#[test]
fn provider_recovery_two_harts() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = boot(2);
    run(&mut qemu, "recovery-client core", "core", CORE_TIMEOUT);
    run(&mut qemu, "recovery-client foreign 1002", "foreign", STEP_TIMEOUT);
    run(&mut qemu, "recovery-client lease", "lease", STEP_TIMEOUT);
    run(&mut qemu, "recovery-client export", "export", STEP_TIMEOUT);
    let log = qemu.dump();
    for marker in [
        "[recovery-worker] RecoveryFailed:",
        "[recovery-authority] RecoveryFailed:",
        "[vfs] recovery-fixture: holding grant lease without acknowledgement",
        "[recovery-authority] node recovery required: containment unproven, no restart",
        "state=Unproven",
        "state=Quarantined",
    ] {
        assert!(log.contains(marker), "missing `{marker}`\n{log}");
    }
    assert!(remote_waits(&log) > 0, "no retirement waited for a remote hart on 2 harts\n{log}");
    println!("{log}");
}

/// The same core scenario on one hart: identical outcomes, no remote waits.
#[test]
fn provider_recovery_one_hart() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = boot(1);
    run(&mut qemu, "recovery-client core", "core", CORE_TIMEOUT);
    let log = qemu.dump();
    assert_eq!(remote_waits(&log), 0, "a 1-hart retirement cannot wait for a remote hart\n{log}");
    println!("{log}");
}
