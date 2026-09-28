//! x86_64 full-boot integration tests.
//!
//! Mirrors the AArch64 `aarch64-boot.rs` suite for the QEMU q35 machine
//! booted from a Limine BIOS ISO.
//!
//! Prerequisites:
//!   - `qemu-system-x86_64` on PATH (or at the Windows default install path)
//!   - ISO built: `cargo build --release --target x86_64-unknown-none -p cellos-kernel`
//!                followed by `.\run-x86.ps1 -NoBuild -NoQemu`
//!                → produces `build/vicell-x86.iso`
//!   - For the two Tier-2 refusal tests: `bash scripts/build-x86_64-prod-refusal-ci.sh`,
//!     which builds the production-feature witness ISO whose embedded VIFS1
//!     carries the domain-class fixtures. Those tests skip loudly (and hard-fail
//!     under `CI=`) when it is absent, because the assertions they make are only
//!     meaningful against an image that carries such a cell.
//!
//! Tests skip gracefully when any prerequisite is absent — CI behaviour is
//! identical to the AArch64 suite.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_x86, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
const CMD_TIMEOUT: u64 = 10;

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
    let iso_exists = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_exists {
        eprintln!(
            "SKIP x86_64: ISO not built ({})\n  Run: cargo build --release --target x86_64-unknown-none -p cellos-kernel && .\\run-x86.ps1 -NoBuild -NoQemu",
            iso_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP x86_64: qemu-system-x86_64 not found (PATH or C:\\Program Files\\qemu\\)");
    }
    vicell_integration_tests::ci_guard(iso_exists && qemu_ok)
}

/// Path to the *production-feature refusal witness* ISO.
///
/// The two Tier-2 refusal tests below must be driven against an image that
/// (a) carries a domain-class cell and (b) has the production feature set, so
/// that the on-path admission control — not a missing file, and not a
/// `test-hooks` qualification — is what refuses the launch:
///
/// * a bare-name spawn prints `shell: command not found` for a *refusal* and for
///   an *absent* file alike, so "no `[domain] admitted cell` in the log" is true
///   for the wrong reason against an image with no such cell;
/// * `build/vicell-x86.iso` is the shipping image, which carries no domain-class
///   cell at all, and `build/vicell-x86-domain-test.iso` is built with
///   `test-hooks`, which *enables* admission, so neither can witness a refusal.
///
/// `scripts/build-x86_64-prod-refusal-ci.sh` builds the witness with its own
/// `CARGO_TARGET_DIR`, `EMBEDDED_OVERRIDE` and ISO root, so neither collision can
/// reach it. Override with `CELLOS_X86_PROD_REFUSAL_ISO`.
fn prod_refusal_iso_path() -> String {
    if let Ok(path) = std::env::var("CELLOS_X86_PROD_REFUSAL_ISO") {
        if !path.is_empty() {
            return path;
        }
    }
    repo_root()
        .join("build/vicell-x86-prod-refusal.iso")
        .to_string_lossy()
        .into_owned()
}

/// Prerequisite gate for the refusal tests.
///
/// Announced by name when the witness is absent: a skip that prints nothing is
/// indistinguishable from a pass, and these assertions are only meaningful
/// against that image.
fn prod_refusal_prerequisites_ok() -> bool {
    let iso = prod_refusal_iso_path();
    let iso_exists = PathBuf::from(&iso).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_exists {
        eprintln!("SKIP x86_64 domain-class refusal: witness ISO not built ({iso})");
        eprintln!("  Run: bash scripts/build-x86_64-prod-refusal-ci.sh");
    }
    if !qemu_ok {
        eprintln!("SKIP x86_64 domain-class refusal: qemu-system-x86_64 not found (PATH or C:\\Program Files\\qemu\\)");
    }
    vicell_integration_tests::ci_guard(iso_exists && qemu_ok)
}

