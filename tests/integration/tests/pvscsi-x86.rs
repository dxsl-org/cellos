//! x86_64 VMware PVSCSI block cell (plan 261103-2218 phase 09).
//!
//! QEMU's `pvscsi` (`15ad:07c0`) with one `scsi-hd` next to an NVMe primary
//! disk: `/bin/pvscsi` binds with interrupt-driven completions (`irq=msi` on
//! QEMU's model, `irq=msix` where the adapter offers MSI-X; H-S3-5), the VFS
//! mounts its FAT32 volume as the secondary disk, a file written on the first
//! boot reads back on the second (two-boot persistence, the `virtio-x86.rs`
//! pattern). Lanes: default q35 image without and with `intel-iommu`, and the
//! strict `x86_64-pc` image under `intel-iommu` on a provisioned node (pin
//! store on the NVMe disk, `scripts/x86/machine_pin_disk.py`).
//!
//! Tests skip when the ISO or QEMU is absent (`ci_guard` turns that into a
//! failure under CI).

use std::path::{Path, PathBuf};
use vicell_integration_tests::block_09c::{read_ok, shutdown, wait_until, write_ok, BOOT_TIMEOUT, PROMPT};
use vicell_integration_tests::block_09c_lanes::fixture;
use vicell_integration_tests::{ci_guard, qemu_binary_x86, x86_iso_path, QemuRunner};

const READY: &str = "[pvscsi] ready";
const BIND: &str = "[pvscsi] pci ";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn prerequisites_ok(iso: &str) -> bool {
    let iso_ok = Path::new(iso).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86()).arg("--version").output().is_ok();
    if !iso_ok {
        eprintln!("SKIP pvscsi-x86: ISO not built ({iso})");
    }
    ci_guard(iso_ok && qemu_ok)
}

fn wait_all(qemu: &QemuRunner, markers: &[&str], what: &str) {
    for marker in markers {
        qemu.wait_for(marker, BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("{what}: marker {marker:?}: {e}\n{}", qemu.dump()));
    }
}

/// The `/mnt/disk/<device>` mount VFS logged for the PVSCSI disk.
fn secondary_mount(qemu: &QemuRunner) -> String {
    wait_until(qemu, BOOT_TIMEOUT, "secondary FAT32 mount", |output| {
        let at = output.find("[vfs] FAT32 /mnt/disk/")?;
        Some(output[at..].lines().next()?.split_whitespace().nth(2)?.to_string())
    })
}

/// The bind line must name the device and an interrupt-driven completion path.
fn check_bind(output: &str, lane: &str) {
    let at = output.find(BIND).unwrap_or_else(|| panic!("{lane}: no bind line\n{output}"));
    let line = output[at..].lines().next().unwrap_or_default();
    assert!(line.contains("15ad:07c0"), "{lane}: {line}");
    assert!(line.contains("irq=msix ") || line.contains("irq=msi "), "{lane}: completions not IRQ-driven: {line}");
    for bad in ["[KERNEL PANIC]", "PANIC: Application crashed!", "[fault] Cell", "[pvscsi] init"] {
        assert!(!output.contains(bad), "{lane}: {bad}\n{output}");
    }
}

fn evidence(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("[pvscsi]") || l.contains("15ad:07c0") || l.contains("[vfs] FAT32") || l.contains("[vtd] Intel"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Two boots over the same disks: boot 1 writes the PVSCSI volume, boot 2
/// reads it back. `nvme` is the primary disk image path.
fn two_boot(lane: &str, nvme: &str, markers: &[&str], boot: impl Fn(&str, &str) -> QemuRunner) {
    let scsi = fixture();
    let mut first = boot(nvme, &scsi.arg());
    wait_all(&first, &[READY], lane);
    wait_all(&first, markers, lane);
    check_bind(&first.dump(), lane);
    let mount = secondary_mount(&first);
    write_ok(&mut first, &format!("{mount}/pvscsi.txt"), "P09-PVSCSI");
    println!("P09 {lane} boot 1:\n{}", evidence(&shutdown(first)));

    let mut second = boot(nvme, &scsi.arg());
    wait_all(&second, &[READY], lane);
    wait_all(&second, markers, lane);
    check_bind(&second.dump(), lane);
    let path = format!("{}/pvscsi.txt", secondary_mount(&second));
    read_ok(&mut second, &path, "P09-PVSCSI");
    println!("P09 {lane} boot 2:\n{}", evidence(&shutdown(second)));
}

fn plain_lane(vtd: bool) {
    let iso = x86_iso_path(&repo_root());
    if !prerequisites_ok(&iso) {
        return;
    }
    let nvme = fixture();
    let markers: &[&str] = if vtd { &["[vtd] Intel VT-d: DMA isolation ACTIVE", PROMPT] } else { &[PROMPT] };
    let lane = if vtd { "q35 + intel-iommu" } else { "q35" };
    two_boot(lane, &nvme.arg(), markers, |nvme, scsi| QemuRunner::boot_x86_pvscsi(&iso, nvme, scsi, vtd, false));
}

#[test]
fn pvscsi_binds_and_persists_x86() {
    plain_lane(false);
}

#[test]
fn pvscsi_binds_and_persists_under_vtd_x86() {
    plain_lane(true);
}

/// The strict `x86_64-pc` image under `intel-iommu`: the NVMe primary carries
/// the machine pin store (P1 FAT32 + blank CellosFS), so boot 1 pins and
/// boot 2 matches.
#[test]
fn pvscsi_persists_on_x86_64_pc_under_vtd_x86() {
    let Ok(iso) = std::env::var("VICELL_X86_PC_ISO") else {
        eprintln!("SKIP pvscsi x86_64-pc lane: VICELL_X86_PC_ISO unset");
        return;
    };
    if !prerequisites_ok(&iso) {
        return;
    }
    let pin = std::env::temp_dir().join(format!("vicell_p09_pin_{}.img", std::process::id()));
    let status = std::process::Command::new("python3")
        .arg(repo_root().join("scripts/x86/machine_pin_disk.py"))
        .arg(&pin)
        .status()
        .expect("run machine_pin_disk.py");
    assert!(status.success(), "machine_pin_disk.py failed: {status}");
    two_boot(
        "x86_64-pc + intel-iommu",
        &pin.to_string_lossy(),
        &["[x86-gate] board=x86_64-pc", "[vtd] Intel VT-d: DMA isolation ACTIVE", PROMPT],
        |nvme, scsi| QemuRunner::boot_x86_pvscsi(&iso, nvme, scsi, true, false),
    );
    let _ = std::fs::remove_file(&pin);
    let _ = std::fs::remove_file(format!("{}.srv-receipt.json", pin.display()));
}
