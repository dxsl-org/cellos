//! x86_64 xHCI USB host integration tests — Driver Cell architecture (phase 03).
//!
//! The Platform Cell (`/bin/platform`, spawned by the kernel) scans ECAM and
//! registers devices/BARs; init spawns the xHCI Driver Cell (`/bin/xhci`), which
//! locates the `0C:03:30` USB controller via `sys_find_pcie_device`, claims its
//! memory BAR through `sys_request_mmio`, resets the controller, programs the
//! command and event rings, resets the port carrying the QEMU `usb-kbd`, and
//! enumerates it (Enable Slot → Address Device → GET_DESCRIPTOR →
//! SET_CONFIGURATION → SET_PROTOCOL(boot) → Configure Endpoint). One interrupt-IN
//! transfer is then polled and the decoded key is delivered through the input
//! service, the same producer path `/bin/dwc2-usb` uses.
//!
//! The oracles are:
//!   `[xhci] controller init ok`
//!   `[xhci] enumerated device vid=0x0627 pid=0x0001`
//!   `[xhci] USB HID keyboard ready`
//!   `[xhci] key down` (after a QMP-injected keystroke)
//!   the character echoed by the shell
//! and the no-keyboard boot still reaches `Cellos >`.
//!
//! Tests skip gracefully when the ISO or `qemu-system-x86_64` is absent.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use vicell_integration_tests::{ci_guard, qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const KEY_TIMEOUT: u64 = 20;
const CMD_TIMEOUT: u64 = 15;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// The ISO carrying the xHCI lane's `/bin/xhci`. `VICELL_XHCI_ISO` overrides;
/// otherwise the phase gate's lane ISO is preferred, falling back to the shared
/// production ISO (which skips if that older image has no xHCI cell).
fn iso_path() -> String {
    if let Ok(path) = std::env::var("VICELL_XHCI_ISO") {
        return path;
    }
    let lane = repo_root().join("build/x86-pc-lane/vicell-x86-xhci.iso");
    if lane.exists() {
        return lane.to_string_lossy().into_owned();
    }
    repo_root()
        .join("build/vicell-x86.iso")
        .to_string_lossy()
        .into_owned()
}

/// Provenance check: the kernel embeds its launch-path table (and the cell FAT
/// image), so a kernel built with the xHCI lane carries the literal `/bin/xhci`.
/// An ISO without it cannot exercise the driver, and treating that as a pass
/// would be a false green.
fn iso_carries_xhci_cell() -> bool {
    std::fs::read(iso_path())
        .map(|bytes| bytes.windows(9).any(|w| w == b"/bin/xhci"))
        .unwrap_or(false)
}

fn prerequisites_ok() -> bool {
    let iso_ok = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_x86_binary())
        .arg("--version")
        .output()
        .is_ok();
    let cell_ok = iso_ok && iso_carries_xhci_cell();
    if iso_ok && !cell_ok {
        eprintln!(
            "SKIP xhci-x86: ISO does not carry /bin/xhci ({})\n  Rebuild: pwsh scripts/build-x86_64-cells.ps1, then the board-x86-pc kernel + ISO",
            iso_path()
        );
    }
    if !iso_ok {
        eprintln!(
            "SKIP xhci-x86: x86_64 ISO not built ({})\n  Run: pwsh scripts/build-x86_64-cells.ps1, then build the board-x86-pc kernel + ISO",
            iso_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP xhci-x86: qemu-system-x86_64 not on PATH");
    }
    ci_guard(iso_ok && qemu_ok && cell_ok)
}

