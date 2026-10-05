//! x86_64 q35 igb (82576-class, `8086:10c9`) data-plane gates — phases 04a/04b.
//!
//! The igb Driver Cell must locate the controller QEMU's `igb` model exposes,
//! confirm the PCI identity is its own family, read its MAC from the NVM, bring
//! up the TX/RX rings, register through the existing `nic` path, and complete a
//! bridge Tx/Rx **and the DHCP lease that follows it** — with QEMU tracing
//! **disabled**. Two bounded variants, matching the e1000 DHCP gates:
//!
//! * ordinary q35 (`igb_x86_dhcp`) — the whole chain with no translation;
//! * VT-d q35 (`igb_x86_vtd_dhcp`) — `-device intel-iommu` precedes the igb
//!   controller, so DMA isolation must be ACTIVE before the first accepted
//!   frame, and the same DHCP sequence must complete through the IOMMU.
//!
//! Device selection is asserted structurally. The controller shares the Ethernet
//! class triple `02:00:00` with `/bin/e1000`, so `/bin/igb` claims it by exact
//! vendor:device (`FindPcieDeviceByVendor`, opcode 424). The sibling e1000 cell's
//! class query must therefore find nothing and exit idle with no output: any
//! `[e1000]` line would mean the shared-triple race is back. The idempotent
//! identity lines are also asserted: the pre-change fail-closed
//! `[e1000] unsupported Ethernet 8086:10c9` refusal must be gone, and the cell's
//! own `[igb] controller bound 8086:10c9` line must be present.
//!
//! Scope limit (`A-01`): QEMU's `igb` model is 82576-class and is the only igb
//! identity this lane can validate. It does **not** model the i210/i211 iNVM or
//! PHY behaviour, so nothing here is evidence for those parts — which is why the
//! cell claims only `10C9` + `1533` (i210 copper with external flash) and leaves
//! the other SKUs refused until their NVM/media paths exist.
//!
//! Skips gracefully when the ISO is not built or `qemu-system-x86_64` is not on
//! PATH; in CI a missing prerequisite is a hard failure (`ci_guard`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 120;

/// The cell's bind line, including the model ID and the MAC it read.
const BOUND_MARKER: &str = "[igb] controller bound 8086:10c9";
/// Intel VT-d must report isolation active before any igb DMA is accepted.
const VTD_ACTIVE: &str = "[vtd] Intel VT-d: DMA isolation ACTIVE";
/// The net service owns DHCP; the cell carries none. ASCII prefix only: the
/// serial reader is byte-at-a-time, so the em dash in the full line would never
/// match (it arrives as UTF-8 fragments).
const DHCP_ACQUIRED: &str = "[net] DHCP acquired";
const DHCP_IP: &str = "[net] IP address: 10.0.2.15";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// ISO under test: `VICELL_IGB_ISO` (the lane image) or the production default.
fn iso_path() -> String {
    std::env::var("VICELL_IGB_ISO").unwrap_or_else(|_| {
        repo_root()
            .join("build/vicell-x86.iso")
            .to_string_lossy()
            .into_owned()
    })
}

