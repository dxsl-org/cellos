//! RV64 QEMU evidence for compositor raise, capture, and keyboard-focus policy.

use std::path::PathBuf;
use std::time::Duration;

use vicell_integration_tests::{pixel_region, qemu_binary, read_ppm_frame, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const OVERLAP_X: usize = 180;
const OVERLAP_Y: usize = 140;
const BACK: [u8; 3] = [0xFF, 0x00, 0x00];
const FRONT: [u8; 3] = [0x00, 0x00, 0xFF];

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
    let frame = read_ppm_frame(path);
    let pixel = pixel_region(&frame, OVERLAP_X, OVERLAP_Y, OVERLAP_X + 1, OVERLAP_Y + 1);
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

    assert!(qemu.capture_qemu_screen("/tmp/cellos-window-policy-front.ppm"));
    assert_eq!(
        overlap_color("/tmp/cellos-window-policy-front.ppm"),
        FRONT,
        "later front surface must initially paint the overlap"
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

    qemu.send_qemu_mouse_abs(280, 200);
    qemu.wait_for("[window-policy-probe back] move 200,120", 15)
        .unwrap_or_else(|error| panic!("captured move not delivered to back: {error}\n{}", qemu.dump()));
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
}
