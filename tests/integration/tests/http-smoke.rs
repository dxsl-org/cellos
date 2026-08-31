//! ostd::http + ostd::json end-to-end smoke gate.
//!
//! Boots QEMU RISC-V with the `http-smoke` cell and a host-side Python mock LLM
//! (`tools/hypha-mock-llm/mock_proxy.py`) reachable at 10.0.2.2 via SLIRP.
//!
//! This gate is intentionally HTTP-only. Default `service-net` has no
//! authenticated certificate time, so the guest's HTTPS attempt cannot exercise
//! certificate verification and its generic connect failure is not evidence.
//! Missing build, QEMU, Python, mock, or cell prerequisites skip locally and
//! fail under CI.

use std::net::TcpStream as StdTcp;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const SMOKE_TIMEOUT: u64 = 90;

// Plain mock port; the guest's HTTPS attempt is denied before TCP opens.
const HTTP_PORT: u16 = 8080;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root()
        .join("disk_v3.img")
        .to_string_lossy()
        .into_owned()
}

fn mock_script_path() -> PathBuf {
    repo_root().join("tools/hypha-mock-llm/mock_proxy.py")
}

/// Resolve a working Python 3 interpreter.
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

/// A built smoke cell is a necessary precondition for its disk entry.
fn http_smoke_on_disk() -> bool {
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
        eprintln!("SKIP http-smoke: kernel not built ({})", kernel_path());
    }
    if !disk_ok {
        eprintln!("SKIP http-smoke: disk_v3.img missing");
    }
    if !qemu_ok {
        eprintln!("SKIP http-smoke: qemu-system-riscv64 not on PATH");
    }
    if !python_ok {
        eprintln!("SKIP http-smoke: Python 3 not on PATH");
    }
    if !mock_ok {
        eprintln!("SKIP http-smoke: {} is missing", mock_script_path().display());
    }
    if !smoke_ok {
        eprintln!("SKIP http-smoke: app-http-smoke is not built");
    }

    vicell_integration_tests::ci_guard(
        kernel_ok && disk_ok && qemu_ok && python_ok && mock_ok && smoke_ok,
    )
}

struct MockProcess {
    child: Child,
}

impl Drop for MockProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawn the required plain mock and fail cleanly if it cannot own the port.
fn start_mock(python: &str, args: &[&str], port: u16) -> MockProcess {
    assert!(
        StdTcp::connect(("127.0.0.1", port)).is_err(),
        "http-smoke: required port {port} is already in use"
    );
    let mut child = Command::new(python)
        .arg(mock_script_path())
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|error| panic!("http-smoke: failed to spawn mock on {port}: {error}"));
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = child.try_wait().expect("mock process status is readable") {
            panic!("http-smoke: mock on {port} exited before bind: {status}");
        }
        if StdTcp::connect(("127.0.0.1", port)).is_ok() {
            return MockProcess { child };
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("http-smoke: mock on {port} did not bind within 8 seconds");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Prove the supported plain-HTTP round trip; HTTPS has a separate gated owner.
#[test]
fn http_smoke_e2e() {
    if !prerequisites_ok() {
        return;
    }

    let py = python_bin().expect("Python passed prerequisite gate");
    let _mock_plain = start_mock(&py, &["--plain"], HTTP_PORT);

    // Boot QEMU with SLIRP (guest sees host at 10.0.2.2).
    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());

    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "shell prompt not reached within {BOOT_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    std::thread::sleep(Duration::from_millis(500));
    qemu.send_line("http-smoke");

    qemu.wait_for("[http-smoke] done", SMOKE_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "http-smoke did not complete within {SMOKE_TIMEOUT}s: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    let output = qemu.dump();

    assert!(
        output.contains("[http-smoke] HTTP PASS"),
        "http-smoke HTTP gate failed\n--- serial output ---\n{output}"
    );
}
