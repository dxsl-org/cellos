//! x86_64 VT-d register-base discovery from ACPI DMAR — phase 05.
//!
//! The remapper's register page must come from **firmware data**, not from a
//! compiled-in address: a real machine's VT-d unit sits wherever its DMAR table
//! says, and the q35 value is only meaningful for the QEMU model. Three bounded
//! lanes cover the whole decision:
//!
//! * `x86_pc_discovers_the_vtd_base_from_acpi_dmar` — the `x86_64-pc` profile
//!   (a real-hardware contract) boots with an IOMMU, the DRHD base is parsed out
//!   of DMAR, and the unit the status line names is the discovered one;
//! * `x86_pc_without_a_remapper_refuses_untranslated_dma` — the same profile
//!   boots **without** an IOMMU: DMA-capable drivers are refused by name instead
//!   of quietly running untranslated (the profile declares isolation required);
//! * `q35_without_a_remapper_names_the_fallback_and_the_identity_contract` — the
//!   QEMU model declares isolation optional, so it must say which fallback base it
//!   used and that DMA is untranslated, rather than degrading silently.
//!
//! Scope (`A-01`): QEMU's vIOMMU tables are QEMU-generated, so the parser's
//! firmware path is exercised against firmware-shaped data, not a real BIOS. The
//! physical path is phase 07.
//!
//! Skips gracefully when an ISO is not built or `qemu-system-x86_64` is not on
//! PATH; in CI a missing prerequisite is a hard failure (`ci_guard`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 120;

/// The base was taken from firmware, not from the compiled-in fallback.
const DMAR_BASE: &str = "[vtd] register base";
/// Isolation is enforcing; the same line names the programmed unit.
const ACTIVE: &str = "[vtd] Intel VT-d: DMA isolation ACTIVE";
/// The QEMU model's named fallback when no DMAR unit exists.
const Q35_FALLBACK: &str = "[vtd] no ACPI DMAR unit; using the board-declared q35 register base";
/// The QEMU model's named identity contract.
const Q35_IDENTITY: &str =
    "[iommu] board qemu-q35-x86_64 declares no DMA remapper; DMA is untranslated";
/// The PC profile's named refusal when isolation is required but absent.
const PC_REFUSAL: &str = "[iommu] DMA isolation REQUIRED by board x86_64-pc";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// The `x86_64-pc` image (`VICELL_PC_ISO` overrides): a real-hardware profile
/// where the DMA contract requires isolation.
fn pc_iso() -> String {
    std::env::var("VICELL_PC_ISO").unwrap_or_else(|_| {
        repo_root()
            .join("build/x86-pc-lane/vicell-x86-pc.iso")
            .to_string_lossy()
            .into_owned()
    })
}

/// The production q35 image (`VICELL_IOMMU_ISO` overrides).
fn prod_iso() -> String {
    std::env::var("VICELL_IOMMU_ISO").unwrap_or_else(|_| {
        repo_root()
            .join("build/vicell-x86.iso")
            .to_string_lossy()
            .into_owned()
    })
}

fn prerequisites_ok(iso: &str) -> bool {
    let iso_ok = PathBuf::from(iso).exists();
    let qemu_ok = std::process::Command::new(qemu_x86_binary())
        .arg("--version")
        .output()
        .is_ok();

    if !iso_ok {
        eprintln!(
            "SKIP iommu-dmar-x86: x86_64 ISO not built ({iso})\n\
             Build with: pwsh scripts/build-x86_64-cells.ps1 then the kernel + ISO"
        );
    }
    if !qemu_ok {
        eprintln!("SKIP iommu-dmar-x86: qemu-system-x86_64 not on PATH");
    }
    vicell_integration_tests::ci_guard(iso_ok && qemu_ok)
}

fn require_marker(qemu: &QemuRunner, disk: &std::path::Path, marker: &str, context: &str) {
    qemu.wait_for(marker, BOOT_TIMEOUT).unwrap_or_else(|error| {
        let _ = std::fs::remove_file(disk);
        panic!(
            "{context}: marker {marker:?} absent after {BOOT_TIMEOUT}s: {error}\n\
             --- serial output ---\n{}",
            qemu.dump()
        )
    });
}

fn make_nvme_disk() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "vicell_iommu_x86_{}_{}.img",
        std::process::id(),
        CTR.fetch_add(1, Ordering::Relaxed)
    ));
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&path)
        .expect("create NVMe disk image");
    f.set_len(64 * 1024 * 1024).expect("set NVMe disk size");
    let _ = f.write_all(b"");
    path
}

/// The `0x…` value that follows `key` on the first line containing `marker`.
fn value_after(serial: &str, marker: &str, key: &str) -> Option<String> {
    let line = serial.lines().find(|line| line.contains(marker))?;
    let rest = line.split(key).nth(1)?;
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == 'x')
        .collect();
    Some(token)
}

