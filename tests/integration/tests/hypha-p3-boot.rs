//! Hypha P3 integration test — tool-sys + tool-spawn cell spawn gate.
//!
//! Verifies that launching `/bin/hypha` successfully spawns all four P3 tool
//! cells (llm-gateway, tool-fs, tool-sys, tool-spawn) and each prints its
//! "ready" banner before the first prompt appears. No LLM mock proxy needed —
//! this only exercises the spawn path and cell initialisation.
//!
//! Gate conditions:
//!   PASS  — all four "[tool-*] ready" banners appear + "you>" prompt
//!   SKIP  — kernel/disk/qemu prerequisites not met
//!
//! Prerequisites:
//!   RUSTFLAGS="-C relocation-model=pic" cargo build --release -p vicell-kernel
//!   cargo build --release -p hypha-core -p hypha-llm-gateway \
//!                          -p hypha-tool-fs -p hypha-tool-sys -p hypha-tool-spawn
//!   ./gen_disk.ps1
//!   qemu-system-riscv64 on PATH

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT:   u64 = 60;
const SPAWN_TIMEOUT:  u64 = 30;
const PROMPT_TIMEOUT: u64 = 20;
const EXIT_TIMEOUT:   u64 = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/vicell-kernel")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root().join("disk_v3.img").to_string_lossy().into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok   = PathBuf::from(disk_path()).exists();
    let qemu_ok   = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_ok {
        eprintln!(
            "SKIP hypha-p3-boot: kernel not built ({})\n  \
             Run: RUSTFLAGS=\"-C relocation-model=pic\" cargo build --release -p vicell-kernel",
            kernel_path()
        );
    }
    if !disk_ok {
        eprintln!("SKIP hypha-p3-boot: disk_v3.img missing — run ./gen_disk.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP hypha-p3-boot: qemu-system-riscv64 not on PATH");
    }
    kernel_ok && disk_ok && qemu_ok
}

/// P3 gate: all tool cells must print their "ready" banner before the first prompt.
///
/// Launches `hypha` from the Cellos shell. `core` spawns llm-gateway, tool-fs,
/// tool-sys, and tool-spawn; each prints `[tool-*] ready`. The test asserts
/// each banner appears (in any order) then waits for the interactive prompt.
/// Sends "exit" and asserts a clean-exit line — confirming the whole P3 spawn
/// path works end-to-end without touching the LLM network path.
#[test]
fn hypha_p3_tool_cells_spawn() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());

    qemu.wait_for("ViCell >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!("shell prompt not reached within {BOOT_TIMEOUT}s: {e}\n--- output ---\n{}", qemu.dump())
    });

    // Use UART (send_line) to launch hypha — the UART relay path is tested here,
    // which is the same path any operator would use over a serial console.
    // Note: send_line appends '\n' so no separate "ret" key needed.
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("hypha");

    // gateway must start before core prints its prompt
    qemu.wait_for("[hypha/llm-gateway] service ready", SPAWN_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "llm-gateway ready banner not seen within {SPAWN_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // tool-fs (P2)
    qemu.wait_for("[tool-fs] ready", SPAWN_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "[tool-fs] ready not seen within {SPAWN_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // tool-sys (P3)
    qemu.wait_for("[tool-sys] ready", SPAWN_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "[tool-sys] ready not seen within {SPAWN_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // tool-spawn (P3)
    qemu.wait_for("[tool-spawn] ready", SPAWN_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "[tool-spawn] ready not seen within {SPAWN_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // interactive prompt
    qemu.wait_for("you>", PROMPT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "Hypha input prompt 'you>' not seen within {PROMPT_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    qemu.send_line("exit");

    qemu.wait_for("[hypha] bye", EXIT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "Hypha did not exit cleanly within {EXIT_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
}