/// The xHCI Driver Cell must bind the q35 `qemu-xhci` controller, reset the port
/// carrying the USB keyboard, and enumerate it (VID/PID of the QEMU keyboard).
#[test]
fn xhci_driver_cell_enumerates_usb_keyboard_x86() {
    if !prerequisites_ok() {
        return;
    }

    let qemu = QemuRunner::boot_x86_bios_with_xhci(&iso_path());

    for marker in [
        "[xhci] controller init ok",
        "[xhci] enumerated device vid=0x0627 pid=0x0001",
        "[xhci] USB HID keyboard ready",
    ] {
        qemu.wait_for(marker, BOOT_TIMEOUT).unwrap_or_else(|e| {
            panic!(
                "xHCI Driver Cell marker {marker:?} not seen within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: platform ECAM scan → find_pcie_device(0C:03:30) → first-memory-BAR \
                 MMIO claim → HCRST → port reset → Enable Slot → Address Device → \
                 GET_DESCRIPTOR → SET_CONFIGURATION → SET_PROTOCOL(boot) → Configure Endpoint.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });
    }

    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell prompt not reached after xHCI enumeration: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });
}

/// A keystroke injected into the QEMU USB keyboard must travel the full xHCI
/// path: interrupt-IN transfer → Transfer Event → shared HID boot-protocol
/// decode in the cell → input service producer gate → shell. Phase 03 stopped at
/// "decoded in cell"; phase 03b completes it, so this test asserts the typed
/// character is *received and executed by the shell* (`shell: command not found:
/// q`), not merely logged by the cell.
///
/// It also pins the whole point of the sub-phase: `/bin/xhci` publishes
/// `service::USB_HID_PRODUCER` and must **never** publish `service::NIC_DRIVER`.
/// The lane has no NIC device attached, so any NIC-registration marker in this
/// boot would prove the xHCI cell took over the singleton network role.
#[test]
fn xhci_injected_key_decodes_in_cell_x86() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_x86_bios_with_xhci(&iso_path());
    qemu.wait_for("[xhci] USB HID keyboard ready", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "xHCI keyboard not ready within {BOOT_TIMEOUT}s: {e}\n--- serial output ---\n{}",
                qemu.dump()
            )
        });
    // The USB HID producer role must be published (kernel-verified identity the
    // input service's gate resolves), so the injected key can be delivered.
    qemu.wait_for("[driver_cell] USB HID producer registered", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "xHCI did not publish the USB HID producer role within {BOOT_TIMEOUT}s: {e}\n\
                 Chain: sys_register_usb_hid_producer (423) → UsbDriverCap gate → \
                 service::USB_HID_PRODUCER.\n--- serial output ---\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell prompt not reached: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });

    let checkpoint = qemu.output_checkpoint();
    // Two presses, spaced so the poll loop re-arms the interrupt-IN transfer
    // between them.
    qemu.send_qemu_key("q");
    thread::sleep(Duration::from_millis(500));
    qemu.send_qemu_key("q");

    qemu.wait_for_after("[xhci] key down code=0x10", checkpoint, KEY_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "injected key never reached the xHCI HID decode within {KEY_TIMEOUT}s: {e}\n\
                 Chain: QMP send-key → usb-kbd interrupt IN → xHCI Transfer Event → \
                 driver_hid::decode_boot_report.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for_after("[xhci] key up code=0x10", checkpoint, KEY_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "release was never decoded within {KEY_TIMEOUT}s: {e}\n--- serial output ---\n{}",
                qemu.dump()
            )
        });

    // The decoded key must have reached the shell through the input service, not
    // just the cell's log. Executing the (unknown) command `q` proves the
    // character survived the full producer → input-service → shell path.
    thread::sleep(Duration::from_millis(500));
    qemu.send_qemu_key("ret");
    qemu.wait_for_after("shell: command not found: q", checkpoint, KEY_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "injected keystroke never reached the shell within {KEY_TIMEOUT}s: {e}\n\
                 Chain: xHCI decode → USB_HID_HOST frame → input service producer gate \
                 (service::USB_HID_PRODUCER) → shell line editor.\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });
    eprintln!(
        "[test] shell receipt confirmed: injected 'q' + 'ret' → \"shell: command not found: q\""
    );

    // The sub-phase contract: `/bin/xhci` never takes the singleton NIC role.
    assert!(
        !qemu.dump().contains("[driver_cell] NIC driver registered"),
        "xHCI lane has no NIC device, so no cell may publish service::NIC_DRIVER — \
         the xHCI cell must use the USB HID producer role, not the NIC role\n\
         --- serial output ---\n{}",
        qemu.dump()
    );
}

/// A boot with no keyboard attached must still reach the shell: the xHCI cell
/// observes the controller with no device, logs an idle line, and exits cleanly.
#[test]
fn xhci_boot_without_keyboard_reaches_shell_x86() {
    if !prerequisites_ok() {
        return;
    }

    let qemu = QemuRunner::boot_x86_bios_with_xhci_no_keyboard(&iso_path());
    qemu.wait_for("[xhci] no USB device attached; driver cell idle", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "xHCI cell did not report an idle machine with no keyboard: {e}\n\
                 --- serial output ---\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell prompt not reached with xHCI present but no keyboard: {e}\n\
             --- serial output ---\n{}",
            qemu.dump()
        )
    });
}

/// COM1 is the fallback input path and must keep working on the xHCI lane.
#[test]
fn xhci_com1_input_still_works_x86() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_x86_bios_with_xhci(&iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell prompt not reached: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });

    qemu.send_line("echo xhci-uart-ok");
    qemu.wait_for("xhci-uart-ok", CMD_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "COM1 input did not reach the shell on the xHCI lane: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });
}