/// Real-hardware profile + IOMMU: the base must come from ACPI DMAR, and the
/// enforcing unit must be that same base.
#[test]
fn x86_pc_discovers_the_vtd_base_from_acpi_dmar() {
    let iso = pc_iso();
    if !prerequisites_ok(&iso) {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_vtd_igb_nic(&iso, &disk.to_string_lossy());

    require_marker(
        &qemu,
        &disk,
        DMAR_BASE,
        "VT-d register base was not taken from ACPI DMAR",
    );
    require_marker(&qemu, &disk, ACTIVE, "VT-d did not activate");
    require_marker(
        &qemu,
        &disk,
        "[driver_cell] NIC driver registered",
        "igb Driver Cell did not register under the discovered base",
    );
    require_marker(
        &qemu,
        &disk,
        "[net] IP address:",
        "the isolated lane did not complete DHCP",
    );

    std::thread::sleep(std::time::Duration::from_millis(500));
    let serial = qemu.dump();

    assert!(
        serial.contains("from ACPI DMAR"),
        "the base line does not name its firmware source:\n{serial}"
    );
    let discovered = value_after(&serial, DMAR_BASE, "register base ")
        .expect("the DMAR base line carries an address");
    let enforcing =
        value_after(&serial, ACTIVE, "unit=").expect("the ACTIVE line names the programmed unit");
    assert_eq!(
        discovered, enforcing,
        "the enforcing unit is not the discovered base:\n{serial}"
    );
    assert!(
        !serial.contains(Q35_FALLBACK),
        "the q35 fallback was used even though DMAR supplied a base:\n{serial}"
    );
    assert!(
        !serial.contains(PC_REFUSAL),
        "a discovered remapper must not be refused:\n{serial}"
    );
    assert!(
        serial.contains("domains="),
        "no per-Cell domain was reported for the isolated requester:\n{serial}"
    );
    assert!(!serial.contains("[KERNEL PANIC]"), "kernel panic\n{serial}");
    let _ = std::fs::remove_file(&disk);
}

/// Real-hardware profile, no remapper: refuse DMA by name, never silently run it
/// untranslated — and stay alive (the shell still starts).
#[test]
fn x86_pc_without_a_remapper_refuses_untranslated_dma() {
    let iso = pc_iso();
    if !prerequisites_ok(&iso) {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_igb_nic(&iso, &disk.to_string_lossy());

    require_marker(
        &qemu,
        &disk,
        PC_REFUSAL,
        "the required-isolation profile did not refuse untranslated DMA by name",
    );
    require_marker(
        &qemu,
        &disk,
        "Cellos >",
        "the machine did not stay alive after refusing DMA",
    );

    std::thread::sleep(std::time::Duration::from_millis(500));
    let serial = qemu.dump();

    assert!(
        !serial.contains("[driver_cell] block driver registered"),
        "a DMA-capable driver activated without translation:\n{serial}"
    );
    assert!(
        !serial.contains("[driver_cell] NIC driver registered"),
        "a DMA-capable driver activated without translation:\n{serial}"
    );
    assert!(
        !serial.contains(Q35_IDENTITY),
        "the required-isolation profile must refuse, not declare identity DMA:\n{serial}"
    );
    assert!(
        !serial.contains(ACTIVE),
        "isolation cannot be active without a remapper:\n{serial}"
    );
    assert!(!serial.contains("[KERNEL PANIC]"), "kernel panic\n{serial}");
    assert!(!serial.contains("[fault] Cell"), "Cell fault\n{serial}");
    let _ = std::fs::remove_file(&disk);
}

/// QEMU model, no remapper: the optional contract must name both the fallback
/// base it used and the untranslated-DMA consequence, and keep working.
#[test]
fn q35_without_a_remapper_names_the_fallback_and_the_identity_contract() {
    let iso = prod_iso();
    if !prerequisites_ok(&iso) {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_igb_nic(&iso, &disk.to_string_lossy());

    require_marker(
        &qemu,
        &disk,
        Q35_FALLBACK,
        "the q35 fallback base was not named",
    );
    require_marker(
        &qemu,
        &disk,
        Q35_IDENTITY,
        "the untranslated-DMA condition was not named",
    );
    require_marker(
        &qemu,
        &disk,
        "[driver_cell] block driver registered",
        "an optional-isolation profile must still run its drivers",
    );
    require_marker(
        &qemu,
        &disk,
        "[net] IP address:",
        "the unisolated lane did not complete DHCP",
    );

    std::thread::sleep(std::time::Duration::from_millis(500));
    let serial = qemu.dump();
    assert!(
        !serial.contains(PC_REFUSAL),
        "the optional profile must not refuse DMA:\n{serial}"
    );
    assert!(
        !serial.contains(DMAR_BASE),
        "no DMAR unit exists in this boot, so no discovered base may be claimed:\n{serial}"
    );
    assert!(!serial.contains("[KERNEL PANIC]"), "kernel panic\n{serial}");
    let _ = std::fs::remove_file(&disk);
}
