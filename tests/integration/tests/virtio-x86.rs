//! virtio-over-PCI driver cells on x86_64 (ADR-0031 phase 05).
//!
//! `/bin/virtio-net`, `/bin/block` (virtio-blk) and `/bin/virtio-scsi` read
//! their capabilities through `QueryPcieFunction` (434) and poll their queues.
//! Lanes:
//! - default q35 lane, no vIOMMU: all three bind, DHCP, two-boot persistence on
//!   the virtio-blk and virtio-scsi disks;
//! - `intel-iommu` without `iommu_platform`: every cell refuses its device with
//!   the named ACCESS_PLATFORM line and the shell still comes up;
//! - `intel-iommu` with `iommu_platform=on`: every cell binds;
//! - `kvm-default-vm` acceptance (`VICELL_X86_VM_ISO`, KVM required): the
//!   `x86_64-vm` profile on a default KVM guest.
//!
//! Tests skip when the ISO or QEMU is absent (`ci_guard` turns that into a
//! failure under CI).

use std::path::{Path, PathBuf};
use vicell_integration_tests::block_09c::{
    read_ok, shutdown, wait_until, write_ok, BOOT_TIMEOUT, PROMPT,
};
use vicell_integration_tests::block_09c_lanes::fixture;
use vicell_integration_tests::block_09c_two_disks::{discover, Disks};
use vicell_integration_tests::fat_disk::FatDisk;
use vicell_integration_tests::{ci_guard, qemu_binary_x86, x86_iso_path, QemuRunner};

const READY: [&str; 3] = [
    "[virtio-net] ready",
    "[virtio-blk] ready",
    "[virtio-scsi] ready",
];
const DHCP: &str = "[net] DHCP acquired";
/// The kernel's 418/424 refusal: no cell may own a virtio function whose DMA
/// would bypass VT-d (the cells' own ACCESS_PLATFORM check is defence in depth).
const KERNEL_REFUSED: &str = "virtio without VIRTIO_F_ACCESS_PLATFORM would bypass VT-d";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn prerequisites_ok(iso: &str) -> bool {
    let iso_ok = Path::new(iso).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_ok {
        eprintln!("SKIP virtio-x86: ISO not built ({iso})");
    }
    ci_guard(iso_ok && qemu_ok)
}

fn wait_all(qemu: &QemuRunner, markers: &[&str], what: &str) {
    for marker in markers {
        qemu.wait_for(marker, BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("{what}: marker {marker:?}: {e}\n{}", qemu.dump()));
    }
}

/// Boot twice over the same two FAT32 disks: write one file per disk, then
/// read both back on the second boot. `boot` builds each QEMU run.
fn two_boot_persistence(boot: impl Fn(&FatDisk, &FatDisk) -> QemuRunner, extra: &[&str]) {
    let (blk, scsi) = (fixture(), fixture());
    let files = |disks: &Disks| {
        [
            ("/mnt/sd/virtio.txt".to_string(), "P05-PRIMARY"),
            (
                format!("{}/virtio.txt", disks.secondary_mount()),
                "P05-SECONDARY",
            ),
        ]
    };
    let mut first = boot(&blk, &scsi);
    wait_all(&first, &READY, "first boot");
    wait_all(&first, extra, "first boot");
    let disks = discover(&first);
    for (path, text) in files(&disks) {
        write_ok(&mut first, &path, text);
    }
    println!("P05 first boot:\n{}", evidence(&shutdown(first)));

    let mut second = boot(&blk, &scsi);
    wait_all(&second, &READY, "second boot");
    let disks = discover(&second);
    for (path, text) in files(&disks) {
        read_ok(&mut second, &path, text);
    }
    println!("P05 second boot:\n{}", evidence(&shutdown(second)));
}

