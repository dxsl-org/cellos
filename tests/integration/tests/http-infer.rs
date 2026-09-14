//! HTTP → inference → response gate: the G2 Level A demo pipeline on real components.
//!
//! Boots the canonical RV64 image, spawns `httpd` from the shell, and drives `POST /api/infer` from
//! the host through QEMU's `hostfwd`. The request path is the shipped one end to end:
//!
//!   host HTTP client → SLIRP hostfwd → service-httpd → `AiClient` (typed IPC) → inference service
//!   → GGUF weights in the model cell → JSON reply
//!
//! What the assertions cover: the endpoint answers 200 with a JSON body that names the model, reports
//! the token count it generated, and carries non-empty text; `?max_tokens=` is honoured; and the AI
//! service reports itself ready on the serial log. What they deliberately do not cover: text quality
//! (the canonical image carries the deterministic fixture, whose output is not language), streaming
//! (one reply per request), and any P99/latency claim — the QEMU serial is the evidence for those.
//!
//! Prerequisites, skipped locally when absent and required in CI: a built RV64 kernel
//! (`target/riscv64gc-unknown-none-elf/release/cellos-kernel`), `disk_v3.img` from `gen_disk.ps1`
//! with `/bin/httpd` and the model in the cell-store, and a `qemu-system-riscv64` on PATH.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 90;
const LISTEN_TIMEOUT: u64 = 20;
const HTTP_TIMEOUT_SECS: u64 = 20;
/// Port `httpd` binds inside the guest.
const GUEST_PORT: u16 = 8080;

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

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_ok {
        eprintln!(
            "SKIP http-infer: kernel not built ({})\n  Run: cargo build --release -p cellos-kernel",
            kernel_path()
        );
    }
    if !disk_ok {
        eprintln!(
            "SKIP http-infer: disk image not built ({})\n  Run: gen_disk.ps1",
            disk_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP http-infer: qemu-system-riscv64 not on PATH");
    }
    // Host/QEMU prerequisites missing must fail — not silently pass — in CI, where the
    // canonical image is built by the same job.
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

/// One HTTP/1.1 request; returns the raw response bytes.
fn http_request(host_port: u16, request: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{host_port}"))
        .unwrap_or_else(|e| panic!("host connect to httpd failed: {e}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(HTTP_TIMEOUT_SECS)))
        .ok();
    stream.write_all(request).expect("write request");
    stream.flush().expect("flush");
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    response
}

fn post_infer(host_port: u16, path: &str, prompt: &str) -> String {
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{prompt}",
        prompt.len()
    );
    String::from_utf8_lossy(&http_request(host_port, request.as_bytes())).into_owned()
}

#[test]
fn http_infer_serves_a_completion_from_the_model_cell() {
    if !prerequisites_ok() {
        return;
    }

    let (mut qemu, host_port) =
        QemuRunner::boot_with_hostfwd(&kernel_path(), &disk_path(), GUEST_PORT);

    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n--- output ---\n{}", qemu.dump()));
    qemu.wait_for("[ai] model ready", BOOT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "inference service not ready: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    qemu.send_line("httpd &");
    qemu.wait_for("httpd: listening on :8080", LISTEN_TIMEOUT)
        .unwrap_or_else(|e| panic!("httpd did not listen: {e}\n--- output ---\n{}", qemu.dump()));
    std::thread::sleep(Duration::from_millis(300));

    // 1. The default request: prompt in the body, JSON completion out.
    let response = post_infer(host_port, "/api/infer", "Once upon a time");
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "endpoint did not answer 200\n--- response ---\n{response}\n--- QEMU ---\n{}",
        qemu.dump()
    );
    assert!(
        response.contains("\"model\":\"tiny-llama-64\""),
        "reply did not name the resident model\n--- response ---\n{response}"
    );
    let token_count = response
        .split("\"tokens\":")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    assert!(
        (1..=24).contains(&token_count),
        "reply reported an invalid default-bounded token count: {token_count}\n--- response ---\n{response}"
    );
    let text = response
        .split("\"text\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_default();
    assert!(
        !text.is_empty(),
        "reply carried no generated text\n--- response ---\n{response}"
    );
    assert!(
        response.contains("\"finish\":\"Stop\"") || response.contains("\"finish\":\"Length\""),
        "reply did not report a terminal finish reason\n--- response ---\n{response}"
    );

    // 2. `?max_tokens=` is honoured, and the second request reuses the same service session table.
    let limited = post_infer(host_port, "/api/infer?max_tokens=4", "Once upon a time");
    assert!(
        limited.starts_with("HTTP/1.1 200 OK"),
        "second request did not answer 200\n--- response ---\n{limited}"
    );
    assert!(
        limited.contains("\"tokens\":4"),
        "max_tokens was not honoured\n--- response ---\n{limited}"
    );

    // 3. A body-less request is a caller error, not a crash.
    let bad = post_infer(host_port, "/api/infer", "");
    assert!(
        bad.starts_with("HTTP/1.1 400"),
        "empty prompt must be a 400\n--- response ---\n{bad}"
    );

    // 4. The HTTP front door rejects a prompt before it can exceed the AI IPC limit.
    let oversized = "x".repeat(2049);
    let too_large = post_infer(host_port, "/api/infer", &oversized);
    assert!(
        too_large.starts_with("HTTP/1.1 400") && too_large.contains("prompt is too large"),
        "oversized prompt must be rejected before inference\n--- response ---\n{too_large}"
    );

    qemu.wait_for("[ai-test] PASS", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("oracle did not pass: {e}\n--- QEMU ---\n{}", qemu.dump()));
}
