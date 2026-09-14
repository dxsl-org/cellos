//! Hypha → local inference backend gate: the agent answers a turn from the on-device AI Cell.
//!
//! Boots the canonical RV64 image, launches `/bin/hypha` from the shell, and types one chat line.
//! The request path is the shipped one end to end:
//!
//!   hypha `core` → `llm-gateway` (IPC) → `AiClient` (typed IPC, `service::AI`) → inference Cell
//!   → GGUF fixture in the cell-store → reply line on the serial console
//!
//! What the assertions cover: the gateway names the local backend and the resident model, `core`
//! prints a reply line for the typed turn, a second turn is answered too (the service releases a
//! finished session slot), and the turn never reaches the network backend. What they deliberately
//! do not cover: reply language quality — the canonical image carries the deterministic fixture,
//! whose output is not language — plus latency, tool-calling, and any board or accelerator claim.
//!
//! Prerequisites, skipped locally when absent and required in CI: a built RV64 kernel
//! (`target/riscv64gc-unknown-none-elf/release/cellos-kernel`), `disk_v3.img` from `gen_disk.ps1`
//! with `/bin/hypha`, `/bin/llm-gateway`, `/bin/ai`, and the model in the cell-store, and a
//! `qemu-system-riscv64` on PATH.

use std::path::PathBuf;

use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 90;
const SPAWN_TIMEOUT: u64 = 30;
const PROMPT_TIMEOUT: u64 = 20;
/// One local generation is bounded (64 tokens, four model steps per poll) but runs under TCG.
const REPLY_TIMEOUT: u64 = 60;
const EXIT_TIMEOUT: u64 = 10;

/// The fixture the canonical image deploys as `/bin/ai-model.gguf`.
const FIXTURE_MODEL: &str = "tiny-llama-64";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root()
        .join("disk_v3.img")
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
    if !kernel_ok {
        eprintln!(
            "SKIP hypha-local-ai: kernel not built ({})\n  \
             Run: RUSTFLAGS=\"-C relocation-model=pic\" cargo build --release -p cellos-kernel",
            kernel_path()
        );
    }
    if !disk_ok {
        eprintln!("SKIP hypha-local-ai: disk_v3.img missing — run ./gen_disk.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP hypha-local-ai: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

#[test]
fn hypha_answers_a_turn_from_the_local_inference_cell() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());

    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell prompt not reached within {BOOT_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    // The local backend has to be resident before the turn, otherwise the gateway would be
    // measuring the boot race rather than the consumer path.
    qemu.wait_for("[ai] model ready", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "inference service not ready: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("hypha");

    qemu.wait_for("[hypha/llm-gateway] service ready", SPAWN_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "llm-gateway ready banner not seen: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for("you>", PROMPT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "Hypha input prompt 'you>' not seen: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // 1. A typed turn is answered by the local inference Cell, which is named in the log.
    qemu.send_line("hello");
    qemu.wait_for(
        &format!("[gw] local AI backend: {FIXTURE_MODEL}"),
        REPLY_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "the gateway did not answer from the local AI Cell within {REPLY_TIMEOUT}s: {e}\n\
                 --- output ---\n{}",
            qemu.dump()
        )
    });
    qemu.wait_for("hypha> ", REPLY_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "no reply line for the typed turn within {REPLY_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // 2. A second turn: the service released the finished session slot, so the next one is seated.
    // Each line goes out only once its prompt is on the wire (and each turn's backend line is
    // matched once more), so the gate measures the consumer path, not how far input can run ahead
    // of the reader.
    qemu.wait_for("you>", PROMPT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "second prompt not offered: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    qemu.send_line("again");
    qemu.wait_for(
        &format!("[gw] local AI backend: {FIXTURE_MODEL}"),
        REPLY_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "the second turn was not answered from the local AI Cell: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    qemu.wait_for("hypha> ", REPLY_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "no reply line for the second turn within {REPLY_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    qemu.wait_for("you>", PROMPT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "third prompt not offered: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    qemu.send_line("exit");
    // One harness write was observed to vanish here — the guest never echoed the keystrokes, while
    // the same input typed over QEMU's stdio console is read normally — so the gate retries once
    // instead of reporting the app as ignoring `exit`. Serial write errors are swallowed by the
    // harness (`let _ = write_all`), which is exactly why the log cannot distinguish the two.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    if !qemu.dump().contains("[hypha] bye") {
        qemu.send_line("exit");
    }
    qemu.wait_for("[hypha] bye", EXIT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "Hypha did not exit cleanly: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // The network backend never ran: its transport line is the marker a fallback would print.
    let output = qemu.dump();
    assert!(
        !output.contains("plaintext mode"),
        "a local turn must not reach the network backend\n--- output ---\n{output}"
    );
}
