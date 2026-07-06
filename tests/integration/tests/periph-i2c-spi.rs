//! Peripheral integration tests: SPI bit-bang (Track C) + I2C sensor-demo.
//!
//! These tests boot the AArch64 ARM virt image (with GPIO PL061) and assert
//! that the peripheral demo cells print their expected probe strings.
//!
//! Prerequisites:
//!   - `qemu-system-aarch64` on PATH
//!   - Kernel: `target/aarch64-unknown-none-softfloat/release/vicell-kernel`
//!   - Disk: `disk_arm_virt.img` at repo root (built by `format-disk-arm.ps1`)
//!
//! Tests skip gracefully when any prerequisite is absent — same pattern as
//! `aarch64-boot.rs`. CI behaviour: skip = exit 0 (green), not failure.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_aarch64, QemuRunner};

/// Allow enough time for all supervised services to start and for the best-effort
/// demo cells (spawned last by init) to run. The demos spawn after bench/periph-demo,
/// so a full BOOT_TIMEOUT is needed.
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

/// Skip when any prerequisite is absent: QEMU binary, built kernel, or disk image.
fn prerequisites_ok() -> bool {
    let kernel_exists = PathBuf::from(kernel_path()).exists();
    let disk_exists = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_aarch64())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_exists {
        eprintln!("SKIP periph-i2c-spi: kernel not built ({})", kernel_path());
    }
    if !disk_exists {
        eprintln!(
            "SKIP periph-i2c-spi: disk_arm_virt.img missing — run .\\format-disk-arm.ps1"
        );
    }
    if !qemu_ok {
        eprintln!("SKIP periph-i2c-spi: qemu-system-aarch64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_exists && disk_exists && qemu_ok)
}

/// Track C — SPI TX path: the spi-demo cell must write bytes via bit-bang GPIO
/// and print the TX-OK probe string.
///
/// On QEMU, MISO floats at 0 — the test asserts the TX path only, which is the
/// meaningful correctness check (MOSI/SCK/CS MMIO toggling via PL061 GPIO).
#[test]
fn aarch64_spi_demo_tx() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    // spi-demo is spawned best-effort by init after all supervised services.
    // Use BOOT_TIMEOUT to allow the full bring-up sequence to complete.
    qemu.wait_for("[spi-demo] SPI TX OK", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "spi-demo TX probe not seen: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// I2C sensor-demo: the cell must open GPIO, print its banner line, and start
/// polling (synthetic data on QEMU where no real SHT3x slave exists).
///
/// Asserts the banner `[sensor-demo] SHT3x via bit-bang I2C` — this line is
/// printed unconditionally before the first poll attempt, so it is stable even
/// when the I2C slave NACKs (expected QEMU behaviour).
#[test]
fn aarch64_i2c_sensor_demo_banner() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("[sensor-demo] SHT3x via bit-bang I2C", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "sensor-demo banner not seen: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}
