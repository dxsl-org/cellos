//! AArch64 full-boot integration tests.
//!
//! Mirrors the RISC-V `boot.rs` suite for the ARM64 virt machine.
//!
//! Prerequisites:
//!   - `qemu-system-aarch64` on PATH (or in the Windows default install path)
//!   - A production-feature image: `bash scripts/gen-disk-aarch64-ci.sh` builds the
//!     kernel (`target/aarch64-prod-ci/aarch64-unknown-none-softfloat/release/
//!     cellos-kernel`), its embedded VIFS1 and `target/aarch64-prod-ci/
//!     disk_arm_virt.img` on Linux, with the cells these tests launch
//!     (`periph-demo`, `input-test`, `httpd`, `virtio-net`, …). On Windows
//!     `build-aarch64-cells.ps1` + `format-disk-arm.ps1` write the legacy in-tree
//!     paths, which are still honoured when the CI image is absent.
//!   - For the two Tier-2 refusal tests: `bash scripts/build-aarch64-prod-refusal-ci.sh`,
//!     which builds the production-feature witness image whose embedded VIFS1
//!     carries the domain-class fixtures. Those tests skip loudly (and hard-fail
//!     under `CI=`) when it is absent, because the assertions they make are only
//!     meaningful against an image that carries such a cell.
//!
//! Tests skip gracefully when any prerequisite is absent — CI behaviour is
//! identical to the RISC-V suite.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_aarch64, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
const CMD_TIMEOUT: u64 = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Path to the production-feature kernel the boot tests drive.
///
/// Resolution order:
///   1. `CELLOS_AARCH64_KERNEL` — explicit override, for a witness built elsewhere.
///   2. `target/aarch64-prod-ci/aarch64-unknown-none-softfloat/release/cellos-kernel`
///      — what `scripts/gen-disk-aarch64-ci.sh` builds on Linux, in its own
///      `CARGO_TARGET_DIR` with its own `EMBEDDED_OVERRIDE`, so no other lane can
///      clobber it and it is guaranteed to be a *production* (no `test-hooks`)
///      kernel whose VIFS1 carries the cells this suite launches.
///   3. `target/aarch64-unknown-none-softfloat/release/cellos-kernel` — the legacy
///      in-tree path written by the Windows recipes. It is written by both the
///      production lane and the test-hooks lane (`build-aarch64-test-hooks-ci.sh`
///      builds there and only then copies aside), so it is only a fallback: a
///      test-hooks kernel on that path enables Tier-2 admission and would make the
///      refusal tests below vacuous, and its VIFS1 does not carry the demo cells.
fn kernel_path() -> String {
    if let Ok(path) = std::env::var("CELLOS_AARCH64_KERNEL") {
        if !path.is_empty() {
            return path;
        }
    }
    let isolated = repo_root()
        .join("target/aarch64-prod-ci/aarch64-unknown-none-softfloat/release/cellos-kernel");
    if isolated.exists() {
        return isolated.to_string_lossy().into_owned();
    }
    repo_root()
        .join("target/aarch64-unknown-none-softfloat/release/cellos-kernel")
        .to_string_lossy()
        .into_owned()
}

