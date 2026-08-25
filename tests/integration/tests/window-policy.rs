//! RV64 QEMU evidence for compositor raise, capture, and keyboard-focus policy.

use std::path::PathBuf;
use std::time::Duration;

use vicell_integration_tests::{pixel_region, qemu_binary, read_ppm_frame, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const OVERLAP_X: usize = 180;
const OVERLAP_Y: usize = 140;
const BACK: [u8; 3] = [0xFF, 0x00, 0x00];
const FRONT: [u8; 3] = [0x00, 0x00, 0xFF];
const BACKGROUND: [u8; 3] = [0x00, 0xFF, 0x00];
const EMPTY: [u8; 3] = [0x00, 0x00, 0x00];
const FRAME: [u8; 3] = [0x2D, 0x34, 0x3B];
const TITLE_INACTIVE: [u8; 3] = [0x42, 0x4D, 0x56];
const TITLE_ACTIVE: [u8; 3] = [0x31, 0x65, 0x83];
const MINIMIZE: [u8; 3] = [0xD6, 0xA4, 0x53];
const MAXIMIZE: [u8; 3] = [0x36, 0xA4, 0x53];
const CLOSE: [u8; 3] = [0xCB, 0x45, 0x45];
const PRIMARY: [u8; 3] = [0xFF, 0x00, 0xFF];
const SILENT: [u8; 3] = [0x00, 0xFF, 0xFF];
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn prerequisites_ok() -> bool {
    let kernel = repo_root().join("target/riscv64gc-unknown-none-elf/release/cellos-kernel");
    let disk = repo_root().join("disk_v3.img");
    vicell_integration_tests::ci_guard(
        kernel.exists()
            && disk.exists()
            && std::process::Command::new(qemu_binary())
                .arg("--version")
                .output()
                .is_ok(),
    )
}

fn overlap_color(path: &str) -> [u8; 3] {
    color_at(path, OVERLAP_X, OVERLAP_Y)
}

fn color_at(path: &str, x: usize, y: usize) -> [u8; 3] {
    let frame = read_ppm_frame(path);
    let pixel = pixel_region(&frame, x, y, x + 1, y + 1);
    [pixel[0], pixel[1], pixel[2]]
}

#[test]
fn clicking_exposed_surface_raises_and_focuses_its_owner() {
    if !prerequisites_ok() {
        return;
    }
    let root = repo_root();
    let kernel = root.join("target/riscv64gc-unknown-none-elf/release/cellos-kernel");
    let disk = root.join("disk_v3.img");
    let mut qemu =
        QemuRunner::boot_with_pointer(&kernel.to_string_lossy(), &disk.to_string_lossy());

    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|error| panic!("shell not reached: {error}\n{}", qemu.dump()));
    qemu.send_line("window-policy-probe background &");
    qemu.wait_for("[window-policy-probe background] ready", 20)
        .unwrap_or_else(|error| panic!("background probe did not start: {error}\n{}", qemu.dump()));
    qemu.send_line("window-policy-probe back &");
    qemu.wait_for("[window-policy-probe back] ready", 20)
        .unwrap_or_else(|error| panic!("back probe did not start: {error}\n{}", qemu.dump()));
    qemu.send_line("window-policy-probe front &");
    qemu.wait_for("[window-policy-probe front] ready", 20)
        .unwrap_or_else(|error| panic!("front probe did not start: {error}\n{}", qemu.dump()));
    qemu.send_line("window-policy-probe wm-primary &");
    qemu.wait_for("[window-policy-probe wm-primary] title set", 20)
        .unwrap_or_else(|error| panic!("primary title was not accepted: {error}\n{}", qemu.dump()));
    qemu.wait_for("[window-policy-probe wm-primary] ready", 20)
        .unwrap_or_else(|error| panic!("primary probe did not start: {error}\n{}", qemu.dump()));
    qemu.send_line("window-policy-probe wm-silent &");
    qemu.wait_for("[window-policy-probe wm-silent] ready", 20)
        .unwrap_or_else(|error| panic!("silent probe did not start: {error}\n{}", qemu.dump()));
    qemu.send_line("window-policy-probe wm-close &");
    qemu.wait_for("[window-policy-probe wm-close] ready", 20)
        .unwrap_or_else(|error| panic!("close probe did not start: {error}\n{}", qemu.dump()));

    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-front.ppm"));
    assert_eq!(
        overlap_color("/tmp/cellos-window-policy-front.ppm"),
        FRONT,
        "later front surface must initially paint the overlap"
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-front.ppm", 79, 140),
        FRAME,
        "unselected interactive surface retains its compositor-owned frame"
    );

    qemu.send_qemu_mouse_abs(100, 100);
    qemu.send_qemu_mouse_button(true);
    qemu.wait_for("[window-policy-probe back] press", 15)
        .unwrap_or_else(|error| panic!("back press not delivered: {error}\n{}", qemu.dump()));
    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-back.ppm"));
    assert_eq!(
        overlap_color("/tmp/cellos-window-policy-back.ppm"),
        BACK,
        "clicking exposed back surface must raise it over front"
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-back.ppm", 100, 70),
        TITLE_ACTIVE,
        "selected back surface must receive an active compositor titlebar"
    );

    qemu.send_qemu_mouse_abs(280, 200);
    qemu.wait_for("[window-policy-probe back] move 200,120", 15)
        .unwrap_or_else(|error| {
            panic!(
                "captured move not delivered to back: {error}\n{}",
                qemu.dump()
            )
        });
    qemu.send_qemu_mouse_button(false);
    qemu.wait_for("[window-policy-probe back] release", 15)
        .unwrap_or_else(|error| {
            panic!(
                "captured release not delivered to back: {error}\n{}",
                qemu.dump()
            )
        });

    qemu.send_qemu_mouse_abs(20, 20);
    qemu.send_qemu_mouse_click();
    std::thread::sleep(Duration::from_millis(100));
    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-background.ppm"));
    assert_eq!(
        overlap_color("/tmp/cellos-window-policy-background.ppm"),
        BACK,
        "non-interactive background must not raise over selected surface"
    );

    qemu.send_qemu_key("a");
    qemu.wait_for("[window-policy-probe back] key", 15)
        .unwrap_or_else(|error| panic!("back key not delivered: {error}\n{}", qemu.dump()));
    std::thread::sleep(Duration::from_millis(100));
    let output = qemu.dump();
    assert!(
        !output.contains("[window-policy-probe front] press")
            && !output.contains("[window-policy-probe front] move")
            && !output.contains("[window-policy-probe front] release")
            && !output.contains("[window-policy-probe front] key")
            && !output.contains("[window-policy-probe background] press")
            && !output.contains("[window-policy-probe background] move")
            && !output.contains("[window-policy-probe background] release")
            && !output.contains("[window-policy-probe background] key"),
        "nonselected or background probe received input:\n{output}"
    );

    qemu.send_qemu_mouse_abs(280, 200);
    qemu.send_qemu_mouse_click();
    qemu.wait_for("[window-policy-probe front] press", 15)
        .unwrap_or_else(|error| panic!("front press not delivered: {error}\n{}", qemu.dump()));
    qemu.wait_for("[window-policy-probe front] release", 15)
        .unwrap_or_else(|error| panic!("front release not delivered: {error}\n{}", qemu.dump()));
    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-front-selected.ppm"));
    assert_eq!(
        color_at("/tmp/cellos-window-policy-front-selected.ppm", 100, 70),
        TITLE_INACTIVE,
        "new selection must deactivate the old back titlebar"
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-front-selected.ppm", 180, 110),
        TITLE_ACTIVE,
        "selected front surface must receive an active compositor titlebar"
    );

    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-decor.ppm"));
    assert_eq!(
        color_at("/tmp/cellos-window-policy-decor.ppm", 397, 110),
        FRAME
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-decor.ppm", 420, 90),
        TITLE_INACTIVE
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-decor.ppm", 518, 84),
        MINIMIZE
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-decor.ppm", 534, 84),
        MAXIMIZE
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-decor.ppm", 550, 84),
        CLOSE
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-decor.ppm", 410, 110),
        PRIMARY
    );

    qemu.send_qemu_mouse_abs(420, 90);
    qemu.send_qemu_mouse_button(true);
    qemu.send_qemu_mouse_abs(460, 130);
    qemu.send_qemu_mouse_button(false);
    std::thread::sleep(Duration::from_millis(100));
    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-drag.ppm"));
    assert_eq!(
        color_at("/tmp/cellos-window-policy-drag.ppm", 450, 150),
        PRIMARY
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-drag.ppm", 410, 110),
        EMPTY
    );
    assert_eq!(
        color_at("/tmp/cellos-window-policy-drag.ppm", 470, 130),
        TITLE_ACTIVE
    );
    assert!(
        !qemu
            .dump()
            .contains("[window-policy-probe wm-primary] press"),
        "titlebar drag must be compositor-owned rather than forwarded to the client"
    );

    qemu.send_qemu_mouse_abs(602, 302);
    qemu.send_qemu_mouse_button(true);
    qemu.send_qemu_mouse_abs(622, 322);
    qemu.send_qemu_mouse_button(false);
    qemu.wait_for("[window-policy-probe wm-primary] configure Resize", 15)
        .unwrap_or_else(|error| panic!("resize configure missing: {error}\n{}", qemu.dump()));
    qemu.wait_for("[window-policy-probe wm-primary] configured serial", 15)
        .unwrap_or_else(|error| panic!("resize did not commit: {error}\n{}", qemu.dump()));

    qemu.send_qemu_mouse_abs(598, 130);
    qemu.send_qemu_mouse_click();
    qemu.wait_for("[window-policy-probe wm-primary] configure Maximize", 15)
        .unwrap_or_else(|error| panic!("maximize configure missing: {error}\n{}", qemu.dump()));
    qemu.wait_for("[window-policy-probe wm-primary] state Maximized", 15)
        .unwrap_or_else(|error| panic!("maximize state missing: {error}\n{}", qemu.dump()));
    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-maximize.ppm"));
    assert_eq!(
        color_at("/tmp/cellos-window-policy-maximize.ppm", 10, 30),
        PRIMARY
    );

    qemu.send_qemu_mouse_abs(1254, 10);
    qemu.send_qemu_mouse_click();
    qemu.wait_for("[window-policy-probe wm-primary] configure Restore", 15)
        .unwrap_or_else(|error| panic!("restore configure missing: {error}\n{}", qemu.dump()));
    qemu.wait_for("[window-policy-probe wm-primary] state Normal", 15)
        .unwrap_or_else(|error| panic!("restore state missing: {error}\n{}", qemu.dump()));

    qemu.send_qemu_mouse_abs(582, 130);
    qemu.send_qemu_mouse_click();
    qemu.wait_for("[window-policy-probe wm-primary] state Minimized", 15)
        .unwrap_or_else(|error| panic!("minimize state missing: {error}\n{}", qemu.dump()));
    qemu.wait_for("[window-policy-probe wm-primary] restore request", 15)
        .unwrap_or_else(|error| {
            panic!("primary did not request restore: {error}\n{}", qemu.dump())
        });
    std::thread::sleep(Duration::from_millis(100));
    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-minimize-restore.ppm"));
    assert_eq!(
        color_at("/tmp/cellos-window-policy-minimize-restore.ppm", 450, 150),
        PRIMARY
    );

    qemu.send_qemu_mouse_abs(562, 462);
    qemu.send_qemu_mouse_button(true);
    qemu.send_qemu_mouse_abs(582, 475);
    qemu.send_qemu_mouse_button(false);
    qemu.wait_for("[window-policy-probe wm-silent] configure Resize", 15)
        .unwrap_or_else(|error| panic!("silent configure missing: {error}\n{}", qemu.dump()));
    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-silent.ppm"));
    assert_eq!(
        color_at("/tmp/cellos-window-policy-silent.ppm", 550, 450),
        SILENT
    );
    assert!(
        !qemu
            .dump()
            .contains("[window-policy-probe wm-silent] configured serial"),
        "silent owner must not commit a configure without applying its replacement Grant"
    );

    for expected in ["reject", "accept"] {
        qemu.send_qemu_mouse_abs(754, 90);
        qemu.send_qemu_mouse_click();
        qemu.wait_for(
            &format!("[window-policy-probe wm-close] close {expected}"),
            15,
        )
        .unwrap_or_else(|error| panic!("close {expected} missing: {error}\n{}", qemu.dump()));
    }
    qemu.wait_for("[window-policy-probe wm-close] destroy", 15)
        .unwrap_or_else(|error| panic!("accepted close did not destroy: {error}\n{}", qemu.dump()));
}
