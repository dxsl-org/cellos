//! Phase 13 step 4: the kernel's policy for the death of trusted init.
//!
//! The ISO comes from `scripts/build-x86_64-init-death-ci.sh`: the canonical
//! headless image with `init` built under the `init-death-fixture` test hook,
//! which faults right after "Init: OS ready.". The kernel must write exactly one
//! `[init-death] … reason=fault … action=reset` record, then reset the machine;
//! QEMU runs without `-no-reboot` (`QemuRunner::boot_x86_bios_power`, as
//! `power-x86` does), so the reset shows as a second boot banner.
//! `VICELL_INIT_DEATH_ISO` pins the ISO (default: the builder's output path).

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_x86, QemuRunner};

const BOOT_TIMEOUT: u64 = 90;
const DEATH_TIMEOUT: u64 = 30;
const BANNER: &str = "[Cellos] kernel boot v";
const OS_READY: &str = "Init: OS ready.";
const FIXTURE: &str = "Init: init-death fixture faulting after OS ready.";
const RECORD: &str = "[init-death] tid=";

fn iso_path() -> String {
    match std::env::var("VICELL_INIT_DEATH_ISO") {
        Ok(path) if !path.is_empty() => path,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../build/vicell-x86-init-death.iso")
            .to_string_lossy()
            .into_owned(),
    }
}

fn prerequisites_ok() -> bool {
    let iso_exists = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_exists {
        eprintln!(
            "SKIP init-death-x86: ISO not built ({}); run scripts/build-x86_64-init-death-ci.sh",
            iso_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP init-death-x86: qemu-system-x86_64 not found");
    }
    vicell_integration_tests::ci_guard(iso_exists && qemu_ok)
}

fn expect(qemu: &QemuRunner, marker: &str, after: usize, timeout: u64, why: &str) {
    qemu.wait_for_after(marker, after, timeout)
        .unwrap_or_else(|e| {
            panic!(
                "{why}: marker {marker:?} missing: {e}\n--- serial output ---\n{}",
                qemu.dump()
            )
        });
}

#[test]
fn init_fault_records_and_resets_into_a_second_boot() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios_power(&iso_path());
    expect(&qemu, BANNER, 0, BOOT_TIMEOUT, "first boot");
    expect(&qemu, OS_READY, 0, BOOT_TIMEOUT, "first boot");
    expect(&qemu, FIXTURE, 0, DEATH_TIMEOUT, "fixture");
    expect(&qemu, RECORD, 0, DEATH_TIMEOUT, "init-death record");

    let dump = qemu.dump();
    let at = dump.find(RECORD).expect("record seen above");
    let record = dump[at..].lines().next().unwrap_or_default();
    for field in [
        " cell=",
        " gen=",
        " reason=fault",
        " pc=0x",
        " action=reset",
    ] {
        assert!(
            record.contains(field),
            "record {record:?} lacks {field:?}\n{dump}"
        );
    }

    // The serial output is one string and a checkpoint a byte offset: a banner
    // after the record is the second boot, whatever the timing of the reset.
    expect(
        &qemu,
        BANNER,
        at,
        BOOT_TIMEOUT,
        "second boot after the reset",
    );
    let dump = qemu.dump();
    let second = at + dump[at..].find(BANNER).expect("second banner seen above");
    assert_eq!(
        dump[..second].matches(RECORD).count(),
        1,
        "the first boot must write exactly one [init-death] record\n{dump}"
    );
    assert!(
        dump[at..second].contains("[power] reset:"),
        "the kernel's ACPI reset must run between the record and the second boot\n{dump}"
    );
    let evidence: Vec<&str> = dump
        .lines()
        .filter(|line| {
            [
                BANNER,
                OS_READY,
                FIXTURE,
                RECORD,
                "[power] reset",
                "[fault] Cell",
            ]
            .iter()
            .any(|needle| line.contains(needle))
        })
        .collect();
    println!("init-death-x86 evidence:\n{}", evidence.join("\n"));
}
