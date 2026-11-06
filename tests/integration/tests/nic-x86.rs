//! x86_64 q35 Intel NIC DHCP data-plane gates, with and without Intel VT-d.
//!
//! The complete oracle crosses every Driver-Cell boundary: the Platform Cell
//! discovers the NIC, its Driver Cell binds the exact SKU and registers, the net
//! service submits a DHCP frame through that driver, the driver reports an RX
//! frame, and smoltcp acquires an address from QEMU's isolated SLIRP DHCP server.
//!
//! Three QEMU models, each with and without VT-d (plan 261103-2218 phase 08):
//! `e1000` (82540EM) and `e1000-82545em` (82545EM) bind `/bin/e1000`, polled
//! (`irq=polled reason=request-driven`); `e1000e` (82574L) binds `/bin/e1000e`
//! with MSI-X for its bounded waits and request-driven RX (`irq=msix
//! rx=request-driven`, H-S3-4). Every lane names its SKU and proves that no
//! sibling NIC cell bound or registered: the `-netdev` suppresses q35's default
//! 82574L, so exactly one Ethernet function exists.
//!
//! The VT-d variant additionally requires isolation to become active before
//! the NIC DMA traffic. Its retained NVMe registration proves an independent
//! DMA client also completes real queue traffic through the translated domain.
//!
//! The bridge witness lines say `first e1000 TX/RX` whichever NIC cell is bound
//! (`cells/services/net/src/interface.rs`).
//!
//! Every test skips gracefully when the x86_64 ISO is not built or
//! `qemu-system-x86_64` is not on PATH.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// The ISO under test. `VICELL_NIC_ISO` overrides (the lane gate pins the image
/// it just built); otherwise the shared production ISO is used.
fn iso_path() -> String {
    if let Ok(path) = std::env::var("VICELL_NIC_ISO") {
        return path;
    }
    repo_root()
        .join("build/vicell-x86.iso")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let iso_ok = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_x86_binary())
        .arg("--version")
        .output()
        .is_ok();

    if !iso_ok {
        eprintln!(
            "SKIP nic-x86: x86_64 ISO not built ({})\n\
             Build with: scripts/build-x86_64-cells.ps1 then build/make-iso.sh",
            iso_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP nic-x86: qemu-system-x86_64 not on PATH");
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

/// One QEMU NIC model and what its Driver Cell must print.
struct Lane {
    model: &'static str,
    /// The bind line prefix naming cell, vendor:device and SKU.
    bind: &'static str,
    /// The interrupt posture the bind line must carry.
    irq: &'static str,
}

const E1000: Lane = Lane {
    model: "e1000",
    bind: "[e1000] controller bound 8086:100e 82540EM",
    irq: "irq=polled reason=request-driven",
};
const E1000_82545EM: Lane = Lane {
    model: "e1000-82545em",
    bind: "[e1000] controller bound 8086:100f 82545EM",
    irq: "irq=polled reason=request-driven",
};
const E1000E: Lane = Lane {
    model: "e1000e",
    bind: "[e1000e] controller bound 8086:10d3 82574L",
    irq: "irq=msix rx=request-driven",
};

/// Every NIC cell's bind line; exactly one may appear per lane.
const BIND_LINES: [&str; 4] = [
    "[e1000] controller bound",
    "[e1000e] controller bound",
    "[igb] controller bound",
    "[virtio-net] ready",
];

fn assert_dhcp_order(serial: &str, lane: &Lane, first_marker: &str) {
    let first = serial
        .find(first_marker)
        .unwrap_or_else(|| panic!("missing initial marker {first_marker:?}\n{serial}"));
    let bind = serial
        .find(lane.bind)
        .unwrap_or_else(|| panic!("{}: missing bind line {:?}\n{serial}", lane.model, lane.bind));
    let registered = serial
        .find("[driver_cell] NIC driver registered")
        .unwrap_or_else(|| panic!("missing NIC registration\n{serial}"));
    let tx = serial
        .find("[net-bridge] first e1000 TX")
        .unwrap_or_else(|| panic!("missing NIC Tx evidence\n{serial}"));
    let rx = serial
        .find("[net-bridge] first e1000 RX")
        .unwrap_or_else(|| panic!("missing NIC Rx evidence\n{serial}"));
    let dhcp = serial
        .find("[net] DHCP acquired")
        .unwrap_or_else(|| panic!("missing DHCP acquisition\n{serial}"));
    let bind_line = serial[bind..].lines().next().unwrap_or_default();
    let tx_line = serial[tx..].lines().next().unwrap_or_default();

    assert!(
        bind_line.contains(lane.irq),
        "{}: bind line lacks {:?}: {bind_line}\n{serial}",
        lane.model,
        lane.irq
    );
    assert!(
        tx_line.contains("accepted=true"),
        "first Tx was not accepted by its Driver Cell: {tx_line}\n{serial}"
    );
    assert!(
        first <= registered && bind < registered && registered < tx && tx < rx && rx < dhcp,
        "{}: invalid DHCP evidence order\n{serial}",
        lane.model
    );
    // Exactly one NIC cell bound and registered: no sibling touched the device.
    for line in BIND_LINES {
        let expected = usize::from(lane.bind.starts_with(line));
        assert_eq!(
            serial.matches(line).count(),
            expected,
            "{}: {line:?} seen a wrong number of times\n{serial}",
            lane.model
        );
    }
    assert_eq!(
        serial.matches("[driver_cell] NIC driver registered").count(),
        1,
        "{}: more than one NIC driver registered\n{serial}",
        lane.model
    );
    assert!(
        !serial.contains("driver gate closed"),
        "{}: the NIC was refused by an identity gate\n{serial}",
        lane.model
    );
    assert!(!serial.contains("[KERNEL PANIC]"), "kernel panic\n{serial}");
    assert!(
        !serial.contains("PANIC: Application crashed!"),
        "Cell panic\n{serial}"
    );
    assert!(!serial.contains("[fault] Cell"), "Cell fault\n{serial}");
    // Evidence: the decisive lines of a passing lane, in boot order.
    for at in [first, bind, registered, tx, rx, dhcp] {
        let line = serial[at..].lines().next().unwrap_or_default();
        eprintln!("[nic-x86 {}] {}", lane.model, line.trim_end());
    }
}

fn make_nvme_disk() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "vicell_nic_x86_{}_{}.img",
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

/// Ordinary q35 (`vtd = false`) or VT-d q35: require the SKU bind line, Driver
/// Cell Tx/Rx and a completed DHCP lease; under VT-d, isolation must precede the
/// NIC DMA and the NVMe DMA must complete through the translated domain.
fn run_lane(lane: &Lane, vtd: bool) {
    if !prerequisites_ok() {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_nic_model(
        &iso_path(),
        &disk.to_string_lossy(),
        lane.model,
        vtd,
    );
    let model = lane.model;

    let first = if vtd {
        let active = "[vtd] Intel VT-d: DMA isolation ACTIVE";
        require_marker(&qemu, &disk, active, "VT-d did not activate");
        require_marker(
            &qemu,
            &disk,
            "[driver_cell] block driver registered",
            "NVMe DMA did not complete under VT-d",
        );
        active
    } else {
        lane.bind
    };
    require_marker(&qemu, &disk, lane.bind, &format!("{model}: Driver Cell did not bind"));
    require_marker(
        &qemu,
        &disk,
        "[driver_cell] NIC driver registered",
        &format!("{model}: Driver Cell did not register"),
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 TX",
        &format!("{model}: net service did not submit DHCP through the NIC"),
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 RX",
        &format!("{model}: the NIC did not receive the SLIRP DHCP response"),
    );
    require_marker(
        &qemu,
        &disk,
        "[net] IP address:",
        &format!("{model}: isolated SLIRP DHCP did not complete"),
    );

    // Keep the guest alive briefly after success so an immediate panic/fault
    // emitted behind the DHCP line reaches the byte-at-a-time serial reader.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_dhcp_order(&qemu.dump(), lane, first);
    let _ = std::fs::remove_file(&disk);
}

#[test]
fn nic_x86_e1000_dhcp() {
    run_lane(&E1000, false);
}

#[test]
fn nic_x86_vtd_e1000_dhcp() {
    run_lane(&E1000, true);
}

#[test]
fn nic_x86_e1000_82545em_dhcp() {
    run_lane(&E1000_82545EM, false);
}

#[test]
fn nic_x86_vtd_e1000_82545em_dhcp() {
    run_lane(&E1000_82545EM, true);
}

#[test]
fn nic_x86_e1000e_dhcp() {
    run_lane(&E1000E, false);
}

#[test]
fn nic_x86_vtd_e1000e_dhcp() {
    run_lane(&E1000E, true);
}
