//! Proves one guest httpd remains responsive while another client's POST body is incomplete.
//! Host TCP connection alone is not an acceptance signal: check guest replies before releasing
//! the held body, then check the eventual response carries the full prompt length.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use vicell_integration_tests::{qemu_binary, QemuRunner};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn disk_path() -> PathBuf {
    std::env::var_os("CELLOS_HTTPD_TEST_DISK")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("disk_v3.img"))
}

fn prerequisites_ok() -> bool {
    let root = repo_root();
    let ready = root
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel")
        .exists()
        && disk_path().exists()
        && std::process::Command::new(qemu_binary())
            .arg("--version")
            .output()
            .is_ok();
    vicell_integration_tests::ci_guard(ready)
}

#[test]
fn incomplete_body_does_not_block_unrelated_request_or_dispatch_inference() {
    if !prerequisites_ok() {
        return;
    }
    let root = repo_root();
    let kernel = root.join("target/riscv64gc-unknown-none-elf/release/cellos-kernel");
    let disk = disk_path();
    let (mut qemu, port) = QemuRunner::boot_with_hostfwd(
        kernel.to_str().unwrap(),
        disk.to_str().unwrap(),
        8080,
    );
    qemu.wait_for("Cellos >", 90).expect("shell ready");
    qemu.wait_for("[ai] model ready", 90).expect("AI ready");
    qemu.send_line("httpd &");
    qemu.wait_for("httpd: listening on :8080", 20)
        .expect("httpd ready");

    let mut slow = TcpStream::connect(("127.0.0.1", port)).expect("slow connect");
    slow.set_read_timeout(Some(Duration::from_millis(200)))
        .expect("slow timeout");
    slow.write_all(b"POST /api/infer?max_tokens=1 HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\nConnection: close\r\n\r\nx")
        .expect("partial body");

    let started = Instant::now();
    let mut fast = TcpStream::connect(("127.0.0.1", port)).expect("fast connect");
    fast.set_read_timeout(Some(Duration::from_secs(2)))
        .expect("fast timeout");
    fast.write_all(b"GET /api/status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("status GET");
    let mut fast_response = Vec::new();
    fast.read_to_end(&mut fast_response)
        .expect("status must finish before slow body is released");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "status was held behind incomplete body"
    );
    assert!(
        fast_response.starts_with(b"HTTP/1.1 200 OK")
            && fast_response.windows(b"\"status\":\"running\"".len()).any(|w| w == b"\"status\":\"running\""),
        "wrong status response: {}",
        String::from_utf8_lossy(&fast_response)
    );

    let mut premature = [0u8; 1];
    match slow.read(&mut premature) {
        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
        Ok(0) => panic!("slow request closed before complete body"),
        Ok(_) => panic!("inference ran with an incomplete request body"),
        Err(e) => panic!("slow request failed before complete body: {e}"),
    }

    slow.set_read_timeout(Some(Duration::from_secs(20)))
        .expect("inference timeout");
    slow.write_all(&[b'x'; 99]).expect("complete body");
    let mut slow_response = Vec::new();
    slow.read_to_end(&mut slow_response)
        .expect("full prompt response");
    let text = String::from_utf8_lossy(&slow_response);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
    assert!(text.contains("\"prompt_bytes\":100"), "{text}");
}
