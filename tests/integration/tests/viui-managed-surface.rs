//! RV64 QEMU oracle for generated ViUI content on a managed surface.

use std::path::PathBuf;
use std::time::Duration;

use vicell_integration_tests::{pixel_region, qemu_binary, read_ppm_frame, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const EVENT_TIMEOUT: u64 = 20;
const INITIAL_FRAME: &str = "/tmp/cellos-viui-initial.ppm";
const CLICKED_FRAME: &str = "/tmp/cellos-viui-clicked.ppm";
const MAXIMIZED_FRAME: &str = "/tmp/cellos-viui-maximized.ppm";
const RESTORED_FRAME: &str = "/tmp/cellos-viui-restored.ppm";

// The content origin is (80, 80). Generated layout adds 16px padding;
// Inter's measured 16px line box and the 8px gap precede the button.
const LABEL_REGION: (usize, usize, usize, usize) = (96, 96, 176, 116);
const BUTTON_CLICK: (u32, u32) = (100, 128);
// Window operations go through the Cell Tab's window menu (ADR-0027: the tab holds
// only the app icon and the title). Geometry as in window-policy.rs: the icon sits
// at content + (13, -15), menu items are 28px tall from content y + 3 (Maximize or
// Restore first, Close after Minimize and a 9px separator), x = icon x + 47. When
// maximized the content origin is (1, 28).
const MENU_ICON: (u32, u32) = (93, 65);
const MENU_MAXIMIZE: (u32, u32) = (140, 97);
const MENU_CLOSE: (u32, u32) = (140, 162);
const MAXIMIZED_MENU_ICON: (u32, u32) = (14, 13);
const MAXIMIZED_MENU_RESTORE: (u32, u32) = (60, 45);
// Inside the button's padding, just in from its bottom-left focus border (the label
// text starts a few pixels to the right). Maximized moves the button by (-79, -52).
const MAXIMIZED_BUTTON_SAMPLE: (usize, usize) = (21, 97);
const RESTORED_BUTTON_SAMPLE: (usize, usize) = (100, 149);
// The default light theme's button fill, plain and hovered.
const BUTTON_NORMAL: [u8; 3] = [230, 232, 238];
const BUTTON_HOVER: [u8; 3] = [218, 222, 230];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn prerequisites_ok() -> bool {
    let root = repo_root();
    vicell_integration_tests::ci_guard(
        root.join("target/riscv64gc-unknown-none-elf/release/cellos-kernel")
            .exists()
            && root.join("disk_v3.img").exists()
            && std::process::Command::new(qemu_binary())
                .arg("--version")
                .output()
                .is_ok(),
    )
}

fn click(qemu: &mut QemuRunner, point: (u32, u32)) {
    qemu.send_qemu_mouse_abs(point.0, point.1);
    qemu.send_qemu_mouse_click();
}

/// Open the window menu from the tab icon at `icon`, then choose the item at `item`.
fn window_menu(qemu: &mut QemuRunner, icon: (u32, u32), item: (u32, u32)) {
    click(qemu, icon);
    std::thread::sleep(Duration::from_millis(200));
    click(qemu, item);
}

fn capture_region(qemu: &mut QemuRunner, path: &str) -> Vec<u8> {
    assert!(qemu.capture_qemu_screen(path), "screen capture failed");
    let frame = read_ppm_frame(path);
    pixel_region(
        &frame,
        LABEL_REGION.0,
        LABEL_REGION.1,
        LABEL_REGION.2,
        LABEL_REGION.3,
    )
}

fn color_at(path: &str, point: (usize, usize)) -> [u8; 3] {
    let frame = read_ppm_frame(path);
    let pixel = pixel_region(&frame, point.0, point.1, point.0 + 1, point.1 + 1);
    [pixel[0], pixel[1], pixel[2]]
}

fn assert_button_background(path: &str, point: (usize, usize), state: &str) {
    let color = color_at(path, point);
    assert!(
        color == BUTTON_NORMAL || color == BUTTON_HOVER,
        "{state} button geometry missing at {point:?}: found {color:?}"
    );
}

fn settle() {
    std::thread::sleep(Duration::from_millis(250));
}

#[test]
fn generated_counter_survives_managed_surface_configure() {
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
    qemu.send_line("viui-demo &");
    qemu.wait_for("[viui-demo] managed surface ready count=0", EVENT_TIMEOUT)
        .unwrap_or_else(|error| panic!("managed Counter did not render: {error}\n{}", qemu.dump()));
    settle();

    let initial_label = capture_region(&mut qemu, INITIAL_FRAME);
    click(&mut qemu, BUTTON_CLICK);
    qemu.wait_for("[viui-demo] count=1", EVENT_TIMEOUT)
        .unwrap_or_else(|error| panic!("pointer activation failed: {error}\n{}", qemu.dump()));
    settle();
    let clicked_label = capture_region(&mut qemu, CLICKED_FRAME);
    assert_ne!(
        initial_label, clicked_label,
        "generated Counter label did not repaint after pointer activation"
    );

    window_menu(&mut qemu, MENU_ICON, MENU_MAXIMIZE);
    settle();
    assert!(qemu.capture_qemu_screen(MAXIMIZED_FRAME));
    // A compositor-owned control move may leave the widget's hover state set.
    assert_button_background(MAXIMIZED_FRAME, MAXIMIZED_BUTTON_SAMPLE, "maximized");

    window_menu(&mut qemu, MAXIMIZED_MENU_ICON, MAXIMIZED_MENU_RESTORE);
    settle();
    assert!(qemu.capture_qemu_screen(RESTORED_FRAME));
    assert_button_background(RESTORED_FRAME, RESTORED_BUTTON_SAMPLE, "restored");

    window_menu(&mut qemu, MENU_ICON, MENU_CLOSE);
    qemu.wait_for("[viui-demo] close request accepted", EVENT_TIMEOUT)
        .unwrap_or_else(|error| {
            panic!("accepted close did not shut down: {error}\n{}", qemu.dump())
        });

    let closed_output = qemu.dump();
    assert!(
        !closed_output.contains("[KERNEL PANIC]"),
        "kernel panic:\n{closed_output}"
    );
    assert!(
        !closed_output.contains("[fault] Cell"),
        "Cell fault:\n{closed_output}"
    );
    drop(qemu);

    let mut qemu =
        QemuRunner::boot_with_pointer(&kernel.to_string_lossy(), &disk.to_string_lossy());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|error| panic!("second shell not reached: {error}\n{}", qemu.dump()));
    qemu.send_line("viui-demo &");
    qemu.wait_for("[viui-demo] managed surface ready count=0", EVENT_TIMEOUT)
        .unwrap_or_else(|error| {
            panic!(
                "second managed Counter did not render: {error}\n{}",
                qemu.dump()
            )
        });
    settle();
    click(&mut qemu, BUTTON_CLICK);
    qemu.wait_for("[viui-demo] count=1", EVENT_TIMEOUT)
        .unwrap_or_else(|error| {
            panic!("second pointer activation failed: {error}\n{}", qemu.dump())
        });
    window_menu(&mut qemu, MENU_ICON, MENU_MAXIMIZE);
    settle();
    window_menu(&mut qemu, MAXIMIZED_MENU_ICON, MAXIMIZED_MENU_RESTORE);
    settle();
    qemu.send_qemu_key("ret");
    qemu.wait_for("[viui-demo] key=Enter", EVENT_TIMEOUT)
        .unwrap_or_else(|error| {
            panic!(
                "keyboard activation delivery failed: {error}\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for("[viui-demo] count=2", EVENT_TIMEOUT)
        .unwrap_or_else(|error| {
            panic!(
                "keyboard activation after restore failed: {error}\n{}",
                qemu.dump()
            )
        });

    let output = qemu.dump();
    assert!(
        !output.contains("[KERNEL PANIC]"),
        "kernel panic:\n{output}"
    );
    assert!(!output.contains("[fault] Cell"), "Cell fault:\n{output}");
}