/// ISO for the VT-d variant: `VICELL_IGB_VTD_ISO` or the production default.
///
/// Kept separate from [`iso_path`] because the VT-d variant is a **q35** gate:
/// Intel VT-d requires the `QemuX86Q35` board identity, and the `x86_64-pc`
/// descriptor refuses the q35 register-base fallback by design until phase 05
/// discovers the base from ACPI DMAR. Pinning a `GenericX86Pc` lane image here
/// would fail the gate for the right reason but the wrong layer.
fn vtd_iso_path() -> String {
    std::env::var("VICELL_IGB_VTD_ISO").unwrap_or_else(|_| {
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
            "SKIP igb-x86: x86_64 ISO not built ({iso})\n\
             Build with: pwsh scripts/build-x86_64-cells.ps1 then the kernel + ISO"
        );
    }
    if !qemu_ok {
        eprintln!("SKIP igb-x86: qemu-system-x86_64 not on PATH");
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
        "vicell_igb_x86_{}_{}.img",
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

/// Assert the observed order and the properties of each evidence line.
///
/// `first_marker` is the lane's opening evidence: the igb bind line in the
/// ordinary variant, the VT-d activation line in the VT-d variant — which must
/// therefore precede the bind line, so the igb controller's DMA is translated
/// before any frame is accepted.
fn assert_igb_dhcp_order(serial: &str, first_marker: &str) {
    let first = serial
        .find(first_marker)
        .unwrap_or_else(|| panic!("missing initial marker {first_marker:?}\n{serial}"));
    let bound = serial
        .find(BOUND_MARKER)
        .unwrap_or_else(|| panic!("missing igb bind line {BOUND_MARKER:?}\n{serial}"));
    let bound_line = serial[bound..].lines().next().unwrap_or_default();
    assert!(
        bound_line.contains("link_up=true"),
        "igb reported link down before enabling RX: {bound_line}\n{serial}"
    );
    assert!(
        !bound_line.contains("mac=00:00:00:00:00:00"),
        "igb published a zero MAC — the NVM read did not complete: {bound_line}\n{serial}"
    );

    let registered = serial
        .find("[driver_cell] NIC driver registered")
        .unwrap_or_else(|| panic!("missing NIC registration\n{serial}"));
    let tx = serial
        .find("[net-bridge] first e1000 TX")
        .unwrap_or_else(|| panic!("missing first bridge Tx evidence\n{serial}"));
    let tx_line = serial[tx..].lines().next().unwrap_or_default();
    assert!(
        tx_line.contains("accepted=true"),
        "first bridge Tx was not accepted by the igb Driver Cell: {tx_line}\n{serial}"
    );
    let rx = serial
        .find("[net-bridge] first e1000 RX")
        .unwrap_or_else(|| panic!("missing first bridge Rx evidence\n{serial}"));
    let dhcp = serial
        .find(DHCP_ACQUIRED)
        .unwrap_or_else(|| panic!("missing DHCP acquisition\n{serial}"));
    let ip = serial
        .find(DHCP_IP)
        .unwrap_or_else(|| panic!("missing the leased address {DHCP_IP:?}\n{serial}"));

    assert!(
        first <= bound && bound < registered && registered < tx && tx < rx && rx < dhcp && dhcp < ip,
        "invalid igb DHCP evidence order\n{serial}"
    );

    // Identity: the pre-change fail-closed refusal for this very ID must be gone,
    // the sibling `/bin/e1000` cell must never touch the device, and no syscall
    // may have been filtered out by the cell's allowlist.
    //
    // `/bin/igb` claims its controller by exact vendor:device
    // (`FindPcieDeviceByVendor`, opcode 424), so the e1000 cell's class-triple
    // query no longer matches the igb model and it exits idle with **no output**
    // at all. Any `[e1000]` line therefore means the shared-class-triple race is
    // back — the sibling claimed the device and declined by ID — which is exactly
    // the dependency phase 04a removes.
    assert!(
        !serial.contains("[e1000]"),
        "sibling /bin/e1000 cell touched the igb controller (device-selection race):\n{serial}"
    );
    assert!(
        !serial.contains("[kernel] syscall denied") && !serial.contains("denied for tid"),
        "a syscall was denied for the igb cell:\n{serial}"
    );

    assert!(!serial.contains("[KERNEL PANIC]"), "kernel panic\n{serial}");
    assert!(
        !serial.contains("PANIC: Application crashed!"),
        "Cell panic\n{serial}"
    );
    assert!(!serial.contains("[fault] Cell"), "Cell fault\n{serial}");
}

/// Ordinary q35 + QEMU `igb`: identity, registration, Tx/Rx, DHCP lease.
#[test]
fn igb_x86_dhcp() {
    let iso = iso_path();
    if !prerequisites_ok(&iso) {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_igb_nic(&iso, &disk.to_string_lossy());

    require_marker(
        &qemu,
        &disk,
        BOUND_MARKER,
        "igb Driver Cell did not bind the 8086:10c9 controller",
    );
    require_marker(
        &qemu,
        &disk,
        "[driver_cell] NIC driver registered",
        "igb Driver Cell did not register through the nic path",
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 TX",
        "net service did not submit a frame through the igb cell",
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 RX",
        "igb did not receive a frame",
    );
    require_marker(
        &qemu,
        &disk,
        DHCP_ACQUIRED,
        "the net service did not complete DHCP through the igb cell",
    );
    require_marker(
        &qemu,
        &disk,
        DHCP_IP,
        "the igb DHCP lease did not configure an address",
    );

    // Keep the guest alive briefly after success so an immediate panic/fault
    // emitted behind the lease line reaches the byte-at-a-time serial reader.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_igb_dhcp_order(&qemu.dump(), BOUND_MARKER);
    let _ = std::fs::remove_file(&disk);
}

/// VT-d q35 + QEMU `igb`: isolation must precede accepted igb DMA and DHCP.
#[test]
fn igb_x86_vtd_dhcp() {
    let iso = vtd_iso_path();
    if !prerequisites_ok(&iso) {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_vtd_igb_nic(&iso, &disk.to_string_lossy());

    require_marker(&qemu, &disk, VTD_ACTIVE, "VT-d did not activate");
    require_marker(
        &qemu,
        &disk,
        "[driver_cell] block driver registered",
        "NVMe DMA did not complete under VT-d",
    );
    require_marker(
        &qemu,
        &disk,
        BOUND_MARKER,
        "igb Driver Cell did not bind the controller under VT-d",
    );
    require_marker(
        &qemu,
        &disk,
        "[driver_cell] NIC driver registered",
        "igb Driver Cell did not register under VT-d",
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 TX",
        "net service did not submit DHCP through the VT-d igb cell",
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 RX",
        "igb did not receive DHCP through VT-d",
    );
    require_marker(
        &qemu,
        &disk,
        DHCP_ACQUIRED,
        "isolated SLIRP DHCP did not complete under VT-d",
    );
    require_marker(
        &qemu,
        &disk,
        DHCP_IP,
        "the isolated igb DHCP lease did not configure an address",
    );

    // The final forbidden-marker scan must include immediate post-DHCP output.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_igb_dhcp_order(&qemu.dump(), VTD_ACTIVE);
    let _ = std::fs::remove_file(&disk);
}