/// Path to the VirtIO disk the boot tests attach.
///
/// Resolution order: `CELLOS_AARCH64_DISK` → the disk
/// `scripts/gen-disk-aarch64-ci.sh` builds beside its kernel → the repo-root
/// `disk_arm_virt.img` the Windows recipes leave behind.
fn disk_path() -> String {
    if let Ok(path) = std::env::var("CELLOS_AARCH64_DISK") {
        if !path.is_empty() {
            return path;
        }
    }
    let isolated = repo_root().join("target/aarch64-prod-ci/disk_arm_virt.img");
    if isolated.exists() {
        return isolated.to_string_lossy().into_owned();
    }
    repo_root()
        .join("disk_arm_virt.img")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel_exists = PathBuf::from(kernel_path()).exists();
    let disk_exists = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_aarch64())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_exists {
        eprintln!("SKIP aarch64: kernel not built ({})", kernel_path());
    }
    if !disk_exists {
        eprintln!("SKIP aarch64: disk_arm_virt.img missing — run .\\format-disk-arm.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP aarch64: qemu-system-aarch64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_exists && disk_exists && qemu_ok)
}

/// Path to the *production-feature refusal witness* image.
///
/// The two Tier-2 refusal tests below must be driven against an image that
/// (a) carries a domain-class cell and (b) has the production feature set, so
/// that the on-path admission control — not a missing file, and not a
/// `test-hooks` qualification — is what refuses the launch:
///
/// * a bare-name spawn prints `shell: command not found` for a *refusal* and for
///   an *absent* file alike, so "no `[domain] admitted cell` in the log" is true
///   for the wrong reason against an image with no such cell;
/// * `target/aarch64-unknown-none-softfloat/release/cellos-kernel` is written by
///   both the production lane and the test-hooks lane, and the test-hooks kernel
///   *enables* admission, so that path cannot witness a refusal either.
///
/// `scripts/build-aarch64-prod-refusal-ci.sh` builds the witness into its own
/// `CARGO_TARGET_DIR` with its own `EMBEDDED_OVERRIDE`, which is why neither
/// collision can reach it. Override with `CELLOS_AARCH64_PROD_REFUSAL_KERNEL`.
fn prod_refusal_kernel_path() -> String {
    if let Ok(path) = std::env::var("CELLOS_AARCH64_PROD_REFUSAL_KERNEL") {
        if !path.is_empty() {
            return path;
        }
    }
    repo_root()
        .join("target/aarch64-prod-refusal/aarch64-unknown-none-softfloat/release/cellos-kernel")
        .to_string_lossy()
        .into_owned()
}

/// Prerequisite gate for the refusal tests.
///
/// Announced by name when the witness is absent: a skip that prints nothing is
/// indistinguishable from a pass, and these assertions are only meaningful
/// against that image.
fn prod_refusal_prerequisites_ok() -> bool {
    let kernel_path = prod_refusal_kernel_path();
    let kernel_exists = PathBuf::from(&kernel_path).exists();
    let disk_exists = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_aarch64())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_exists {
        eprintln!("SKIP aarch64 domain-class refusal: witness kernel not built ({kernel_path})");
        eprintln!("  Run: bash scripts/build-aarch64-prod-refusal-ci.sh");
    }
    if !disk_exists {
        eprintln!("SKIP aarch64 domain-class refusal: disk_arm_virt.img missing — run .\\format-disk-arm.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP aarch64 domain-class refusal: qemu-system-aarch64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_exists && disk_exists && qemu_ok)
}

/// The kernel must boot and emit the scheduler-initialized banner, then bring up
/// all services and reach the `Cellos >` shell prompt.
#[test]
fn aarch64_boots_to_shell_prompt() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "aarch64 shell prompt not reached: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
}

/// The kernel must emit its boot banner (`[ViCell] kernel boot v`) on AArch64.
///
/// This verifies the kernel's `kmain` is entered correctly after EL2→EL1 drop
/// and the PL011 UART is initialised before any subsystem setup begins.
#[test]
fn aarch64_kernel_banner() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("[Cellos] kernel boot v", 15)
        .unwrap_or_else(|e| {
            panic!(
                "aarch64 kernel banner missing: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The task scheduler must report it is ready before any cell is spawned.
///
/// `"Scheduler initialized"` is emitted after the frame allocator, heap, page
/// tables, and interrupt controller have all been set up successfully.
#[test]
fn aarch64_scheduler_initializes() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Scheduler initialized", 20)
        .unwrap_or_else(|e| {
            panic!(
                "aarch64 scheduler init not seen: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The embedded init ELF must be spawned successfully from the kernel ramdisk.
///
/// `"Successfully spawned init"` is logged by `main.rs` when `spawn_from_mem`
/// returns `Ok` for the embedded init binary. A failure here means the EL0
/// entry path, page-table user-flag setup, or manifest parsing is broken.
#[test]
fn aarch64_init_spawns() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Successfully spawned init", 20)
        .unwrap_or_else(|e| {
            panic!(
                "aarch64 init spawn not seen: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The shell must execute an interactive command.
///
/// Waits for the shell prompt, sends `echo aarch64-ok`, and asserts the
/// response appears. Proves the full path: PL011 UART RX → shell readline →
/// built-in dispatch → UART TX → serial harness.
#[test]
fn aarch64_echo_command() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "aarch64 shell prompt not reached: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("echo aarch64-ok");
    qemu.wait_for("aarch64-ok", CMD_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "aarch64 echo did not respond: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The periph-demo cell must open GPIO PL061 and UART PL011 on AArch64.
///
/// Demos are on-demand: init no longer auto-spawns periph-demo (demo
/// philosophy — no boot-output pollution), so the test launches it from the
/// shell like a user would. It exercises the PL061 GPIO controller at
/// 0x0903_0000 and the PL011 UART at 0x0900_0000 on the QEMU ARM virt
/// machine. It also proves the pinned child receives its worker sentinel,
/// preventing the demo from recursively spawning itself.
///
/// Prerequisites: `/bin/periph-demo` in the aarch64 embedded ramdisk
/// (scripts/build-aarch64-cells.ps1).
#[test]
fn aarch64_periph_demo_gpio() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n--- output ---\n{}", qemu.dump()));

    qemu.send_line("periph-demo &");
    qemu.wait_for("[periph-demo] GPIO PL061 opened", 30)
        .unwrap_or_else(|e| {
            panic!(
                "periph-demo GPIO not seen: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for("[periph-demo] UART PL011 opened", CMD_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "periph-demo UART not seen: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for(
        "[periph-demo] spawning pinned-poll cell on hart 0",
        CMD_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "periph-demo pinned spawn not seen: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    qemu.wait_for(
        "[periph-demo] pinned-poll worker active on hart 0",
        CMD_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "periph-demo pinned worker not seen: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
}

/// UART → input-service → app delivery on AArch64.
///
/// ARM64 QEMU virt has no virtio-keyboard-device — the only keyboard path is
/// the PL011 serial line.  This test exercises the full chain:
///
///   TCP socket → QEMU PL011 RX → viConsole::poll() →
///   relay_ascii_to_input() → input service (EV_ASCII) → dispatcher →
///   input-test AppContext
///
/// The input service deliberately does not log per-event (it would bury the
/// shell prompt), so the only observable marker is the app-side delivery
/// (`[input-test] input ok`).
///
/// Prerequisites: `/bin/input` + `/bin/input-test` in the aarch64 embedded
/// ramdisk (scripts/build-aarch64-cells.ps1).
#[test]
fn aarch64_uart_input_delivery() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_aarch64_with_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n--- output ---\n{}", qemu.dump()));

    // Demos are on-demand: spawn input-test from the shell (mirrors the riscv
    // `input_bare_cell` test).
    qemu.send_line("input-test &");

    // Wait for input-test to acquire focus (retries in a yield loop until the
    // input service is registered and grants focus).
    qemu.wait_for("[input-test] focus granted", 30)
        .unwrap_or_else(|e| {
            panic!(
                "input-test did not claim focus: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // Settle: let input-test's AppContext event loop park in sys_recv before
    // we inject.  Mirrors the 300ms pause used in `input_bare_cell`.
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Inject a single printable byte — no trailing newline to avoid a
    // spurious second key event from the Enter character.
    qemu.send_bytes(b"a");

    // Assert the app received the event (UART relay → input service →
    // dispatcher → input-test).
    qemu.wait_for("[input-test] input ok", 15)
        .unwrap_or_else(|e| {
            panic!(
                "input-test did not receive key: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The httpd web server cell must serve HTTP/1.1 requests over VirtIO-Net on AArch64.
///
/// Exercises the full network stack:
///   QEMU virtio-net-device (SLIRP) -> driver-virtio-net -> service-net (smoltcp + DHCP) ->
///   service-httpd (port 8080) -> host HTTP client via hostfwd.
#[test]
fn aarch64_httpd_web_server_serves_requests() {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    if !prerequisites_ok() {
        return;
    }

    let (mut qemu, host_port) =
        QemuRunner::boot_aarch64_with_hostfwd(&kernel_path(), &disk_path(), 8080);

    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n--- output ---\n{}", qemu.dump()));

    qemu.wait_for("DHCP acquired", 30)
        .unwrap_or_else(|e| panic!("DHCP not acquired: {e}\n--- output ---\n{}", qemu.dump()));

    std::thread::sleep(Duration::from_millis(500));

    // Spawn httpd in the background
    qemu.send_line("httpd &");
    qemu.wait_for("httpd: listening on :8080", 15)
        .unwrap_or_else(|e| panic!("httpd did not listen: {e}\n--- output ---\n{}", qemu.dump()));

    std::thread::sleep(Duration::from_millis(300));

    // 1. Test GET / -> HTML Dashboard
    let mut stream = TcpStream::connect(format!("127.0.0.1:{host_port}"))
        .unwrap_or_else(|e| panic!("host connect to httpd failed: {e}\n{}", qemu.dump()));
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("write GET /");
    stream.flush().expect("flush");

    let mut response = Vec::new();
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let _ = stream.read_to_end(&mut response);

    let body = String::from_utf8_lossy(&response);
    assert!(
        body.contains("Cellos Mini-Server Dashboard"),
        "response did not contain dashboard title\n--- response ---\n{body}\n--- QEMU ---\n{}",
        qemu.dump()
    );
    assert!(
        body.contains("HTTP/1.1 200 OK"),
        "response was not 200 OK\n--- response ---\n{body}"
    );

    std::thread::sleep(Duration::from_millis(300));

    // 2. Test GET /api/system -> REST API JSON
    let mut stream2 = TcpStream::connect(format!("127.0.0.1:{host_port}"))
        .unwrap_or_else(|e| panic!("host connect 2 failed: {e}\n{}", qemu.dump()));
    stream2
        .write_all(b"GET /api/system HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("write GET /api/system");
    stream2.flush().expect("flush");

    let mut response2 = Vec::new();
    stream2.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let _ = stream2.read_to_end(&mut response2);

    let body2 = String::from_utf8_lossy(&response2);
    assert!(
        body2.contains(r#""status":"running""#),
        "response did not contain status:running\n--- response ---\n{body2}"
    );
    assert!(
        body2.contains(r#""arch":"aarch64""#),
        "response did not contain arch:aarch64\n--- response ---\n{body2}"
    );
}

/// Phase-01 containment, made a *runtime* witness by phase 02: Tier-2 admission
/// stays closed on every production AArch64 image, and the refusal is observed
/// against a production-feature image that really carries a domain-class cell.
///
/// Phase 02 moved the root write inside `Context::switch_with_root` and proved
/// the root-writing path plus a real domain entry on one CPU *in the test-hooks
/// image* (`domain_admission.rs::switch_ordering_qualified` is `aarch64 &&
/// test-hooks`, const-asserted false for a production build), so a production
/// refusal is a deliberate policy decision rather than an unqualified mechanism.
///
/// The image is the one `scripts/build-aarch64-prod-refusal-ci.sh` builds: no
/// `test-hooks`, and its embedded VIFS1 carries `/bin/tier2-smoke` — the same
/// signed, `PROTECTION_CLASS_UNTRUSTED` fixture the test-hooks lane admits to a
/// private root. What this test asserts is therefore the whole chain: the
/// artifact is present and the loader reads and evaluates it, the on-path
/// admission control refuses the launch (`error=NotSupported`, the error for
/// `SwitchOrderingUnqualified`), no domain is published, no cell code runs, and
/// the shell survives.
#[test]
fn aarch64_tier2_admission_is_refused_until_switch_is_qualified() {
    if !prod_refusal_prerequisites_ok() {
        return;
    }
    let mut qemu =
        QemuRunner::boot_aarch64_with_disk(&prod_refusal_kernel_path(), &disk_path());
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
        "the refusal witness must be a production-feature AArch64 image\n--- boot output ---\n{boot}"
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
                "shell did not return after a refused Tier-2 spawn on AArch64: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 3. No domain was published and no cell code ran.
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
        "AArch64 Tier-2 admission must stay closed in the phase-01 posture\n--- output ---\n{after}"
    );
    assert!(
        !after.contains("[tier2-smoke]"),
        "a refused domain-class cell must not execute\n--- output ---\n{after}"
    );

    // 4. Verify shell interactive
    std::thread::sleep(std::time::Duration::from_millis(500));
    for b in b"echo aarch64-tier2-ok\n" {
        qemu.send_bytes(&[*b]);
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    qemu.wait_for("aarch64-tier2-ok", timeout)
        .unwrap_or_else(|e| {
            panic!(
                "shell not responding after refused Tier-2 spawn on AArch64: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

/// The fault-containment fixture is a domain-class cell, so on AArch64 it must be
/// refused at admission before it can run at all; the kernel must stay alive and
/// keep serving the shell.
///
/// This one drives `/bin/tier2-exploit`, which the witness image deliberately
/// leaves **unsigned** (no `__ViCell_sig`), so the second of the two class rules
/// in `kernel/src/loader/governed_spawn.rs:60-82` is exercised as well: an
/// unsigned artifact is domain-class, and a domain-class artifact is refused on
/// this build. Same denial marker as the signed/untrusted case above.
#[test]
fn aarch64_tier2_fault_isolation_fixture_is_refused() {
    if !prod_refusal_prerequisites_ok() {
        return;
    }
    let mut qemu =
        QemuRunner::boot_aarch64_with_disk(&prod_refusal_kernel_path(), &disk_path());
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
                "shell did not return after a refused Tier-2 spawn on AArch64: {e}\n--- output ---\n{}",
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
        "AArch64 Tier-2 admission must stay closed in the phase-01 posture\n--- output ---\n{after}"
    );
    assert!(
        !after.contains("[tier2-exploit]"),
        "a refused domain-class cell must not execute\n--- output ---\n{after}"
    );

    // 4. Verify kernel survivability: shell returns
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("echo tier2-aarch64-alive");
    qemu.wait_for("tier2-aarch64-alive", timeout)
        .unwrap_or_else(|e| {
            panic!(
                "kernel crashed or shell hung after a refused Tier-2 spawn on AArch64: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}
