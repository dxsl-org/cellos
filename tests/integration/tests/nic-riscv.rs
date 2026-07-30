//! Phase B-04: RISC-V IOMMU bare passthrough integration test.
//!
//! `nic_riscv_iommu_bare` boots QEMU RISC-V virt with
//! `-device riscv-iommu-pci,bus=pcie.0` and asserts the kernel logs
//! `[iommu] RISC-V IOMMU: bare passthrough enabled`.
//!
//! Skip conditions (graceful, not a failure):
//! - `qemu-system-riscv64` not on PATH
//! - QEMU version < 8.2 (riscv-iommu-pci added in 8.2)
//! - RISC-V kernel not built

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn riscv_kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/vicell-kernel")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root()
        .join("disk_v3.img")
        .to_string_lossy()
        .into_owned()
}

/// Returns true if qemu-system-riscv64 version is ≥ 8.2.
/// Whether this QEMU can emulate the RISC-V IOMMU at all.
///
/// Asks the emulator for its device list rather than parsing a version. A version
/// threshold gets this wrong in both directions: distributions backport and omit
/// devices independently of the release number, and Debian's 8.2.2 in particular
/// reports a version that clears any sane threshold while shipping only
/// `virtio-iommu`. The result was a hard failure — the only suite here that
/// aborts instead of skipping when its emulator cannot run it.
fn qemu_riscv_iommu_available() -> bool {
    let Ok(out) = std::process::Command::new(qemu_binary())
        .arg("-device")
        .arg("help")
        .output()
    else {
        return false;
    };
    // The list goes to stdout on some builds and stderr on others.
    let listing = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    listing.contains("riscv-iommu-pci")
}

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(riscv_kernel_path()).exists();
    let disk_ok   = PathBuf::from(disk_path()).exists();
    let qemu_ok   = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();

    if !kernel_ok {
        eprintln!(
            "SKIP nic-riscv: RISC-V kernel not built ({})",
            riscv_kernel_path()
        );
    }
    if !disk_ok {
        eprintln!("SKIP nic-riscv: disk_v3.img not found");
    }
    if !qemu_ok {
        eprintln!("SKIP nic-riscv: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

/// Phase B-04: RISC-V IOMMU bare passthrough.
///
/// Boots with `riscv-iommu-pci` and asserts the kernel probe log.
/// Skips when the emulator does not carry that device.
#[test]
fn nic_riscv_iommu_bare() {
    if !prerequisites_ok() {
        return;
    }
    if !qemu_riscv_iommu_available() {
        vicell_integration_tests::skip_notice(
            "qemu-system-riscv64 has no riscv-iommu-pci device, so the IOMMU probe \
             cannot be exercised",
        );
        return;
    }

    let qemu = QemuRunner::boot_riscv_with_iommu(&riscv_kernel_path(), &disk_path());

    qemu.wait_for("[iommu] RISC-V IOMMU: bare passthrough enabled", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!(
            "RISC-V IOMMU not detected within {BOOT_TIMEOUT}s: {e}\n\
             Hint: pcie_ecam::find_class(0x08, 0x06, 0x00) must find the device.\n\
             --- serial output ---\n{}",
            qemu.dump()
        ));
}