/// The lines a reviewer needs from one boot.
fn evidence(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .filter(|l| {
            l.contains("[virtio-")
                || l.contains("[pcie] refusing")
                || l.contains("[x86-gate]")
                || l.contains("timer: clock=")
                || l.contains(DHCP)
                || l.contains("[vfs] FAT32")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn virtio_pci_cells_bind_dhcp_and_persist_x86() {
    let iso = x86_iso_path(&repo_root());
    if !prerequisites_ok(&iso) {
        return;
    }
    two_boot_persistence(
        |blk, scsi| QemuRunner::boot_x86_virtio_pci(&iso, &blk.arg(), &scsi.arg(), false, false),
        &[DHCP],
    );
}

/// The strict `x86_64-pc` image (`scripts/x86/limine-pc.conf`): the VT-d lanes
/// run under the profile that requires DMA isolation.
fn pc_iso() -> String {
    std::env::var("VICELL_X86_PC_ISO").unwrap_or_else(|_| {
        repo_root()
            .join("build/x86-pc-lane/vicell-x86-pc.iso")
            .to_string_lossy()
            .into_owned()
    })
}

#[test]
fn virtio_pci_refuses_devices_without_access_platform_under_vtd_x86() {
    let iso = pc_iso();
    if !prerequisites_ok(&iso) {
        return;
    }
    let (blk, scsi) = (fixture(), fixture());
    let qemu = QemuRunner::boot_x86_virtio_pci(&iso, &blk.arg(), &scsi.arg(), true, false);
    // intel-iommu takes no PCI slot: blk 00:03.0, scsi 00:04.0. Init starts the
    // block drivers in every mode; with both disks refused there is no pin
    // store, so `x86_64-pc` withholds `net` (SAFE MODE) and virtio-net never runs.
    let refusals = ["[pcie] refusing 00:03.0", "[pcie] refusing 00:04.0"];
    let lane = "x86_64-pc + vIOMMU without iommu_platform";
    wait_all(&qemu, &["[x86-gate] board=x86_64-pc"], lane);
    wait_all(&qemu, &refusals, lane);
    wait_all(&qemu, &["Init: SAFE MODE", PROMPT], lane);
    let output = qemu.dump();
    for function in refusals {
        let line = output
            .lines()
            .find(|l| l.contains(function))
            .unwrap_or_default();
        assert!(
            line.contains(KERNEL_REFUSED),
            "{function} refused for another reason: {line}"
        );
        assert_eq!(
            output.matches(function).count(),
            1,
            "{function}: refusal logged more than once"
        );
    }
    assert!(
        !READY.iter().any(|ready| output.contains(ready)),
        "a cell bound a bypassing device:\n{output}"
    );
    println!("P05 fail-closed:\n{}", evidence(&output));
}

#[test]
fn virtio_pci_binds_with_access_platform_under_vtd_x86() {
    let iso = pc_iso();
    if !prerequisites_ok(&iso) {
        return;
    }
    provisioned_two_boot(
        "x86_64-pc + vIOMMU + iommu_platform",
        |blk, scsi| QemuRunner::boot_x86_virtio_pci(&iso, &blk.arg(), &scsi.arg(), true, true),
        &["[x86-gate] board=x86_64-pc"],
    );
}

/// A virtio-blk disk from `scripts/x86/machine_pin_disk.py` (P1 FAT32 + blank
/// P5 CellosFS): `x86_64-vm` requires the machine pin store (ADR-0029 §3), so
/// the acceptance guest is a provisioned node, not a bare FAT32 disk.
struct PinDisk(PathBuf);

impl PinDisk {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("vicell_p05_pin_{}_{index}.img", std::process::id()));
        let status = std::process::Command::new("python3")
            .arg(repo_root().join("scripts/x86/machine_pin_disk.py"))
            .arg(&path)
            .status()
            .expect("run machine_pin_disk.py");
        assert!(status.success(), "machine_pin_disk.py failed: {status}");
        Self(path)
    }

    fn arg(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for PinDisk {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(format!("{}.srv-receipt.json", self.0.display()));
    }
}

/// The `/mnt/disk/<device>` mount VFS logged for the secondary disk.
fn secondary_mount(qemu: &QemuRunner) -> String {
    wait_until(qemu, BOOT_TIMEOUT, "secondary FAT32 mount", |output| {
        let at = output.find("[vfs] FAT32 /mnt/disk/")?;
        let line = output[at..].lines().next()?;
        Some(line.split_whitespace().nth(2)?.to_string())
    })
}

/// ADR-0031 S1 acceptance: a default KVM guest under `x86_64-vm`, booted twice.
#[test]
fn kvm_default_vm_acceptance_x86() {
    let Ok(iso) = std::env::var("VICELL_X86_VM_ISO") else {
        eprintln!("SKIP kvm_default_vm_acceptance_x86: VICELL_X86_VM_ISO unset");
        return;
    };
    if !Path::new("/dev/kvm").exists() {
        eprintln!("SKIP kvm_default_vm_acceptance_x86: /dev/kvm absent");
        return;
    }
    if !prerequisites_ok(&iso) {
        return;
    }
    provisioned_two_boot(
        "kvm-default-vm",
        |blk, scsi| QemuRunner::boot_x86_kvm_default_vm(&iso, &blk.arg(), &scsi.arg()),
        &[
            "[x86-gate] board=x86_64-vm",
            "dma isolation optional (operator-selected VM profile)",
            "apic=x2apic",
        ],
    );
}

/// Two boots of a provisioned node (pin store on the virtio-blk disk, FAT32 on
/// the virtio-scsi disk): the first pins the machine, starts `net`, takes a
/// DHCP lease and writes one file per disk; the second matches the pin and
/// reads both files back.
fn provisioned_two_boot(
    lane: &str,
    boot: impl Fn(&PinDisk, &FatDisk) -> QemuRunner,
    markers: &[&str],
) {
    let (blk, scsi) = (PinDisk::new(), fixture());
    let common = [
        "[vfs] FAT32 /mnt/sd volume mounted",
        "Init: net launched.",
        DHCP,
        PROMPT,
    ];
    let mut first = boot(&blk, &scsi);
    for set in [&READY[..], markers, &common, &["machine-pin: first boot"]] {
        wait_all(&first, set, lane);
    }
    let secondary = secondary_mount(&first);
    write_ok(&mut first, "/mnt/sd/virtio.txt", "P05-PRIMARY");
    write_ok(
        &mut first,
        &format!("{secondary}/virtio.txt"),
        "P05-SECONDARY",
    );
    println!("P05 {lane} boot 1:\n{}", evidence(&shutdown(first)));

    let mut second = boot(&blk, &scsi);
    for set in [&READY[..], markers, &common, &["machine-pin: match "]] {
        wait_all(&second, set, lane);
    }
    read_ok(&mut second, "/mnt/sd/virtio.txt", "P05-PRIMARY");
    read_ok(
        &mut second,
        &format!("{secondary}/virtio.txt"),
        "P05-SECONDARY",
    );
    println!("P05 {lane} boot 2:\n{}", evidence(&shutdown(second)));
}
