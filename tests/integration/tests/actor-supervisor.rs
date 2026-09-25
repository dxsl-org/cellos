//! B0 actor/supervisor-tree witness (ADR-0021).
//!
//! The witness is a real guest script: `backend-supervisor` declares a tree over
//! `/bin/backend-worker`, kills its own children, and asserts what the library
//! did. `scripts/qemu-actor-supervisor.sh` owns the whole path (build, sign,
//! private disk overlay, boot, marker assertions), so this test drives that
//! runner instead of duplicating it.
//!
//! It is ignored by the general host sweep because it rebuilds a cross-compiled
//! kernel and guest. Run it directly:
//!
//! ```text
//! cargo test --test actor-supervisor -- --ignored --test-threads=1
//! ```

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

#[test]
#[ignore = "requires RV64 QEMU, a built kernel and disk_v3.img"]
fn riscv64_actor_supervisor_tree_reaches_pass() {
    let output = Command::new("bash")
        .args(["scripts/qemu-actor-supervisor.sh", "--harts", "1"])
        .current_dir(repo_root())
        .output()
        .expect("launch the actor/supervisor QEMU runner");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "actor/supervisor QEMU runner failed\n--- stdout ---\n{stdout}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("ACTOR-SUPERVISOR-QEMU: PASS"),
        "runner did not report its PASS marker\n--- stdout ---\n{stdout}"
    );
    // The supervisor measures its own restart latency; the runner only prints it.
    // Asserting here too keeps the number visible in the test transcript.
    assert!(
        stdout.contains("restart-latency ticks="),
        "runner did not surface the measured restart latency\n--- stdout ---\n{stdout}"
    );
}
