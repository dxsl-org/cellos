//! Integration test: compositor cursor, scanout, and ViUI pointer delivery.
//!
//! Boots QEMU with a VirtIO GPU + tablet, verifies cursor motion reaches the
//! compositor, launches the dashboard, captures a non-black scanout, and clicks
//! a real ViUI button through the compositor's surface-local pointer route.
//!
//! Prerequisites: qemu-system-riscv64 on PATH, kernel + disk built.
//! Gracefully skips when any prerequisite is missing.

use std::path::PathBuf;
use std::time::Duration;

use vicell_integration_tests::{pixel_region, qemu_binary, read_ppm_frame, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;

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
    if !kernel_exists {
        eprintln!(
            "SKIP compositor-cursor: kernel not built ({})",
            kernel_path()
        );
    }
    if !disk_exists {
        eprintln!("SKIP compositor-cursor: disk_v3.img missing — run ./gen_disk.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP compositor-cursor: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_exists && disk_exists && qemu_ok)
}

/// End-to-end cursor move test.
///
/// Data flow:
///   QMP abs event → QEMU virtio-tablet → kernel virtio_input (EV_ABS, opcode 2)
///   → input service apply_abs → MouseMove{x,y}
///   → compositor update_cursor → "[compositor] cursor at X,Y"
///
/// The QEMU abs coordinate 16383 maps to roughly the centre of the 32767-range
/// (display-independent). Any non-zero position is sufficient to assert the
/// cursor moved from its initial (0,0) position.
#[test]
fn compositor_cursor_moves_on_mouse_event() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_pointer(&kernel_path(), &disk_path());

    // Wait for the shell prompt — full userspace stack is up by this point.
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell not reached: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });

    // Wait for the compositor to print its startup banner.
    qemu.wait_for("[compositor] Compositor v0.2", 15)
        .unwrap_or_else(|e| {
            panic!(
                "compositor did not start: {e}\n--- serial output ---\n{}",
                qemu.dump()
            )
        });

    // Give the compositor time to settle its input-focus registration loop.
    std::thread::sleep(Duration::from_millis(400));

    // Inject an absolute pointer move to near-centre of the QEMU logical range
    // (0..32767). Sending both axes in one call avoids split-event coalescing.
    // The VirtIO input ring is polled on the 10 ms timer tick; allow 15 s.
    qemu.send_qemu_mouse_abs(16383, 16383);

    // End-to-end: tablet EV_ABS → input service virtqueue drain → MouseMove
    // routed to the compositor (dispatch_mouse) → update_cursor probe.
    // The input service does not log per-event (it would bury the shell
    // prompt), so the compositor probe is the first observable marker.
    qemu.wait_for("[compositor] cursor at ", 15)
        .unwrap_or_else(|e| {
            panic!(
                "compositor cursor probe not seen: {e}\n\
             Hint: verify virtio-tablet-device is attached, the input service\n\
             claims ALL virtio-input devices, and dispatch_mouse routes to the\n\
             compositor (update_cursor emits the probe).\n\
             --- serial output ---\n{}",
                qemu.dump()
            )
        });

    // Verify the cursor moved from the initial (0,0) position — the reported
    // coords must contain at least one non-zero value.
    let output = qemu.dump();
    let probe_line = output
        .lines()
        .find(|l| l.contains("[compositor] cursor at "))
        .expect("cursor probe line must be present after wait_for succeeded");

    // Coords follow "cursor at " — format is "X,Y".
    let coords_str = probe_line
        .split("[compositor] cursor at ")
        .nth(1)
        .unwrap_or("")
        .trim();
    let not_origin = coords_str != "0,0";
    assert!(
        not_origin,
        "cursor stayed at origin — EV_ABS event may not have reached compositor (coords={coords_str:?})"
    );

    eprintln!("[test] cursor probe: {:?}", probe_line);

    // Pointer delivery must cross compositor hit-testing and invoke a real ViUI button.
    qemu.send_line("robot-dashboard &");
    qemu.wait_for("[robot-dashboard] input focus granted", 20)
        .unwrap_or_else(|error| panic!("dashboard did not start: {error}\n{}", qemu.dump()));
    std::thread::sleep(Duration::from_millis(500));

    // The dashboard's split layout places STOP inside this surface-local point.
    qemu.send_qemu_mouse_abs(440, 124);
    qemu.wait_for("[compositor] cursor at 440,124", 15)
        .unwrap_or_else(|error| {
            panic!("pointer did not reach compositor: {error}\n{}", qemu.dump())
        });
    assert!(
        qemu.capture_qemu_screen("/tmp/cellos-dashboard-before.ppm"),
        "QMP screen capture unavailable"
    );
    let before_frame = read_ppm_frame("/tmp/cellos-dashboard-before.ppm");
    assert!(
        before_frame.pixels.iter().any(|pixel| *pixel != 0),
        "dashboard scanout remained entirely black"
    );
    // The status label is below the STOP button. Simulation only repaints the
    // sensor and event-log regions, so this crop causally observes STOPPED.
    let before_status = pixel_region(&before_frame, 408, 144, 500, 165);

    qemu.send_qemu_mouse_click();
    qemu.wait_for("[robot-dashboard] pointer press received", 15)
        .unwrap_or_else(|error| {
            panic!("pointer press did not reach ViUI: {error}\n{}", qemu.dump())
        });
    qemu.wait_for("[robot-dashboard] STOP clicked", 15)
        .unwrap_or_else(|error| {
            panic!(
                "ViUI button click was not delivered: {error}\n{}",
                qemu.dump()
            )
        });
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        qemu.capture_qemu_screen("/tmp/cellos-dashboard-after.ppm"),
        "QMP screen capture unavailable"
    );
    let after_frame = read_ppm_frame("/tmp/cellos-dashboard-after.ppm");
    assert!(
        after_frame.pixels.iter().any(|pixel| *pixel != 0),
        "dashboard scanout became entirely black after click"
    );
    let after_status = pixel_region(&after_frame, 408, 144, 500, 165);
    assert_ne!(
        before_status, after_status,
        "STOP click did not repaint the visible dashboard status label"
    );
}
