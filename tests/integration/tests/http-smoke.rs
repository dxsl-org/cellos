//! ostd::http + ostd::json end-to-end smoke gate.
//!
//! Boots QEMU RISC-V with the `http-smoke` cell and a host-side Python mock LLM
//! (`tools/hypha-mock-llm/mock_proxy.py`) reachable at 10.0.2.2 via SLIRP.
//!
//! Gate conditions:
//!   PASS  — `[http-smoke] HTTP PASS` seen in serial output
//!           `[http-smoke] HTTPS PASS` seen (soft: skipped if TLS mock unavailable)
//!   SKIP  — prerequisites not met (kernel/disk/qemu/python/http-smoke missing)
//!   FAIL  — `[http-smoke] HTTP FAIL` seen OR neither PASS nor FAIL within timeout
//!
//! Prerequisites:
//!   cargo build --release -p vicell-kernel  (RUSTFLAGS="-C relocation-model=pic")
//!   cargo build --release -p app-http-smoke
//!   ./gen_disk.ps1                           (installs /bin/http-smoke on the disk)
//!   qemu-system-riscv64 on PATH
//!   python (or python3) on PATH — with optional `cryptography` package for TLS mock

use std::net::TcpStream as StdTcp;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const SMOKE_TIMEOUT: u64 = 90;

// Ports the smoke cell connects to (must match cells/demos/http-smoke/src/main.rs).
const HTTP_PORT: u16 = 8080;
const HTTPS_PORT: u16 = 8443;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/vicell-kernel")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root().join("disk_v3.img").to_string_lossy().into_owned()
}

fn mock_script_path() -> PathBuf {
    repo_root().join("tools/hypha-mock-llm/mock_proxy.py")
}

/// Resolve the Python interpreter: try `python` first, then `python3`.
fn python_bin() -> Option<String> {
    for name in &["python", "python3"] {
        if Command::new(name)
            .args(["--version"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(name.to_string());
        }
    }
    None
}

/// Disk contains `/bin/http-smoke` if gen_disk.ps1 was run after the build.
fn http_smoke_on_disk() -> bool {
    // Probe via the write-cell-table: the cell table has a fixed-size header that's
    // hard to parse here. Instead we heuristically check that http-smoke was built,
    // which is necessary for gen_disk to include it.
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/http-smoke")
        .exists()
}

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok = PathBuf::from(disk_path()).exists();
    let qemu_ok = Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    let python_ok = python_bin().is_some();
    let mock_ok = mock_script_path().exists();
    let smoke_ok = http_smoke_on_disk();

    if !kernel_ok {
        eprintln!(
            "SKIP http-smoke: kernel not built ({})\n  Run: RUSTFLAGS=\"-C relocation-model=pic\" cargo build --release -p vicell-kernel",
            kernel_path()
        );
    }
    if !disk_ok {
        eprintln!("SKIP http-smoke: disk_v3.img missing — run ./gen_disk.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP http-smoke: qemu-system-riscv64 not on PATH");
    }
    if !python_ok {
        eprintln!("SKIP http-smoke: python / python3 not on PATH");
    }
    if !mock_ok {
        eprintln!("SKIP http-smoke: mock_proxy.py not found at {}", mock_script_path().display());
    }
    if !smoke_ok {
        eprintln!(
            "SKIP http-smoke: http-smoke cell not built\n  Run: cargo build --release -p app-http-smoke && ./gen_disk.ps1"
        );
    }

    kernel_ok && disk_ok && qemu_ok && python_ok && mock_ok && smoke_ok
}

/// Spawn the mock proxy on the host and wait until it accepts TCP connections.
///
/// Returns the child process (caller must keep it alive) or None if the mock
/// failed to bind within the timeout (e.g. TLS mock when `cryptography` is
/// absent and no cert files exist).
fn start_mock(python: &str, args: &[&str], port: u16) -> Option<Child> {
    let script = mock_script_path();
    let mut cmd = Command::new(python);
    cmd.arg(script.as_os_str())
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = cmd.spawn().ok()?;

    // Poll the port until it accepts a connection or we time out.
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if StdTcp::connect(format!("127.0.0.1:{port}")).is_ok() {
            return Some(child);
        }
        if Instant::now() > deadline {
            eprintln!("WARN http-smoke: mock on :{port} did not bind within 8 s — skipping");
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// G1 gate: `ostd::http` (HTTP + HTTPS) round-trip smoke over the mock LLM.
///
/// HTTP path is a hard gate (must pass). HTTPS path is soft-skipped when the
/// TLS mock is unavailable (no `cryptography` package and no pre-generated cert).
#[test]
fn http_smoke_e2e() {
    if !prerequisites_ok() {
        return;
    }

    let py = python_bin().unwrap();

    // Start plain HTTP mock (port 8080) — required.
    let _mock_plain = match start_mock(&py, &["--plain"], HTTP_PORT) {
        Some(c) => c,
        None => {
            panic!("http-smoke: plain HTTP mock (:{HTTP_PORT}) failed to start — cannot run test");
        }
    };

    // Start TLS mock (port 8443) — soft: skip HTTPS assertion if unavailable.
    let tls_mock_running = start_mock(&py, &[], HTTPS_PORT).is_some();
    if !tls_mock_running {
        eprintln!(
            "WARN http-smoke: TLS mock (:{HTTPS_PORT}) unavailable — HTTPS assertion skipped.\n\
             Install `pip install cryptography` and re-run to enable the HTTPS gate."
        );
    }

    // Boot QEMU with SLIRP (guest sees host at 10.0.2.2).
    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());

    qemu.wait_for("ViCell >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell prompt not reached within {BOOT_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // Small settle delay so init finishes spawning services before we send.
    std::thread::sleep(Duration::from_millis(500));
    qemu.send_line("http-smoke");

    // Wait until the smoke cell prints its final line.
    qemu.wait_for("[http-smoke] done", SMOKE_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "http-smoke did not complete within {SMOKE_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    let output = qemu.dump();

    // ── HTTP gate (hard) ──────────────────────────────────────────────────────
    assert!(
        output.contains("[http-smoke] HTTP PASS"),
        "http-smoke HTTP gate FAILED.\n\
         Expected `[http-smoke] HTTP PASS` in serial output.\n\
         --- serial output ---\n{output}"
    );

    // ── HTTPS gate (soft) ─────────────────────────────────────────────────────
    if tls_mock_running {
        assert!(
            output.contains("[http-smoke] HTTPS PASS"),
            "http-smoke HTTPS gate FAILED.\n\
             TLS mock was running on :{HTTPS_PORT} but HTTPS path did not pass.\n\
             Check TlsStream::connect / HttpClient<TlsStream> implementation.\n\
             --- serial output ---\n{output}"
        );
    } else if output.contains("[http-smoke] HTTPS PASS") {
        // Bonus: HTTPS passed even without explicit mock (unlikely but accept it).
        eprintln!("INFO http-smoke: HTTPS PASS (unexpected — TLS mock was not confirmed running)");
    }
}
