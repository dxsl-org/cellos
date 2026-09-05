//! x86_64 q35 e1000 DHCP data-plane gates, with and without Intel VT-d.
//!
//! The complete oracle crosses every Driver-Cell boundary: the Platform Cell
//! discovers e1000, the e1000 Cell registers, the net service submits a DHCP
//! frame through that driver, e1000 reports an RX frame, and smoltcp acquires an
//! address from QEMU's isolated SLIRP DHCP server.
//!
//! The VT-d variant additionally requires isolation to become active before
//! the e1000 DMA traffic. Its retained NVMe registration proves an independent
//! DMA client also completes real queue traffic through the translated domain.
//!
//! Both tests skip gracefully when the x86_64 ISO is not built or
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

fn iso_path() -> String {
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

fn assert_e1000_dhcp_order(serial: &str, first_marker: &str) {
    let first = serial
        .find(first_marker)
        .unwrap_or_else(|| panic!("missing initial marker {first_marker:?}\n{serial}"));
    let registered = serial
        .find("[driver_cell] NIC driver registered")
        .unwrap_or_else(|| panic!("missing NIC registration\n{serial}"));
    let tx = serial
        .find("[net-bridge] first e1000 TX")
        .unwrap_or_else(|| panic!("missing e1000 Tx evidence\n{serial}"));
    let rx = serial
        .find("[net-bridge] first e1000 RX")
        .unwrap_or_else(|| panic!("missing e1000 Rx evidence\n{serial}"));
    let dhcp = serial
        .find("[net] DHCP acquired")
        .unwrap_or_else(|| panic!("missing DHCP acquisition\n{serial}"));
    let tx_line = serial[tx..].lines().next().unwrap_or_default();

    assert!(
        tx_line.contains("accepted=true"),
        "first e1000 Tx was not accepted by its Driver Cell: {tx_line}\n{serial}"
    );
    assert!(
        first <= registered && registered < tx && tx < rx && rx < dhcp,
        "invalid e1000 DHCP evidence order\n{serial}"
    );
    assert!(!serial.contains("[KERNEL PANIC]"), "kernel panic\n{serial}");
    assert!(
        !serial.contains("PANIC: Application crashed!"),
        "Cell panic\n{serial}"
    );
    assert!(!serial.contains("[fault] Cell"), "Cell fault\n{serial}");
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

/// Ordinary q35: require Driver-Cell Tx/Rx and a completed DHCP lease.
#[test]
fn nic_x86_e1000_dhcp() {
    if !prerequisites_ok() {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_nic(&iso_path(), &disk.to_string_lossy());

    let registration = "[driver_cell] NIC driver registered";
    require_marker(
        &qemu,
        &disk,
        registration,
        "e1000 Driver Cell did not register",
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 TX",
        "net service did not submit DHCP through e1000",
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 RX",
        "e1000 did not receive the SLIRP DHCP response",
    );
    require_marker(
        &qemu,
        &disk,
        "[net] IP address:",
        "isolated SLIRP DHCP did not complete",
    );

    // Keep the guest alive briefly after success so an immediate panic/fault
    // emitted behind the DHCP line reaches the byte-at-a-time serial reader.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_e1000_dhcp_order(&qemu.dump(), registration);
    let _ = std::fs::remove_file(&disk);
}

/// VT-d q35: isolation must precede accepted e1000 DMA and DHCP completion.
#[test]
fn nic_x86_vtd_e1000_dhcp() {
    if !prerequisites_ok() {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_vtd(&iso_path(), &disk.to_string_lossy());

    let active = "[vtd] Intel VT-d: DMA isolation ACTIVE";
    require_marker(&qemu, &disk, active, "VT-d did not activate");
    require_marker(
        &qemu,
        &disk,
        "[driver_cell] block driver registered",
        "NVMe DMA did not complete under VT-d",
    );
    require_marker(
        &qemu,
        &disk,
        "[driver_cell] NIC driver registered",
        "e1000 Driver Cell did not register under VT-d",
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 TX",
        "net service did not submit DHCP through VT-d e1000",
    );
    require_marker(
        &qemu,
        &disk,
        "[net-bridge] first e1000 RX",
        "e1000 did not receive DHCP through VT-d",
    );
    require_marker(
        &qemu,
        &disk,
        "[net] IP address:",
        "isolated SLIRP DHCP did not complete under VT-d",
    );

    // The final forbidden-marker scan must include immediate post-DHCP output.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_e1000_dhcp_order(&qemu.dump(), active);
    let _ = std::fs::remove_file(&disk);
}
