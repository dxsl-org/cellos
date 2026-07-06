//! P07 — Cluster net-broker boot gate.
//!
//! Verifies that the net-broker Cell boots correctly on an ARM64 node:
//!   1. Entropy gate passes (VirtIO-RNG accessible at broker Init).
//!   2. X25519 static keypair is generated (Noise KKpsk0 transport ready).
//!
//! This test is the P07 GATE — P08 (gossip/lease) and P09 (enrollment) depend
//! on it passing before proceeding. If this test fails, the broker is broken
//! and cluster feature work should not continue.
//!
//! ## Prerequisites
//!
//!   - `qemu-system-aarch64` on PATH (or `$ViCell_QEMU_AARCH64`).
//!   - Kernel: `target/aarch64-unknown-none-softfloat/release/vicell-kernel`
//!     (build: `RUSTFLAGS="-C relocation-model=pic ..." cargo build --release ...`).
//!   - Disk: `disk_arm_virt.img` with `/bin/net-broker` signed and installed.
//!     (build: `cargo build --release -p service-net-broker ...` then `.\gen_disk.ps1`).
//!   - VirtIO-RNG in the QEMU command line (added by `QemuRunner::boot_aarch64_with_disk`).
//!
//! If any prerequisite is absent the test SKIPs gracefully rather than failing.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_aarch64, QemuRunner};

/// Generous timeout: two QEMU boots + init service sequencing can take ~60s.
const BOOT_TIMEOUT: u64 = 60;
/// Broker-specific timeout after the shell prompt is reached.
const BROKER_TIMEOUT: u64 = 15;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/aarch64-unknown-none-softfloat/release/vicell-kernel")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root().join("disk_arm_virt.img").to_string_lossy().into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok   = PathBuf::from(disk_path()).exists();
    let qemu_ok   = std::process::Command::new(qemu_binary_aarch64())
        .arg("--version")
        .output()
        .is_ok();

    if !kernel_ok {
        eprintln!("SKIP cluster-boot: kernel not built ({})", kernel_path());
        eprintln!("  Run: RUSTFLAGS=\"-C relocation-model=pic -C target-feature=+bti,+paca,+pacg\"");
        eprintln!("       cargo build --release -p vicell-kernel --target aarch64-unknown-none-softfloat");
    }
    if !disk_ok {
        eprintln!("SKIP cluster-boot: disk_arm_virt.img missing");
        eprintln!("  Run: cargo build --release -p service-net-broker --target aarch64-unknown-none-softfloat");
        eprintln!("       .\\gen_disk.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP cluster-boot: qemu-system-aarch64 not on PATH or $ViCell_QEMU_AARCH64");
    }
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

/// P07 GATE — net-broker entropy gate must pass on a single node.
///
/// The broker panics at Init if VirtIO-RNG is absent (fail-closed policy).
/// This test verifies the opposite: with VirtIO-RNG present (added by
/// `boot_aarch64_with_disk` which uses `-object rng-builtin`), the broker
/// must print the entropy gate confirmation and the keypair-ready message.
///
/// If this test fails:
///   - "[net-broker] VirtIO-RNG entropy gate passed" missing → broker panicked
///     before generating Noise keys. Check VirtIO-RNG QEMU args.
///   - "[net-broker] static keypair ready" missing → broker panicked after
///     seeding but before keygen. Likely a clatter API mismatch.
#[test]
fn cluster_broker_entropy_gate_passes() {
    if !prerequisites_ok() { return; }

    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());

    // Wait for full boot first — init must start before the broker is spawned.
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!(
            "P07 GATE FAIL: shell prompt not reached within {BOOT_TIMEOUT}s: {e}\n\
             The broker cannot start before init completes service registration.\n\
             --- serial output ---\n{}",
            qemu.dump()
        ));

    // The broker starts as part of init's supervised services — verify startup.
    qemu.wait_for("[net-broker] VirtIO-RNG entropy gate passed", BROKER_TIMEOUT)
        .unwrap_or_else(|e| panic!(
            "P07 GATE FAIL: broker entropy gate not confirmed within {BROKER_TIMEOUT}s: {e}\n\
             Expected: \"[net-broker] VirtIO-RNG entropy gate passed\"\n\
             Possible causes:\n\
               1. /bin/net-broker not installed in disk_arm_virt.img (re-run gen_disk.ps1)\n\
               2. init does not spawn /bin/net-broker (check init spawn table)\n\
               3. VirtIO-RNG absent in QEMU args (boot_aarch64_with_disk should include it)\n\
               4. Broker crashed before entropy gate — check for panic in serial output\n\
             --- serial output ---\n{}",
            qemu.dump()
        ));

    qemu.wait_for("[net-broker] static keypair ready", BROKER_TIMEOUT)
        .unwrap_or_else(|e| panic!(
            "P07 GATE FAIL: broker keypair not ready within {BROKER_TIMEOUT}s: {e}\n\
             Entropy gate passed but X25519 keygen failed.\n\
             Check clatter/x25519-dalek no_std compatibility.\n\
             --- serial output ---\n{}",
            qemu.dump()
        ));
}

/// P07 GATE — net-broker service lookup (NET_BROKER = 8) succeeds.
///
/// init must register service::NET_BROKER (id=8) on the broker's behalf.
/// This test asks the shell to verify the broker is reachable.
///
/// NOTE: This test depends on the shell having a `lssvc` or equivalent
/// introspection command. Until that command exists it is tagged `#[ignore]`
/// and serves as a specification/placeholder for P07 completion.
#[test]
#[ignore = "lssvc shell command not yet implemented; P07 specification placeholder"]
fn cluster_broker_service_registered() {
    if !prerequisites_ok() { return; }

    let mut qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell prompt not reached: {e}"));

    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("lssvc");

    // Once lssvc exists: assert service id=8 is registered as NET_BROKER.
    qemu.wait_for("8 net-broker", 10)
        .unwrap_or_else(|e| panic!(
            "NET_BROKER (id=8) not in service table: {e}\n--- output ---\n{}", qemu.dump()
        ));
}
