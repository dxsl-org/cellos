// SPDX-License-Identifier: MIT
//! Integration test: the Ocel viewer, its Tier 2 `ocel-js` script service, and
//! the viewer's own input paths, driven end to end in QEMU (riscv64).
//!
//! What each assertion is evidence for:
//!
//! 1. `[domain] admitted cell 'ocel-quickjs' … (SATP isolation)` and
//!    `Init: registered OCEL_JS on …` — `init` spawns the engine cell at boot
//!    and the kernel admits it with private page tables, so an engine fault
//!    cannot reach the viewer's address space. The lane accepts either engine
//!    cell: which one an image ships is an image decision, and the engine's own
//!    identity is the `ocel-quickjs` lane's subject.
//! 2. `[ocel] loaded file:///data/ocel-js-demo.html (HTML, …)` — the viewer
//!    opened the document named on its command line (`ocel <url>`), parsed it,
//!    and built layout nodes for it.
//! 3. `[ocel] js backend: Tier 2 domain service` — the inline script in that
//!    fixture was executed by the Tier 2 service, not by the in-process
//!    fallback engine (the test also asserts the fallback line is absent).
//! 4. `[ocel] dom title: …` — the mutation the service returned
//!    crossed the IPC boundary and was applied to the document.
//! 5. `[ocel] search "<q>": N match(es)` after QMP keystrokes — the keyboard
//!    path (input service → focused cell) reaches the viewer, and in-document
//!    search finds the fixture's own text.
//! 6. `[ocel] tab 2 active: …` after a QMP click on the tab bar's `[+]` — the
//!    compositor routes pointer input to the viewer and its hit-testing runs.
//!
//! Everything is read from the serial log or the framebuffer; nothing here
//! re-states a value the test itself supplied, except the document path and the
//! search term, both of which are the inputs under test.

use std::path::PathBuf;
use std::time::Duration;

