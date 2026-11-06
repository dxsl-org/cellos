//! x86_64 q35 VMware vmxnet3 DHCP data-plane gate, with and without Intel VT-d
//! (plan 261103-2218 phase 09).
//!
//! QEMU's `vmxnet3` (`15ad:07b0`) must bind `/bin/vmxnet3` with MSI-X for its
//! bounded waits and request-driven RX (`irq=msix rx=request-driven`,
//! H-S3-4), register as the only NIC driver, and carry the DHCP exchange
//! (DISCOVER/REQUEST out, OFFER/ACK in) to a configured address. The
//! `-netdev` suppresses q35's default 82574L, so no sibling NIC cell may bind.
//! Under VT-d, isolation must be active before the NIC DMA.
//!
//! Skips when the ISO (`VICELL_NIC_ISO` or `build/vicell-x86.iso`) or QEMU is
//! absent (`ci_guard` turns that into a failure under CI).

use std::path::PathBuf;
use vicell_integration_tests::{ci_guard, qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const BIND: &str = "[vmxnet3] controller bound 15ad:07b0 vmxnet3";
const IRQ: &str = "irq=msix rx=request-driven";
const OTHER_NIC_CELLS: [&str; 4] = [
    "[e1000] controller bound",
    "[e1000e] controller bound",
    "[igb] controller bound",
    "[virtio-net] ready",
];

fn iso_path() -> String {
    std::env::var("VICELL_NIC_ISO").unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../build/vicell-x86.iso")
            .to_string_lossy()
            .into_owned()
    })
}

fn run(vtd: bool) {
    let iso = iso_path();
    let qemu_ok = std::process::Command::new(qemu_x86_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !ci_guard(PathBuf::from(&iso).exists() && qemu_ok) {
        eprintln!("SKIP vmxnet3-x86: ISO ({iso}) or QEMU absent");
        return;
    }
    let disk = std::env::temp_dir().join(format!("vicell_vmxnet3_{}_{vtd}.img", std::process::id()));
    std::fs::File::create(&disk)
        .and_then(|f| f.set_len(64 * 1024 * 1024))
        .expect("create NVMe disk image");
    let qemu = QemuRunner::boot_x86_bios_with_nic_model(&iso, &disk.to_string_lossy(), "vmxnet3", vtd);
    let mut markers = vec![
        BIND,
        "[driver_cell] NIC driver registered",
        "[net-bridge] first e1000 TX accepted=true",
        "[net-bridge] first e1000 RX",
        "[net] DHCP acquired",
        "[net] IP address:",
    ];
    if vtd {
        markers.insert(0, "[vtd] Intel VT-d: DMA isolation ACTIVE");
    }
    for marker in &markers {
        if let Err(error) = qemu.wait_for(marker, BOOT_TIMEOUT) {
            let _ = std::fs::remove_file(&disk);
            panic!("vmxnet3 vtd={vtd}: {marker:?}: {error}\n{}", qemu.dump());
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    let serial = qemu.dump();
    let _ = std::fs::remove_file(&disk);
    let at = |marker: &str| serial.find(marker).unwrap();
    let positions: Vec<usize> = markers.iter().map(|m| at(m)).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "evidence out of order\n{serial}");
    let bind_line = serial[at(BIND)..].lines().next().unwrap_or_default();
    assert!(bind_line.contains(IRQ), "bind line lacks {IRQ:?}: {bind_line}");
    assert_eq!(serial.matches(BIND).count(), 1, "{serial}");
    assert_eq!(serial.matches("[driver_cell] NIC driver registered").count(), 1, "{serial}");
    for other in OTHER_NIC_CELLS {
        assert!(!serial.contains(other), "a sibling NIC cell bound: {other}\n{serial}");
    }
    for bad in ["driver gate closed", "[KERNEL PANIC]", "PANIC: Application crashed!", "[fault] Cell"] {
        assert!(!serial.contains(bad), "{bad}\n{serial}");
    }
    for p in positions {
        eprintln!("[vmxnet3-x86 vtd={vtd}] {}", serial[p..].lines().next().unwrap_or_default().trim_end());
    }
}

#[test]
fn vmxnet3_x86_dhcp() {
    run(false);
}

#[test]
fn vmxnet3_x86_vtd_dhcp() {
    run(true);
}
