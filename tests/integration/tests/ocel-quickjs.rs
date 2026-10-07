// SPDX-License-Identifier: MIT
//! Integration test: the vendored QuickJS engine cell (`/bin/ocel-quickjs`).
//!
//! The viewer activates QuickJS only when the document needs scripts.
//! Real control flow, closures, arrays and DOM mutation must execute in a
//! separate Tier 2 address space and reach the rendered document.

use std::path::PathBuf;
use std::time::Duration;

use vicell_integration_tests::{pixel_region, qemu_binary, read_ppm_frame, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const EVENT_TIMEOUT: u64 = 25;

/// Fixture document placed in VIFS1 by both image builders.
const FIXTURE_URL: &str = "file:///data/ocel-js-demo.html";
/// The title the fixture's script computes (see tests/fixtures/ocel-js-demo.html).
const COMPUTED_TITLE: &str = "[ocel] dom title: sum=10 doubled=2, 4, 6";
/// The engine's own start-up check result (see the cell's `SELF_CHECK` script).
const SELF_CHECK: &str = "[ocel-quickjs] self-check 55:2,4,6:accent:0.30";

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

fn require_marker(qemu: &QemuRunner, marker: &str, what: &str) {
    if !qemu.output_contains(marker) {
        panic!(
            "{what}: serial log lacks {marker:?}\n--- serial output ---\n{}",
            qemu.dump()
        );
    }
}

fn assert_region_not_black(frame_path: &str, what: &str, region: [usize; 4]) {
    let [l, t, r, b] = region;
    let pixels = pixel_region(&read_ppm_frame(frame_path), l, t, r, b);
    assert!(
        !pixels
            .chunks_exact(3)
            .all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0),
        "{what}: region {l},{t}..{r},{b} is entirely black (nothing drawn)"
    );
}

#[test]
fn quickjs_engine_cell_runs_real_javascript_in_tier2() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_pointer(&kernel_path(), &disk_path());

    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!("shell not reached: {e}\n--- serial output ---\n{}", qemu.dump())
    });
    let _ = qemu.wait_for("[vfs-test] ALL TESTS PASSED", EVENT_TIMEOUT);
    settle();

    assert!(!qemu.output_contains("[domain] admitted cell 'ocel-quickjs'"),
        "QuickJS must remain unloaded before demand:\n{}", qemu.dump());
    qemu.send_line(&format!("ocel {FIXTURE_URL} &"));
    qemu.wait_for("[ocel] Window initialized and painted.", EVENT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!("Ocel failed to start: {e}\n--- serial output ---\n{}", qemu.dump())
        });
    settle();
    require_marker(&qemu,
        "[domain] admitted cell 'ocel-quickjs' to Tier 2 Paged Domain (SATP isolation)",
        "demand-loaded QuickJS must execute in a private address space");
    require_marker(
        &qemu,
        "[ocel-quickjs] quickjs ",
        "the engine cell must create a runtime and evaluate its DOM prelude",
    );
    require_marker(
        &qemu,
        SELF_CHECK,
        "the engine's start-up self-check must produce its exact expected value",
    );

    require_marker(
        &qemu,
        &format!("[ocel] loaded {FIXTURE_URL} (HTML,"),
        "the viewer must open the fixture",
    );
    require_marker(
        &qemu,
        COMPUTED_TITLE,
        "the loop/closure/map script must compute its value inside the engine",
    );

    // The painted window is evidence the mutation-carrying document is what is
    // on screen, not a stale frame from before the load.
    let frame_path = "/tmp/cellos-ocel-quickjs.ppm";
    assert!(qemu.capture_qemu_screen(frame_path), "failed to capture the screen");
    let frame = read_ppm_frame(frame_path);
    let win_x = (frame.width as i32 - 1024) / 2;
    let win_y = (frame.height as i32 - 680) / 2;
    assert!(win_x >= 0 && win_y >= 0, "window does not fit the frame");
    assert_region_not_black(
        frame_path,
        "document viewport",
        [
            win_x as usize + 20,
            win_y as usize + 120,
            win_x as usize + 900,
            win_y as usize + 121,
        ],
    );

    // ── 6. Containment: nothing faulted, nothing panicked ───────────────────
    let output = qemu.dump();
    assert!(!output.contains("[KERNEL PANIC]"), "kernel panic detected:\n{output}");
    assert!(!output.contains("[fault] Cell"), "Cell fault detected:\n{output}");
    std::fs::write("/tmp/ocel-quickjs-test.log", &output).expect("write serial log");

    println!(
        "[ocel-quickjs test] vendored QuickJS executed in Tier 2, its self-check and the \
         fixture's loop/closure/map script both produced their computed values."
    );
}
