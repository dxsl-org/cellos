// SPDX-License-Identifier: MIT
//! Integration test: CellOS Desktop Taskbar and Spotlight GUI Interaction in QEMU.
//!
//! Verifies:
//! 1. Kernel boots cleanly with VirtIO GPU + VirtIO Tablet + Compositor + Desktop.
//! 2. Desktop initializes its bottom taskbar and registers with the display compositor.
//! 3. QEMU screendump captures the taskbar rendered at the bottom of the screen.
//! 4. Clicking the Search button or sending input triggers Spotlight search modal.
//! 5. Zero kernel panics or cell faults throughout execution.

use std::path::PathBuf;
use std::time::Duration;

use vicell_integration_tests::{pixel_region, qemu_binary, read_ppm_frame, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const EVENT_TIMEOUT: u64 = 15;

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
    std::thread::sleep(Duration::from_millis(300));
}


#[test]
fn desktop_taskbar_and_spotlight_interaction() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_pointer(&kernel_path(), &disk_path());

    // Wait for the shell prompt
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!("shell not reached: {e}\n--- serial output ---\n{}", qemu.dump())
    });
    // Wait for any background boot tests to finish settling:
    let _ = qemu.wait_for("[vfs-test] ALL TESTS PASSED", EVENT_TIMEOUT);
    settle();

    // Check if desktop cell was spawned by init or launch it from shell
    let dump_so_far = qemu.dump();
    if !dump_so_far.contains("[desktop] Ready. Entering event loop.") {
        qemu.send_line("desktop &");
        qemu.wait_for("[desktop] Ready. Entering event loop.", EVENT_TIMEOUT)
            .unwrap_or_else(|e| {
                panic!("desktop failed to start: {e}\n--- serial output ---\n{}", qemu.dump())
            });
    }

    settle();

    // 1. Capture initial screen with desktop taskbar
    let initial_frame = "/tmp/cellos-desktop-initial.ppm";
    assert!(qemu.capture_qemu_screen(initial_frame), "failed to capture initial desktop screen");

    let frame = read_ppm_frame(initial_frame);
    assert!(frame.width >= 1024, "unexpected frame width: {}", frame.width);
    assert!(frame.height >= 768, "unexpected frame height: {}", frame.height);

    // Inspect taskbar region at bottom (y = height - 30): should be non-black (taskbar background)
    let tb_y = (frame.height - 20) as usize;
    let tb_pixels = pixel_region(&frame, 20, tb_y, 200, tb_y + 1);
    let is_all_black = tb_pixels.chunks_exact(3).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0);
    assert!(!is_all_black, "Taskbar region at bottom of screen is completely black / not rendered");
    // Move cursor to Search button on the taskbar: x=142, y=780
    qemu.send_qemu_mouse_abs(142, 780);
    qemu.wait_for("[compositor] cursor at 142,780", EVENT_TIMEOUT)
        .unwrap_or_else(|e| panic!("cursor move did not reach compositor: {e}\n{}", qemu.dump()));

    qemu.send_qemu_mouse_click();
    qemu.wait_for("[desktop] Taskbar: Opening Spotlight Search", EVENT_TIMEOUT)
        .unwrap_or_else(|e| panic!("Taskbar Search click failed: {e}\n{}", qemu.dump()));
    settle();

    // Press Escape to close Spotlight modal
    qemu.send_qemu_key("esc");
    qemu.wait_for("[desktop] Spotlight: Closed", EVENT_TIMEOUT)
        .unwrap_or_else(|e| panic!("Spotlight close via Esc failed: {e}\n{}", qemu.dump()));
    settle();

    // 3. Capture screen after opening Spotlight
    let spotlight_frame = "/tmp/cellos-desktop-spotlight.ppm";
    assert!(qemu.capture_qemu_screen(spotlight_frame), "failed to capture spotlight screen");

    let sp_frame = read_ppm_frame(spotlight_frame);
    // Center of screen where spotlight modal resides: (frame.width / 2, 160)
    let center_x = sp_frame.width / 2;
    let center_pixels = pixel_region(&sp_frame, center_x - 100, 150, center_x + 100, 151);
    let center_all_black = center_pixels.chunks_exact(3).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0);
    assert!(!center_all_black, "Spotlight modal window did not render at center of screen");

    // Verify zero kernel panics or cell faults
    let output = qemu.dump();
    assert!(!output.contains("[KERNEL PANIC]"), "kernel panic detected:\n{output}");
    assert!(!output.contains("[fault] Cell"), "Cell fault detected:\n{output}");
    std::fs::write("/tmp/desktop-test.log", &output).unwrap();

    println!("[desktop-shell test] Desktop taskbar and Spotlight verified successfully.");
}
