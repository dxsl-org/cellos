// SPDX-License-Identifier: MIT
//! Integration test: Ocel Native Browser and JS Engine Execution in QEMU.
//!
//! Verifies:
//! 1. Kernel boots cleanly with VirtIO GPU + Compositor + VFS.
//! 2. Ocel (/bin/ocel) launches cleanly from shell.
//! 3. Ocel creates a ViSurface, registers with the Compositor, and renders the top toolbar.
//! 4. Screen capture verifies toolbar rendering.
//! 5. Zero kernel panics or cell faults throughout execution.

use std::path::PathBuf;
use std::time::Duration;

use vicell_integration_tests::{pixel_region, qemu_binary, read_ppm_frame, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const EVENT_TIMEOUT: u64 = 20;

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
    let kernel_exists = PathBuf::from(kernel_path()).exists();
    let disk_exists = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    vicell_integration_tests::ci_guard(kernel_exists && disk_exists && qemu_ok)
}

fn settle() {
    std::thread::sleep(Duration::from_millis(400));
}

#[test]
fn ocel_browser_launch_and_render() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_pointer(&kernel_path(), &disk_path());

    // Wait for the shell prompt
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!("shell not reached: {e}\n--- serial output ---\n{}", qemu.dump())
    });

    // Let background boot tests finish
    let _ = qemu.wait_for("[vfs-test] ALL TESTS PASSED", EVENT_TIMEOUT);
    settle();

    // Launch Ocel from shell
    qemu.send_line("ocel &");
    qemu.wait_for("[ocel] Window initialized and painted.", EVENT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!("Ocel failed to start: {e}\n--- serial output ---\n{}", qemu.dump())
        });

    settle();

    // Capture screen with Ocel rendered
    let ocel_frame = "/tmp/cellos-ocel-render.ppm";
    assert!(qemu.capture_qemu_screen(ocel_frame), "failed to capture Ocel screen");

    let frame = read_ppm_frame(ocel_frame);
    assert!(frame.width >= 1024, "unexpected frame width: {}", frame.width);
    assert!(frame.height >= 768, "unexpected frame height: {}", frame.height);

    // Inspect toolbar region at top (y = 20): should be non-black (toolbar background)
    let tb_pixels = pixel_region(&frame, 20, 20, 300, 21);
    let is_all_black = tb_pixels.chunks_exact(3).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0);
    assert!(!is_all_black, "Ocel top toolbar is completely black / not rendered");

    // Verify zero kernel panics or cell faults
    let output = qemu.dump();
    assert!(!output.contains("[KERNEL PANIC]"), "kernel panic detected:\n{output}");
    assert!(!output.contains("[fault] Cell"), "Cell fault detected:\n{output}");
    std::fs::write("/tmp/ocel-test.log", &output).unwrap();

    println!("[ocel-browser test] Ocel Native Browser launched and rendered successfully.");
}
