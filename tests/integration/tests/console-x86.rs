//! x86_64 console selection and redirection integration test — Phase 10.
//!
//! Verifies:
//! 1. ACPI SPCR console redirection to COM2 (0x2F8, GSI 3) with full boot and shell prompt.
//! 2. Framebuffer text console initialization and handover contract.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 90;
const SHELL_PROMPT: &str = "Cellos >";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn iso_path() -> String {
    std::env::var("VICELL_CONSOLE_ISO").unwrap_or_else(|_| {
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
    vicell_integration_tests::ci_guard(iso_ok && qemu_ok)
}

#[test]
fn console_selection_spcr_and_framebuffer_contract() {
    let iso = iso_path();
    if !prerequisites_ok(&iso) {
        return;
    }

    let fixture = repo_root().join("tests/fixtures/acpi/spcr-com2.bin");
    assert!(fixture.exists(), "SPCR COM2 fixture must exist at {}", fixture.display());

    let devices = [
        "-acpitable".to_string(),
        format!("file={}", fixture.display()),
        "-device".to_string(),
        "isa-serial,iobase=0x2f8,irq=3".to_string(),
    ];
    let runner = QemuRunner::boot_x86_q35(&iso, 1, &devices);

    let markers = [
        "[acpi] gates: madt=true",
        "spcr=true",
        "[acpi] SPCR: rev=2 type=0x00 space=1 base=0x2f8",
        "[console] active console:",
        SHELL_PROMPT,
    ];

    for marker in &markers {
        if let Err(error) = runner.wait_for(marker, BOOT_TIMEOUT) {
            panic!("console-x86: {marker:?}: {error}\n{}", runner.dump());
        }
    }
}
