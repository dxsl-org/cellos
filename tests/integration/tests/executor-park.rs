//! Parked-executor TIMER witness (Spec 03b, async reactor ADR, Phase 05).
//!
//! `ostd::executor::block_on` must park a pending future through
//! `WaitCompletion(TIMER)` rather than spin, and logs
//! `[executor] dummy-waker=absent executor=parked source=TIMER PASS` the first
//! time a cell does so.
//!
//! The only boot-time consumer is the graphical shell: with a compositor present,
//! Cellbox runs `ViShell::run` under `block_on`, and its first await is a 2 s
//! `sleep`. A headless image (`disk_v3.img`) has no compositor at shell start, so
//! the shell takes the synchronous serial-session loop and never parks. This
//! test therefore boots the `input,ui` image (`CELLOS_UI_DISK`, default
//! `disk_ocel_static.img`, built by CI's static-viewer step) with virtio-gpu and
//! the pointer devices.

use std::path::PathBuf;
use vicell_integration_tests::{ci_guard, qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const MARKER: &str = "[executor] dummy-waker=absent executor=parked source=TIMER PASS";

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

fn ui_disk_path() -> String {
    std::env::var("CELLOS_UI_DISK").unwrap_or_else(|_| {
        repo_root()
            .join("disk_ocel_static.img")
            .to_string_lossy()
            .into_owned()
    })
}

fn prerequisites_ok() -> bool {
    let present = PathBuf::from(kernel_path()).exists()
        && PathBuf::from(ui_disk_path()).exists()
        && std::process::Command::new(qemu_binary())
            .arg("--version")
            .output()
            .is_ok();
    ci_guard(present)
}

#[test]
fn graphical_shell_parks_its_executor_on_timer() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_with_pointer(&kernel_path(), &ui_disk_path());
    qemu.wait_for(MARKER, BOOT_TIMEOUT).unwrap_or_else(|error| {
        panic!(
            "parked executor TIMER proof not observed: {error}\n--- output ---\n{}",
            qemu.dump()
        )
    });
}
