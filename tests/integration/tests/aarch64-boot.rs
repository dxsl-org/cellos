//! AArch64 full-boot integration tests.
//!
//! Mirrors the RISC-V `boot.rs` suite for the ARM64 virt machine.
//!
//! Prerequisites:
//!   - `qemu-system-aarch64` on PATH (or in the Windows default install path)
//!   - Kernel built: `RUSTFLAGS="-C relocation-model=pic" cargo build --release
//!                    --target aarch64-unknown-none-softfloat -p vicell-kernel`
//!   - Disk image: `disk_arm_virt.img` at repo root (built by `format-disk-arm.ps1`
//!                 or by `tools/mkfat32.py`)
//!
//! Tests skip gracefully when any prerequisite is absent — CI behaviour is
//! identical to the RISC-V suite.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_aarch64, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
const CMD_TIMEOUT: u64 = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/aarch64-unknown-none-softfloat/release/vicell-kernel")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root()
        .join("disk_arm_virt.img")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel_exists = PathBuf::from(kernel_path()).exists();
    let disk_exists = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_aarch64())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_exists {
        eprintln!("SKIP aarch64: kernel not built ({})", kernel_path());
    }
    if !disk_exists {
        eprintln!("SKIP aarch64: disk_arm_virt.img missing — run .\\format-disk-arm.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP aarch64: qemu-system-aarch64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_exists && disk_exists && qemu_ok)
}

/// The kernel must boot and emit the scheduler-initialized banner, then bring up
/// all services and reach the `ViCell >` shell prompt.
#[test]
fn aarch64_boots_to_shell_prompt() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("aarch64 shell prompt not reached: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The kernel must emit its boot banner (`[ViCell] kernel boot v`) on AArch64.
///
/// This verifies the kernel's `kmain` is entered correctly after EL2→EL1 drop
/// and the PL011 UART is initialised before any subsystem setup begins.
#[test]
fn aarch64_kernel_banner() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("[Cellos] kernel boot v", 15)
        .unwrap_or_else(|e| panic!("aarch64 kernel banner missing: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The task scheduler must report it is ready before any cell is spawned.
///
/// `"Scheduler initialized"` is emitted after the frame allocator, heap, page
/// tables, and interrupt controller have all been set up successfully.
#[test]
fn aarch64_scheduler_initializes() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Scheduler initialized", 20)
        .unwrap_or_else(|e| panic!("aarch64 scheduler init not seen: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The embedded init ELF must be spawned successfully from the kernel ramdisk.
///
/// `"Successfully spawned init"` is logged by `main.rs` when `spawn_from_mem`
/// returns `Ok` for the embedded init binary. A failure here means the EL0
/// entry path, page-table user-flag setup, or manifest parsing is broken.
#[test]
fn aarch64_init_spawns() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Successfully spawned init", 20)
        .unwrap_or_else(|e| panic!("aarch64 init spawn not seen: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The shell must execute an interactive command.
///
/// Waits for the shell prompt, sends `echo aarch64-ok`, and asserts the
/// response appears. Proves the full path: PL011 UART RX → shell readline →
/// built-in dispatch → UART TX → serial harness.
#[test]
fn aarch64_echo_command() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("aarch64 shell prompt not reached: {e}\n--- output ---\n{}", qemu.dump()));
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("echo aarch64-ok");
    qemu.wait_for("aarch64-ok", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("aarch64 echo did not respond: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The periph-demo cell must open GPIO PL061 and UART PL011 on AArch64.
///
/// Demos are on-demand: init no longer auto-spawns periph-demo (demo
/// philosophy — no boot-output pollution), so the test launches it from the
/// shell like a user would. It exercises the PL061 GPIO controller at
/// 0x0903_0000 and the PL011 UART at 0x0900_0000 on the QEMU ARM virt
/// machine. The test only verifies that GPIO was opened successfully — UART
/// TX also runs but its output merges with the serial console stream.
///
/// Prerequisites: `/bin/periph-demo` in the aarch64 embedded ramdisk
/// (scripts/build-aarch64-cells.ps1).
#[test]
fn aarch64_periph_demo_gpio() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n--- output ---\n{}", qemu.dump()));

    qemu.send_line("periph-demo &");
    qemu.wait_for("[periph-demo] GPIO PL061 opened", 30)
        .unwrap_or_else(|e| panic!("periph-demo GPIO not seen: {e}\n--- output ---\n{}", qemu.dump()));
}

/// UART → input-service → app delivery on AArch64.
///
/// ARM64 QEMU virt has no virtio-keyboard-device — the only keyboard path is
/// the PL011 serial line.  This test exercises the full chain:
///
///   TCP socket → QEMU PL011 RX → viConsole::poll() →
///   relay_ascii_to_input() → input service (EV_ASCII) → dispatcher →
///   input-test AppContext
///
/// The input service deliberately does not log per-event (it would bury the
/// shell prompt), so the only observable marker is the app-side delivery
/// (`[input-test] input ok`).
///
/// Prerequisites: `/bin/input` + `/bin/input-test` in the aarch64 embedded
/// ramdisk (scripts/build-aarch64-cells.ps1).
#[test]
fn aarch64_uart_input_delivery() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n--- output ---\n{}", qemu.dump()));

    // Demos are on-demand: spawn input-test from the shell (mirrors the riscv
    // `input_bare_cell` test).
    qemu.send_line("input-test &");

    // Wait for input-test to acquire focus (retries in a yield loop until the
    // input service is registered and grants focus).
    qemu.wait_for("[input-test] focus granted", 30)
        .unwrap_or_else(|e| panic!(
            "input-test did not claim focus: {e}\n--- output ---\n{}",
            qemu.dump()
        ));

    // Settle: let input-test's AppContext event loop park in sys_recv before
    // we inject.  Mirrors the 300ms pause used in `input_bare_cell`.
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Inject a single printable byte — no trailing newline to avoid a
    // spurious second key event from the Enter character.
    qemu.send_bytes(b"a");

    // Assert the app received the event (UART relay → input service →
    // dispatcher → input-test).
    qemu.wait_for("[input-test] input ok", 15)
        .unwrap_or_else(|e| panic!(
            "input-test did not receive key: {e}\n--- output ---\n{}",
            qemu.dump()
        ));
}
