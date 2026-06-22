//! Hypha P1 integration test — agent cell boot and chat prompt.
//!
//! Verifies that `/bin/hypha` (the agent brain) spawns successfully, prints its
//! P1 banner, prompts for input, and exits cleanly on `exit`. The LLM gateway
//! is spawned by core but the network round-trip is NOT exercised here —
//! that requires the host mock proxy and is a manual step (see os-gaps.md G3).
//!
//! Gate conditions:
//!   PASS  — `hypha` banner appears AND `you>` prompt AND clean exit on "exit"
//!   SKIP  — kernel/disk/qemu prerequisites not met (binary not in disk)
//!
//! Prerequisites:
//!   cargo build --release -p vicell-kernel (RUSTFLAGS="-C relocation-model=pic")
//!   cargo build --release -p hypha-core -p hypha-llm-gateway
//!   ./gen_disk.ps1    (embeds /bin/hypha and /bin/llm-gateway on the disk)
//!   qemu-system-riscv64 on PATH

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const BANNER_TIMEOUT: u64 = 30;
const PROMPT_TIMEOUT: u64 = 15;
const EXIT_TIMEOUT: u64 = 10;

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
    let disk_ok = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_ok {
        eprintln!(
            "SKIP hypha-boot: kernel not built ({})\n  Run: RUSTFLAGS=\"-C relocation-model=pic\" cargo build --release -p vicell-kernel",
            kernel_path()
        );
    }
    if !disk_ok {
        eprintln!("SKIP hypha-boot: disk_v3.img missing — run ./gen_disk.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP hypha-boot: qemu-system-riscv64 not on PATH");
    }
    kernel_ok && disk_ok && qemu_ok
}

/// Hypha P1 banner gate: `/bin/hypha` must print the welcome banner and prompt.
///
/// Spawns `hypha` from the Cellos shell, waits for the P1 welcome line
/// ("Hypha — ViCell's first AI agent"), then the input prompt ("you>"), sends
/// "exit", and waits for the clean-exit message ("[hypha] bye"). No LLM proxy
/// is needed — this only exercises the spawn + readline path.
#[test]
fn hypha_banner_and_prompt() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());

    qemu.wait_for("ViCell >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!("shell prompt not reached within {BOOT_TIMEOUT}s: {e}\n--- output ---\n{}", qemu.dump())
    });

    // Wait a moment for the console to settle (mirrors boot-console pattern).
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("hypha");

    qemu.wait_for("Hypha", BANNER_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "Hypha banner not seen within {BANNER_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

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
