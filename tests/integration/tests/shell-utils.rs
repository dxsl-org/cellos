//! Shell utility integration test (Phase E — Shell M3.1).
//!
//! Boots a shell-test kernel (compiled with `app-shell --features shell_test`)
//! and waits for the `[shell-test] COMPLETE` marker before checking the final
//! result printed by `cells/tools/shell/src/shell_test.rs`.
//!
//! Prerequisites:
//!   bash scripts/build-shell-test-ci.sh
//!   → produces target/riscv64gc-unknown-none-elf/release/cellos-kernel-shell-test
//!
//! Run via:
//!   cargo test --manifest-path tests/integration/Cargo.toml --test shell-utils

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

/// Timeout for the whole test suite to finish inside the guest.
const SUITE_TIMEOUT: u64 = 120;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn shell_test_kernel() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-shell-test")
        .to_string_lossy()
        .into_owned()
}

/// Skip gracefully when prerequisites are missing (no QEMU or no built kernel).
fn prerequisites_ok() -> bool {
    let kernel_path = shell_test_kernel();
    let kernel_exists = PathBuf::from(&kernel_path).exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_exists {
        eprintln!("SKIP: shell-test kernel not built ({})", kernel_path);
        eprintln!("      Run: bash scripts/build-shell-test-ci.sh");
    }
    if !qemu_ok {
        eprintln!("SKIP: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_exists && qemu_ok)
}

/// Phase E: boot the shell-test kernel and wait for all scenario tests to finish.
///
/// The shell-test cell runs `shell_test::run()` on startup, exercises all
/// Phase 1–3 shell features (stderr redirect, tee, sed, fg/bg, pipes), and
/// prints `[shell-test] COMPLETE` after the final success or failure marker.
#[test]
fn shell_utils_all_scenarios_pass() {
    if !prerequisites_ok() {
        return;
    }
    let kernel = shell_test_kernel();
    // boot_rv64: minimal config (no disk, no VirtIO peripherals).
    // The shell-test kernel embeds init + vfs + shell-test in its kernel_fs.img.
    let qemu = QemuRunner::boot_rv64(&kernel);

    qemu.wait_for("[shell-test] COMPLETE", SUITE_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "shell-test suite did not complete within {}s: {}\n--- serial output ---\n{}",
                SUITE_TIMEOUT,
                e,
                qemu.dump()
            )
        });
    let output = qemu.dump();
    for marker in [
        "[shell-test] PASS  test -f uses stat for files larger than sample buffers",
        "[shell-test] PASS  bounded handle read exact bound",
        "[shell-test] PASS  bounded handle read exceeds 480 bytes",
        "[shell-test] PASS  bounded handle read rejects truncation",
        "[shell-test] PASS  bounded handle read preserves directory error",
        "[shell-test] PASS  bounded handle read preserves missing error",
        "[shell-test] PASS  bounded handle read cleans up after errors",
        "[shell-test] PASS  shell cwd root direct",
        "[shell-test] PASS  shell cwd root captured",
        "[shell-test] PASS  shell cwd relative cd succeeds",
        "[shell-test] PASS  shell cwd relative direct",
        "[shell-test] PASS  shell cwd relative captured",
        "[shell-test] PASS  shell cwd absolute root cd succeeds",
        "[shell-test] PASS  shell cwd absolute BIN cd succeeds",
        "[shell-test] PASS  shell cwd BIN direct",
        "[shell-test] PASS  shell cwd BIN captured",
        "[shell-test] PASS  shell cwd dot cd succeeds",
        "[shell-test] PASS  shell cwd dot direct",
        "[shell-test] PASS  shell cwd dot captured",
        "[shell-test] PASS  shell cwd dotdot cd succeeds",
        "[shell-test] PASS  shell cwd dotdot direct",
        "[shell-test] PASS  shell cwd dotdot captured",
        "[shell-test] PASS  shell cwd root saturation cd succeeds",
        "[shell-test] PASS  shell cwd root saturation direct",
        "[shell-test] PASS  shell cwd root saturation captured",
        "[shell-test] PASS  shell cwd zero operands fails",
        "[shell-test] PASS  shell cwd zero operands retains CWD",
        "[shell-test] PASS  shell cwd two operands fail",
        "[shell-test] PASS  shell cwd two operands retains CWD",
        "[shell-test] PASS  shell cwd missing dir fails",
        "[shell-test] PASS  shell cwd missing dir retains CWD",
        "[shell-test] PASS  shell cwd regular file fails",
        "[shell-test] PASS  shell cwd regular file retains CWD",
        "[shell-test] PASS  shell cwd final BIN cd succeeds",
        "[shell-test] PASS  shell cwd direct matches captured",
        "[shell-test] PASS  shell cwd restore root cd succeeds",
        "[shell-test] PASS  shell cwd restored root direct",
        "[shell-test] PASS  shell cwd restored root captured",
    ] {
        assert!(
            output.contains(marker),
            "missing Phase 05 shell handle-read evidence: {marker}\n--- serial output ---\n{output}"
        );
    }
    assert!(
        output.contains("[shell-test] ALL TESTS PASSED"),
        "shell-test suite completed with failures\n--- serial output ---\n{output}"
    );
    assert!(
        !output.contains("[shell-test] FAIL")
            && !output.contains("Load access fault")
            && !output.contains("Store/AMO access fault")
            && !output.contains("Instruction access fault"),
        "shell-test output contains a failure or guest fault\n--- serial output ---\n{output}"
    );
}
