//! Explicit RV64 QEMU target for test-hooks native-domain assertions.
//!
//! It is ignored by the general host test sweep because it rebuilds a dedicated
//! cross-compiled guest. Run this target directly in the RV64 QEMU job.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn run(harts: &str, cases: &str) {
    let output = Command::new("bash")
        .args([
            "scripts/qemu-native-domain-test.sh",
            "--harts",
            harts,
            "--case",
            cases,
        ])
        .current_dir(repo_root())
        .output()
        .expect("launch native-domain QEMU runner");

    assert!(
        output.status.success(),
        "native-domain QEMU runner failed for harts={harts}, cases={cases}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
#[ignore = "requires RV64 QEMU and a fresh cross-compiled test-hooks guest"]
fn riscv64_native_domain_cases() {
    run("1", "switch,sas-fastpath");
    run("2", "migration");
}
