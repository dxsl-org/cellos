//! Plan 261103-2218 phase 06: ACPI reset, S5 poweroff and the power button.
//!
//! Every lane boots the q35 ISO through `QemuRunner::boot_x86_bios_power`
//! (no `-no-reboot`, QMP attached), so a guest reset boots again and only an
//! S5 entry ends QEMU. TCG by default; `CELLOS_X86_ACCEL=kvm` runs the same
//! lanes under KVM. `VICELL_POWER_ISO` pins the ISO (default: the one
//! `x86_iso_path` resolves). The evidence ceiling is QEMU.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_x86, x86_iso_path, QemuRunner};

const BOOT_TIMEOUT: u64 = 90;
const POWER_TIMEOUT: u64 = 30;
const BANNER: &str = "[Cellos] kernel boot v";
const PROMPT: &str = "Cellos >";

fn iso_path() -> String {
    match std::env::var("VICELL_POWER_ISO") {
        Ok(path) if !path.is_empty() => path,
        _ => {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .expect("repo root resolves");
            x86_iso_path(&root)
        }
    }
}

fn prerequisites_ok() -> bool {
    let iso_exists = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_exists {
        eprintln!("SKIP power-x86: ISO not built ({})", iso_path());
    }
    if !qemu_ok {
        eprintln!("SKIP power-x86: qemu-system-x86_64 not found");
    }
    vicell_integration_tests::ci_guard(iso_exists && qemu_ok)
}

fn expect(qemu: &QemuRunner, marker: &str, after: usize, timeout: u64, why: &str) {
    qemu.wait_for_after(marker, after, timeout).unwrap_or_else(|e| {
        panic!(
            "{why}: marker {marker:?} missing: {e}\n--- serial output ---\n{}",
            qemu.dump()
        )
    });
}

/// Boot to the shell with the power facts read and the SCI armed.
fn boot_to_shell() -> Option<QemuRunner> {
    if !prerequisites_ok() {
        return None;
    }
    let qemu = QemuRunner::boot_x86_bios_power(&iso_path());
    for marker in [
        "[acpi] power: \\_S5_ static SLP_TYPa=0 SLP_TYPb=0",
        "[acpi] sci: ACPI mode on",
        "[acpi] sci: armed",
        PROMPT,
    ] {
        expect(&qemu, marker, 0, BOOT_TIMEOUT, "first boot");
    }
    Some(qemu)
}

fn expect_exit_zero(qemu: &mut QemuRunner, why: &str) {
    let status = qemu.wait_for_exit_status(POWER_TIMEOUT);
    let dump = qemu.dump();
    assert!(
        status.is_some_and(|status| status.success()),
        "{why}: QEMU must exit with status 0 after S5, got {status:?}\n{dump}"
    );
    assert!(
        !dump.contains("[power] poweroff: machine still running"),
        "{why}: the sleep write did not take effect\n{dump}"
    );
    let evidence: Vec<&str> = dump
        .lines()
        .filter(|line| {
            ["kernel boot v", "System ", "[supervisor] power", "[power]", "[acpi] power button"]
                .iter()
                .any(|needle| line.contains(needle))
        })
        .collect();
    println!("{why}: QEMU exit {status:?}\n{}", evidence.join("\n"));
}

#[test]
fn shutdown_enters_s5_and_qemu_exits_zero() {
    let Some(mut qemu) = boot_to_shell() else {
        return;
    };
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("/bin/shutdown");
    for marker in [
        "System shutting down",
        "[supervisor] power: poweroff (Operator) froze=",
        "[supervisor] power: requesting poweroff",
        "[power] poweroff: PM1a_CNT 0x604 SLP_TYPa=0",
    ] {
        expect(&qemu, marker, checkpoint, POWER_TIMEOUT, "/bin/shutdown");
    }
    expect_exit_zero(&mut qemu, "/bin/shutdown");
}

#[test]
fn shutdown_r_resets_into_a_second_boot() {
    let Some(mut qemu) = boot_to_shell() else {
        return;
    };
    assert_eq!(qemu.output_count(BANNER), 1);
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("/bin/shutdown -r");
    for marker in [
        "System rebooting",
        "[supervisor] power: reboot (Operator) froze=",
        "[supervisor] power: requesting reboot",
        "[power] reset: FADT reset register space=1 addr=0xcf9",
    ] {
        expect(&qemu, marker, checkpoint, POWER_TIMEOUT, "/bin/shutdown -r");
    }
    expect(&qemu, BANNER, checkpoint, BOOT_TIMEOUT, "second boot");
    let second = qemu.output_checkpoint();
    expect(&qemu, PROMPT, second, BOOT_TIMEOUT, "second boot");
    let dump = qemu.dump();
    assert_eq!(qemu.output_count(BANNER), 2, "{dump}");
    assert!(
        !dump.contains("[power] reset: triple fault"),
        "the FADT reset register must reset q35 before the fallbacks\n{dump}"
    );

    // The second boot powers off as well.
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("/bin/shutdown");
    expect(
        &qemu,
        "[supervisor] power: requesting poweroff",
        checkpoint,
        POWER_TIMEOUT,
        "second boot /bin/shutdown",
    );
    expect_exit_zero(&mut qemu, "second boot /bin/shutdown");
}

#[test]
fn power_button_runs_the_supervisor_shutdown() {
    let Some(mut qemu) = boot_to_shell() else {
        return;
    };
    let checkpoint = qemu.output_checkpoint();
    assert!(
        qemu.send_qmp_system_powerdown(),
        "QMP system_powerdown refused\n{}",
        qemu.dump()
    );
    for marker in [
        "[acpi] power button: poweroff request relayed to supervisor",
        "[supervisor] power: poweroff (PowerButton) froze=",
        "[supervisor] power: requesting poweroff",
        "[power] poweroff: PM1a_CNT 0x604 SLP_TYPa=0",
    ] {
        expect(&qemu, marker, checkpoint, POWER_TIMEOUT, "power button");
    }
    expect_exit_zero(&mut qemu, "power button");
}
