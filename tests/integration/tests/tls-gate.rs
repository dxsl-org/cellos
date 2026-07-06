//! G14 TLS server certificate verification gate.
//!
//! Core invariant: the default build (`tls-ca-private`) must REJECT a public
//! HTTPS host whose cert chains to a public CA, proving verification is active
//! rather than silently no-op'd.
//!
//! Gate conditions:
//!   PASS  — `https-demo` prints `TLS handshake failed` AND net logs
//!            `certificate verification failed`.
//!   SKIP  — host has no outbound internet (TCP connect fails before TLS →
//!            `transport I/O`). Inconclusive, not a gate failure.
//!   FAIL  — `TLS handshake OK` → cert verification bypassed (ship-blocker).
//!
//! Prerequisites:
//!   cargo build --release -p vicell-kernel (RUSTFLAGS="-C relocation-model=pic")
//!   cargo build --release -p service-net       # default = tls-roots-embedded + tls-ca-private
//!   cargo build --release -p app-https-demo
//!   ./gen_disk.ps1                             # installs /bin/https-demo on the disk
//!   qemu-system-riscv64 on PATH

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
/// P02 transport deadline is 30 s; add boot + TCP connect margin.
const TLS_TIMEOUT: u64 = 90;

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

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_ok {
        eprintln!("SKIP tls-gate: kernel not built ({})", kernel_path());
        eprintln!("      Run: RUSTFLAGS=\"-C relocation-model=pic\" cargo build --release -p vicell-kernel");
    }
    if !disk_ok {
        eprintln!("SKIP tls-gate: disk_v3.img missing");
        eprintln!("      Run: ./gen_disk.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP tls-gate: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

/// G14 gate (NEG-untrusted): default build must reject a public HTTPS cert.
///
/// The default `tls-ca-private` trust anchor is a dev self-signed CA that no
/// real public server uses. Connecting to example.com:443 must fail with a
/// certificate verification reject — never a silent accept.
///
/// The contrast proves verification is enforced: the `tls-insecure` build
/// (manual gate in the runbook) accepts the same host. If the default build
/// also accepts it, `UnsecureProvider` or a broken verifier path is active.
#[test]
fn tls_gate_default_rejects_public_cert() {
    if !prerequisites_ok() {
        return;
    }
    // boot_with_fresh_disk includes VirtIO net (SLIRP), RNG (TLS entropy),
    // keyboard, and GPU — the standard G1 hardware set. Fresh-disk copy avoids
    // FAT16 partition corruption between concurrent tests.
    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());

    qemu.wait_for("ViCell >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!("shell prompt not reached within {BOOT_TIMEOUT}s: {e}\n--- output ---\n{}", qemu.dump())
    });

    // Run https-demo: connects to example.com:443 via TLS 1.3.
    qemu.send_line("https-demo");

    // Wait until the demo completes. Both success and failure print a marker.
    qemu.wait_for("[https-demo]", TLS_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "https-demo produced no output within {TLS_TIMEOUT}s: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    let output = qemu.dump();

    // FAIL: verifier was bypassed — ship-blocker.
    if output.contains("TLS handshake OK") {
        panic!(
            "G14 gate FAILED: default build accepted a public HTTPS cert.\n\
             The UnsecureProvider or a broken verifier path is active.\n\
             Expected `TLS handshake failed`; got `TLS handshake OK`.\n\
             --- serial output ---\n{output}"
        );
    }

    // SKIP: no outbound internet — TCP connect failed before TLS handshake.
    // This is inconclusive, not a verification bypass.
    if output.contains("transport I/O") && !output.contains("certificate verification failed") {
        eprintln!(
            "SKIP tls-gate: TCP connect failed before TLS (host has no outbound internet).\n\
             Re-run with QEMU SLIRP internet access to execute the full gate.\n\
             Runbook: .agents/260621-1823-g14-tls-server-auth/reports/p03-e2e-gate.md"
        );
        return;
    }

    // PASS: the verifier ran and produced a reject (not a transport timeout).
    assert!(
        output.contains("certificate verification failed"),
        "G14 gate INCONCLUSIVE: TLS handshake failed but reject reason unknown.\n\
         Expected `certificate verification failed` in net cell log;\n\
         got `transport I/O` or unknown error — fix networking and re-run.\n\
         --- serial output ---\n{output}"
    );
}
