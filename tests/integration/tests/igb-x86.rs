//! x86_64 q35 igb (82576-class, `8086:10c9`) bring-up gate — phase 04a.
//!
//! The igb Driver Cell must locate the controller QEMU's `igb` model exposes,
//! confirm the PCI identity is its own family, read its MAC from the NVM, bring
//! up the TX/RX rings, register through the existing `nic` path, and complete one
//! bridge Tx/Rx — with QEMU tracing **disabled**.
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
//! Skips gracefully when the ISO is not built or `qemu-system-x86_64` is not on
//! PATH; in CI a missing prerequisite is a hard failure (`ci_guard`).
//!
//! The DHCP lease that follows first Rx is phase 04b's gate, not this one: the
//! cell carries no DHCP logic, and this test only proves the data plane moves a
//! frame in each direction.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 120;

/// The cell's bind line, including the model ID and the MAC it read.
const BOUND_MARKER: &str = "[igb] controller bound 8086:10c9";

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

fn prerequisites_ok() -> bool {
    let iso = iso_path();
    let iso_ok = PathBuf::from(&iso).exists();
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
fn assert_igb_bring_up_order(serial: &str) {
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

    assert!(
        bound < registered && registered < tx && tx < rx,
        "invalid igb bring-up evidence order\n{serial}"
    );

    // Identity: the pre-change fail-closed refusal for this very ID must be gone,
    // the sibling `/bin/e1000` cell must never touch the device, and no syscall
    // may have been filtered out by the cell's allowlist.
    //
    // `/bin/igb` now claims its controller by exact vendor:device
    // (`FindPcieDeviceByVendor`, opcode 424), so the e1000 cell's class-triple
    // query no longer matches the igb model and it exits idle with **no output**
    // at all. Any `[e1000]` line therefore means the shared-class-triple race is
    // back — the sibling claimed the device and declined by ID — which is exactly
    // the dependency this phase removes.
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

/// Ordinary q35 + QEMU `igb`: identity, registration, first Tx, first Rx.
#[test]
fn igb_x86_identity_registration_tx_rx() {
    if !prerequisites_ok() {
        return;
    }

    let disk = make_nvme_disk();
    let qemu = QemuRunner::boot_x86_bios_with_igb_nic(&iso_path(), &disk.to_string_lossy());

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

    // Keep the guest alive briefly after success so an immediate panic/fault
    // emitted behind the Rx line reaches the byte-at-a-time serial reader.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_igb_bring_up_order(&qemu.dump());
    let _ = std::fs::remove_file(&disk);
}