/// The kernel must emit its boot banner on x86_64.
///
/// Verifies the kernel ELF is correctly loaded by Limine, the entry point
/// (`_start`) is reached, and COM1 output is routed to the TCP serial socket
/// before any subsystem initialisation begins.
#[test]
fn x86_kernel_banner() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("[Cellos] kernel boot v", 15)
        .unwrap_or_else(|e| {
            panic!(
                "x86_64 kernel banner missing: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The task scheduler must report it is ready before any cell is spawned.
///
/// `"Scheduler initialized"` is emitted after the frame allocator, heap,
/// page tables, APIC, HPET, and IDT have all been set up successfully.
#[test]
fn x86_scheduler_initializes() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("Scheduler initialized", 20)
        .unwrap_or_else(|e| {
            panic!(
                "x86_64 scheduler init not seen: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The embedded init ELF must be spawned successfully from the kernel ramdisk.
///
/// `"Successfully spawned init"` is logged by `main.rs` when `spawn_from_mem`
/// returns `Ok` for the init binary. A failure here means the ring-3 entry
/// path, page-table setup, or manifest parsing is broken on x86_64.
#[test]
fn x86_init_spawns() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("Successfully spawned init", 25)
        .unwrap_or_else(|e| {
            panic!(
                "x86_64 init spawn not seen: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The kernel must boot through init → config → shell and reach the
/// interactive `Cellos >` prompt on COM1.
#[test]
fn x86_boots_to_shell_prompt() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 shell prompt not reached: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
}

/// The shell must execute an interactive command over COM1.
///
/// Waits for the shell prompt, sends `echo x86-ok`, and asserts the response
/// appears. Proves the full round-trip: COM1 UART RX → shell readline →
/// built-in dispatch → UART TX → TCP serial harness.
#[test]
fn x86_echo_command() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 shell prompt not reached: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("echo x86-ok");
    qemu.wait_for("x86-ok", CMD_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 echo did not respond: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
}

/// The `ls /bin` command must return at least one entry over COM1.
///
/// Proves the VFS service cell is running under ring-3 on x86_64, and the
/// IPC path (shell → VFS cell → OP_READDIR → shell) round-trips correctly.
#[test]
fn x86_ls_command() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 shell prompt not reached: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("ls /bin");
    // Any one of the expected binaries appearing proves readdir is working.
    qemu.wait_for("shell", CMD_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 ls /bin did not respond: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
}

/// The `ps` command must list at least the init and shell tasks.
///
/// Proves the task-enumeration syscall (SysGetTaskInfo or equivalent) works
/// under ring-3 on x86_64. The scheduler and hart-local table must be
/// populated correctly for ps output to appear.
#[test]
fn x86_ps_command() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 shell prompt not reached: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("ps");
    // ps prints a task table; any numeric PID appearing proves the syscall worked.
    qemu.wait_for("init", CMD_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 ps did not respond: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
}

/// Phase-01 containment, made a *runtime* witness by phase 02: on x86_64 the
/// ordered root transition is proven only in the `test-hooks` image
/// (`domain_admission.rs::switch_ordering_qualified` is `x86_64 && test-hooks`,
/// const-asserted false for a production build), so a domain-class cell must be
/// refused, never published, never executed — and the shell must survive.
///
/// The image is the one `scripts/build-x86_64-prod-refusal-ci.sh` builds: no
/// `test-hooks`, and its embedded VIFS1 carries `/bin/tier2-smoke` — the same
/// signed, `PROTECTION_CLASS_UNTRUSTED` fixture the domain-test image admits to a
/// private root (`S22-X86-DOMAIN-LIVE`). What is asserted is therefore the whole
/// chain: the artifact is present and the loader reads and evaluates it, the
/// on-path admission control refuses the launch (`error=NotSupported`, the error
/// for `SwitchOrderingUnqualified`), no domain is published, no cell code runs,
/// and the shell survives.
#[test]
fn x86_tier2_admission_is_refused_until_switch_is_qualified() {
    if !prod_refusal_prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&prod_refusal_iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell prompt: {e}\n{}", qemu.dump()));

    // 0. The boot posture is part of the witness: the phase-02 switch-ordering
    //    gate is what keeps this build closed, which is why the denial below is
    //    `SwitchOrderingUnqualified` (→ `NotSupported`) and not a fleet-profile
    //    or missing-feature refusal. A test-hooks image would print ENABLED here
    //    and admit the cell, so this also pins that the image is a production one.
    let boot = qemu.dump();
    assert!(
        boot.contains(
            "Tier 2 admission: DISABLED (development profile, phase-02 switch-ordering gate)"
        ),
        "the refusal witness must be a production-feature x86_64 image\n--- boot output ---\n{boot}"
    );
    assert!(
        !boot.contains("Tier 2 admission: ENABLED"),
        "a test-hooks image enables Tier-2 admission and cannot witness a refusal\n--- boot output ---\n{boot}"
    );

    std::thread::sleep(std::time::Duration::from_millis(500));
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("tier2-smoke &");

    let timeout = 30;
    // 1. The *loader* saw the artifact and refused the launch. This is the
    //    assertion that cannot be satisfied by an image which merely lacks the
    //    cell: an absent file takes the same shell route and reports
    //    `error=NotFound` (`shell: command not found`), never `NotSupported`.
    qemu.wait_for_after(
        "path=/bin/tier2-smoke error=NotSupported",
        checkpoint,
        timeout,
    )
    .unwrap_or_else(|e| {
        panic!(
            "the loader must read /bin/tier2-smoke and refuse the launch with the \
             domain-admission denial (NotSupported): {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // 2. The shell must come back after the refusal.
    qemu.wait_for_after("Cellos >", checkpoint, timeout)
        .unwrap_or_else(|e| {
            panic!(
                "shell did not return after a refused Tier-2 spawn on x86_64: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 3. Nothing was published and no cell code ran.
    let output = qemu.dump();
    let after = output.get(checkpoint..).unwrap_or("");
    assert!(
        after.contains("[loader] SpawnFromPath refused: caller="),
        "the refusal must be the loader's, not the shell's file lookup\n--- output ---\n{after}"
    );
    assert!(
        !after.contains("[domain] Tier 2 Native Domain requested for"),
        "the build must carry the domain backend: a refusal from the `not(native-domains)` \
         cfg branch is not an admission decision\n--- output ---\n{after}"
    );
    assert!(
        !after.contains("[domain] admitted cell"),
        "x86_64 Tier-2 admission must stay closed in the phase-01 posture\n--- output ---\n{after}"
    );
    assert!(
        !after.contains("[tier2-smoke]"),
        "a refused domain-class cell must not execute\n--- output ---\n{after}"
    );

    // 4. Verify shell interactive
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("echo x86-tier2-ok");
    qemu.wait_for("x86-tier2-ok", timeout)
        .unwrap_or_else(|e| {
            panic!(
                "shell not responding after a refused Tier-2 spawn on x86_64: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The fault-containment fixture is a domain-class cell, so on x86_64 it must be
/// refused at admission before it can run at all; the kernel must stay alive and
/// keep serving the shell.
///
/// This one drives `/bin/tier2-exploit`, which the witness image deliberately
/// leaves **unsigned** (no `__ViCell_sig`), so the second of the two class rules
/// in `kernel/src/loader/governed_spawn.rs:60-82` is exercised as well: an
/// unsigned artifact is domain-class, and a domain-class artifact is refused on
/// this build. Same denial marker as the signed/untrusted case above.
#[test]
fn x86_tier2_fault_isolation_fixture_is_refused() {
    if !prod_refusal_prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&prod_refusal_iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell prompt: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("tier2-exploit");

    let timeout = 30;
    // 1. The loader refused the *launch* of an artifact it read.
    qemu.wait_for_after(
        "path=/bin/tier2-exploit error=NotSupported",
        checkpoint,
        timeout,
    )
    .unwrap_or_else(|e| {
        panic!(
            "the loader must read /bin/tier2-exploit and refuse the launch with the \
             domain-admission denial (NotSupported): {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // 2. The shell must come back after the refusal.
    qemu.wait_for_after("Cellos >", checkpoint, timeout)
        .unwrap_or_else(|e| {
            panic!(
                "shell did not return after a refused Tier-2 spawn on x86_64: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 3. Nothing was published and no cell code ran.
    let output = qemu.dump();
    let after = output.get(checkpoint..).unwrap_or("");
    assert!(
        after.contains("[loader] SpawnFromPath refused: caller="),
        "the refusal must be the loader's, not the shell's file lookup\n--- output ---\n{after}"
    );
    assert!(
        !after.contains("[domain] Tier 2 Native Domain requested for"),
        "the build must carry the domain backend: a refusal from the `not(native-domains)` \
         cfg branch is not an admission decision\n--- output ---\n{after}"
    );
    assert!(
        !after.contains("[domain] admitted cell"),
        "x86_64 Tier-2 admission must stay closed in the phase-01 posture\n--- output ---\n{after}"
    );
    assert!(
        !after.contains("[tier2-exploit]"),
        "a refused domain-class cell must not execute\n--- output ---\n{after}"
    );

    // 4. Verify kernel survivability: shell returns
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("echo x86-tier2-alive");
    qemu.wait_for("x86-tier2-alive", timeout)
        .unwrap_or_else(|e| {
            panic!(
                "kernel crashed or shell hung after a refused Tier-2 spawn on x86_64: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}
