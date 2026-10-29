// SPDX-License-Identifier: MIT
//! Integration test: CellOS Desktop x86 QEMU runner.
//!
//! Materializes the planned x86 desktop runner contract from
//! `.agents/261019-1310-unified-desktop/verification.md`:
//!
//! Verifies:
//! 1. ISO image selection via `CELLOS_DESKTOP_ISO` (resolving relative paths against repo root)
//!    or canonical `build/cellos-x86_64.iso` (fails closed if absent).
//! 2. Honors `CELLOS_DESKTOP_ACCEL` ("tcg" or "kvm"; rejects invalid strings).
//! 3. Boots x86_64 QEMU q35 with Limine BIOS, VirtIO tablet, and active QMP monitor.
//! 4. Exercises desktop pointer routing via QMP absolute mouse coordinates and asserts acceptance.
//! 5. Proves end-to-end desktop rendering by diffing pixels specifically in the target cursor region (635..656, 395..416).
//! 6. Kernel reaches display compositor and fb-console publication.
//! 7. Interactive shell reaches "Cellos > ".
//! 8. Exercises interactive command execution and verifies fresh post-command prompt.
//! 9. Zero kernel panics or cell faults throughout execution.

use std::path::{Path, PathBuf};
use vicell_integration_tests::{qemu_binary_x86, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const CMD_TIMEOUT: u64 = 15;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn desktop_iso_path() -> PathBuf {
    if let Ok(path) = std::env::var("CELLOS_DESKTOP_ISO") {
        if !path.is_empty() {
            let p = PathBuf::from(path);
            if p.is_relative() {
                return repo_root().join(p);
            }
            return p;
        }
    }
    repo_root().join("build/cellos-x86_64.iso")
}

fn assert_prerequisites(iso: &Path) {
    assert!(
        iso.exists(),
        "desktop-x86: canonical desktop ISO not found at {}. Build with: bash scripts/build-cloud-images.sh --ocel-profile desktop --javascript",
        iso.display()
    );
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    assert!(
        qemu_ok,
        "desktop-x86: qemu-system-x86_64 must be installed on PATH"
    );
}

#[test]
fn desktop_x86_boots_to_shell_and_compositor() {
    let iso = desktop_iso_path();
    assert_prerequisites(&iso);

    let accel = std::env::var("CELLOS_DESKTOP_ACCEL").unwrap_or_else(|_| "tcg".to_string());
    let mut qemu = QemuRunner::boot_x86_bios_with_accel(&iso.to_string_lossy(), &accel);

    // Wait for the initial interactive prompt
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "desktop-x86: shell prompt not reached within {BOOT_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // Verify critical desktop milestones
    let boot_output = qemu.dump();
    assert!(
        boot_output.contains("[Cellos] kernel boot"),
        "desktop-x86: kernel boot marker missing"
    );
    assert!(
        boot_output.contains("[compositor] Compositor v0.2") || boot_output.contains("Init: compositor launched"),
        "desktop-x86: compositor service publication missing"
    );
    assert!(
        boot_output.contains("[fb-console] background surface created") || boot_output.contains("[fb-console] initial damage submitted"),
        "desktop-x86: fb-console background surface publication missing"
    );

    // Capture initial desktop framebuffer state
    // Captures go to the system temp directory, never into the repository's build/.
    let capture_dir = std::env::temp_dir();
    let init_capture_path =
        capture_dir.join(format!("cellos-desktop-x86-{}-initial.ppm", std::process::id()));
    let init_str = init_capture_path.to_string_lossy().to_string();
    assert!(
        qemu.capture_qemu_screen(&init_str) && init_capture_path.exists(),
        "desktop-x86: failed to capture initial framebuffer state via QMP"
    );
    let initial_ppm = std::fs::read(&init_capture_path).expect("read initial PPM");
    assert!(
        initial_ppm.len() >= 1024 && initial_ppm.starts_with(b"P6\n"),
        "desktop-x86: initial capture is not a valid PPM image"
    );

    // Exercise real desktop pointer routing via QMP and verify command acceptance
    let moved_ok = qemu.send_qemu_mouse_abs(16384, 16384);
    assert!(
        moved_ok,
        "desktop-x86: QMP absolute pointer event was rejected by QEMU monitor"
    );
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Capture moved desktop framebuffer and prove compositor repainted cursor at new coordinates
    let moved_capture_path =
        capture_dir.join(format!("cellos-desktop-x86-{}-moved.ppm", std::process::id()));
    let moved_str = moved_capture_path.to_string_lossy().to_string();
    assert!(
        qemu.capture_qemu_screen(&moved_str) && moved_capture_path.exists(),
        "desktop-x86: failed to capture moved framebuffer state via QMP"
    );
    let moved_ppm = std::fs::read(&moved_capture_path).expect("read moved PPM");
    assert!(
        moved_ppm.len() == initial_ppm.len(),
        "desktop-x86: framebuffer capture size changed unexpectedly"
    );

    // Verify that pixels changed specifically within the expected center cursor region
    // (x=635..656, y=395..416), strictly excluding unrelated taskbar/clock updates.
    let width = 1280usize;
    let height = 800usize;
    let header_offset = moved_ppm.len() - (width * height * 3);
    let mut center_cursor_pixels_changed = 0;
    for y in 395..416 {
        for x in 635..656 {
            let offset = header_offset + (y * width + x) * 3;
            if initial_ppm[offset..offset + 3] != moved_ppm[offset..offset + 3] {
                center_cursor_pixels_changed += 1;
            }
        }
    }
    assert!(
        center_cursor_pixels_changed > 0,
        "desktop-x86: pointer move did not update rendered cursor in expected center region x=635..656, y=395..416 (0 cursor pixels changed)"
    );

    let _ = std::fs::remove_file(&init_capture_path);
    let _ = std::fs::remove_file(&moved_capture_path);

    // Exercise real desktop keyboard input via QMP and verify command execution
    std::thread::sleep(std::time::Duration::from_millis(300));
    let pre_cmd_len = qemu.dump().len();
    let keys_ok = qemu.send_qemu_keys(&["h", "e", "l", "p", "ret"]);
    assert!(
        keys_ok,
        "desktop-x86: QMP keyboard event sequence was rejected by QEMU monitor"
    );

    let start = std::time::Instant::now();
    let mut fresh_prompt_seen = false;
    while start.elapsed() < std::time::Duration::from_secs(CMD_TIMEOUT) {
        let current = qemu.dump();
        if current.len() > pre_cmd_len {
            let new_slice = &current[pre_cmd_len..];
            if new_slice.contains("Cellos >") && (new_slice.contains("help") || new_slice.contains("Syntax")) {
                fresh_prompt_seen = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    assert!(
        fresh_prompt_seen,
        "desktop-x86: QMP keyboard 'help' command did not return a fresh prompt within {CMD_TIMEOUT}s\n--- recent output ---\n{}",
        &qemu.dump()[pre_cmd_len..]
    );
    // Verify zero kernel panics or cell faults in final output
    let final_output = qemu.dump();
    assert!(
        !final_output.contains("KERNEL PANIC"),
        "desktop-x86: kernel panic detected in execution output"
    );
    assert!(
        !final_output.contains("[fault] Cell"),
        "desktop-x86: cell fault detected in execution output"
    );
}