use vicell_integration_tests::{pixel_region, qemu_binary, read_ppm_frame, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const EVENT_TIMEOUT: u64 = 20;

/// The document fixture the lane loads. `scripts/gen-disk-ci.sh` and
/// `gen_disk.ps1` place it in VIFS1 at this path.
const FIXTURE_URL: &str = "file:///data/ocel-js-demo.html";
/// The fixture body contains this word; the search assertion counts its matches.
const FIXTURE_TERM: &str = "cellos";

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

/// The image this lane boots. `CELLOS_OCEL_DISK` lets the statement-matcher
/// step in `boot-suite` point at the image assembled without the QuickJS cell
/// (`CELLOS_NO_OCEL_QUICKJS=1`), so both engine picker outcomes are witnessed
/// against the same lane instead of the missing one rotting.
fn disk_path() -> String {
    std::env::var("CELLOS_OCEL_DISK").unwrap_or_else(|_| {
        repo_root()
            .join("disk_v3.img")
            .to_string_lossy()
            .into_owned()
    })
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

/// Serial marker that must be present once the shell is up; the failure message
/// carries the whole log so a missing marker is diagnosable from CI output.
fn require_marker(qemu: &QemuRunner, marker: &str, what: &str) {
    if !qemu.output_contains(marker) {
        panic!(
            "{what}: serial log lacks {marker:?}\n--- serial output ---\n{}",
            qemu.dump()
        );
    }
}

/// Whichever of `markers` appears in the log (the engine cell an image ships is
/// an image decision, so a lane that is not about the engine itself must accept
/// either engine cell).
fn require_any_marker(qemu: &QemuRunner, markers: &[&str], what: &str) {
    if markers.iter().any(|m| qemu.output_contains(m)) {
        return;
    }
    panic!(
        "{what}: serial log lacks all of {markers:?}\n--- serial output ---\n{}",
        qemu.dump()
    );
}

/// The integer that follows `prefix` in the last occurrence of `prefix`.
fn count_after(output: &str, prefix: &str) -> Option<usize> {
    let at = output.rfind(prefix)?;
    let rest = &output[at + prefix.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn assert_region_not_black(qemu: &QemuRunner, frame_path: &str, what: &str, region: [usize; 4]) {
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
fn ocel_viewer_runs_scripts_in_tier2_and_answers_input() {
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

    // ── 1. A Tier 2 engine cell is up before the viewer starts ──────────────
    require_any_marker(
        &qemu,
        &[
            "[domain] admitted cell 'ocel-quickjs' to Tier 2 Paged Domain (SATP isolation)",
            "[domain] admitted cell 'ocel-js' to Tier 2 Paged Domain (SATP isolation)",
        ],
        "an Ocel engine cell must be admitted to Tier 2 with private page tables",
    );
    require_marker(
        &qemu,
        "Init: registered OCEL_JS on ",
        "init must spawn an engine cell and register service::OCEL_JS",
    );
    // An image assembled for one engine must have picked that engine: without
    // this, a builder that silently dropped the cell would still pass every
    // other assertion in every lane.
    if let Ok(expected) = std::env::var("CELLOS_EXPECT_ENGINE_CELL") {
        let marker = format!("Init: registered OCEL_JS on {expected}");
        if !qemu.output_contains(&marker) {
            panic!(
                "expected the image to register {expected} as the Ocel engine:\n{}",
                qemu.dump()
            );
        }
        let other = if expected == "/bin/ocel-quickjs" {
            "/bin/ocel-js"
        } else {
            "/bin/ocel-quickjs"
        };
        assert!(
            !qemu.output_contains(&format!("Init: registered OCEL_JS on {other}")),
            "the image registered {other} instead of {expected}:\n{}",
            qemu.dump()
        );
    }

    // ── 2-4. Load the script fixture by argument and run its script ──────────
    qemu.send_line(&format!("ocel {FIXTURE_URL} &"));
    qemu.wait_for("[ocel] Window initialized and painted.", EVENT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!("Ocel failed to start: {e}\n--- serial output ---\n{}", qemu.dump())
        });
    settle();

    require_marker(
        &qemu,
        &format!("[ocel] loaded {FIXTURE_URL} (HTML,"),
        "the viewer must open the document named on its command line",
    );
    require_marker(
        &qemu,
        "[ocel] js backend: Tier 2 domain service",
        "the fixture's script must be served by the Tier 2 engine cell",
    );
    // Which *engine* answered is the other lane's subject (`ocel-quickjs`): this
    // one only requires that the mutation crossed the boundary and landed.
    require_marker(
        &qemu,
        "[ocel] dom title: ",
        "the mutation returned by the Tier 2 engine must reach the document",
    );
    assert!(
        !qemu.output_contains("[ocel] js backend: in-process fallback"),
        "the viewer fell back to the in-process engine:\n{}",
        qemu.dump()
    );

    // ── 5. Framebuffer: toolbar and document viewport both painted ──────────
    let frame_path = "/tmp/cellos-ocel-render.ppm";
    assert!(qemu.capture_qemu_screen(frame_path), "failed to capture Ocel screen");
    let frame = read_ppm_frame(frame_path);
    assert!(frame.width >= 1024, "unexpected frame width: {}", frame.width);
    assert!(frame.height >= 768, "unexpected frame height: {}", frame.height);
    // The window is centered: (screen - window) / 2 with a 1024x680 window.
    let win_x = (frame.width as i32 - 1024) / 2;
    let win_y = (frame.height as i32 - 680) / 2;
    assert!(win_x >= 0 && win_y >= 0, "window does not fit the frame");
    let wx = win_x as usize;
    let wy = win_y as usize;
    assert_region_not_black(&qemu, frame_path, "toolbar", [wx + 10, wy + 20, wx + 300, wy + 21]);
    assert_region_not_black(&qemu, frame_path, "tab bar", [wx + 10, wy + 50, wx + 200, wy + 51]);
    assert_region_not_black(
        &qemu,
        frame_path,
        "document viewport",
        [wx + 20, wy + 120, wx + 900, wy + 121],
    );

    // ── 6. Keyboard: F3 search finds the fixture's own text ─────────────────
    qemu.send_qemu_key("f3");
    settle();
    for key in ["c", "e", "l", "l", "o", "s"] {
        qemu.send_qemu_key(key);
    }
    let marker = format!("[ocel] search \"{FIXTURE_TERM}\": ");
    qemu.wait_for(&marker, EVENT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "F3 search did not reach the viewer: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });
    let matches = count_after(&qemu.dump(), &marker)
        .unwrap_or_else(|| panic!("search marker has no match count:\n{}", qemu.dump()));
    assert!(
        matches >= 1,
        "search for {:?} in the fixture reported {matches} matches:\n{}",
        FIXTURE_TERM,
        qemu.dump()
    );
    qemu.send_qemu_key("ret");
    settle();
    require_marker(&qemu, "[ocel] search next: ", "Enter must cycle to the next match");

    // ── 7. Pointer: click the tab bar's [+] button ──────────────────────────
    // Window-local (166, 58) is inside the [+] hit box (plus_x = 156, y = 44..72).
    // The virtio-tablet path passes the injected coordinates through unscaled
    // (cells/services/input/src/mouse_state.rs `apply_abs`), and the compositor
    // routes pointer events in those same units, so the value to inject is the
    // screen pixel of the target — the same convention the ViUI pointer lanes use.
    qemu.send_qemu_mouse_abs((wx + 166) as u32, (wy + 58) as u32);
    settle();
    qemu.send_qemu_mouse_click();
    qemu.wait_for("[ocel] tab 2 active: ", EVENT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "clicking [+] did not open a tab: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });

    // ── 8. No cell faults or kernel panics anywhere in the run ──────────────
    let output = qemu.dump();
    assert!(!output.contains("[KERNEL PANIC]"), "kernel panic detected:\n{output}");
    assert!(!output.contains("[fault] Cell"), "Cell fault detected:\n{output}");
    std::fs::write("/tmp/ocel-test.log", &output).expect("write serial log");

    println!(
        "[ocel-browser test] the Tier 2 engine cell served the fixture script, search found \
         {matches} match(es) for {FIXTURE_TERM:?}, and the tab bar answered a click."
    );
}
