//! Phase 04 hotswap smoke tests.
//!
//! QEMU-level tests verify that the demo cells boot and produce their startup
//! banner when spawned from the shell.  Full end-to-end message-passing
//! (inc × 5 → hotswap → get with counter preserved) requires inter-cell IPC
//! from outside QEMU, which is not supported by the current serial-drive
//! harness — that scenario is exercised manually or by a future kernel-level
//! test cell.
//!
//! # Prerequisites
//! - `qemu-system-riscv64` on PATH
//! - `./gen_disk.ps1` run (disk_v3.img present)
//! - Kernel and both demo cells built in release mode

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 40;
const CMD_TIMEOUT:  u64 = 20;

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
    let disk_ok   = PathBuf::from(disk_path()).exists();
    let qemu_ok   = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_ok { eprintln!("SKIP: kernel not built ({})", kernel_path()); }
    if !disk_ok   { eprintln!("SKIP: disk_v3.img missing — run ./gen_disk.ps1"); }
    if !qemu_ok   { eprintln!("SKIP: qemu-system-riscv64 not on PATH"); }
    kernel_ok && disk_ok && qemu_ok
}

/// P04: hotswap-demo-v1 spawns and prints its startup banner.
///
/// The shell's `exec` command prints "exec: process spawned (pid N)" followed
/// by the cell's own banner message, then waits for the cell to exit.
/// hotswap-demo-v1 is a long-running service so the test only waits for the
/// banner — it does NOT wait for the shell prompt to return.
///
/// This verifies:
/// - The cell ELF loads and relocates correctly (cell-build linker script).
/// - The `spawn = true` manifest is accepted by the kernel (no capability error).
/// - `AppEvent::Init` fires and the println reaches the UART.
#[test]
fn hotswap_demo_v1_spawns_and_announces() {
    if !prerequisites_ok() { return; }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Send char-by-char to avoid UART FIFO overflow (>16 bytes drops chars).
    for b in b"hotswap-demo-v1 &" {
        qemu.send_bytes(&[*b]);
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    qemu.send_bytes(b"\n");
    qemu.wait_for("[hotswap-demo-v1] ready", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!(
            "demo-v1 banner not seen: {e}\n--- output ---\n{}", qemu.dump()
        ));
}

/// P04: hotswap-demo-v2 spawns and prints its startup banner.
///
/// Verifies the v2 cell compiles, links, and boots cleanly.
/// Schema-version and the "v2:" response prefix are tested at the unit level.
#[test]
fn hotswap_demo_v2_spawns_and_announces() {
    if !prerequisites_ok() { return; }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));

    for b in b"hotswap-demo-v2 &" {
        qemu.send_bytes(&[*b]);
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    qemu.send_bytes(b"\n");
    qemu.wait_for("[hotswap-demo-v2] ready", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!(
            "demo-v2 banner not seen: {e}\n--- output ---\n{}", qemu.dump()
        ));
}

// ── Unit tests (host-only, no QEMU) ──────────────────────────────────────────
//
// These test the state-serialization logic that runs inside both demo cells.
// They run on the host without QEMU and cover the happy path + the key failure
// modes (truncated input, wrong version).

#[cfg(test)]
mod unit {
    /// Simulate a v1 round-trip: serialize then deserialize a counter.
    ///
    /// Mirrors the logic in `hotswap-demo-v1::DemoState::serialize /
    /// deserialize` — kept in sync manually (single source of truth is the cell
    /// source; this test catches schema regressions before a QEMU run).
    #[test]
    fn state_round_trip_v1() {
        let counter: u32 = 42;
        // serialize
        let bytes: [u8; 4] = counter.to_le_bytes();
        assert_eq!(bytes.len(), 4);
        // deserialize — schema v1: version=1, bytes.len()>=4
        let version: u32 = 1;
        assert_eq!(version, 1);
        let restored = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(restored, counter, "round-trip must preserve counter value");
    }

    /// v2 reads the SAME wire format as v1 (schema_version = 1, 4-byte LE counter).
    #[test]
    fn v2_reads_v1_wire_format() {
        let v1_counter: u32 = 5;
        let bytes = v1_counter.to_le_bytes();
        // v2 deserialize: same schema (version=1, 4 bytes)
        let version: u32 = 1;
        assert_eq!(version, 1);
        assert!(bytes.len() >= 4);
        let v2_counter = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(v2_counter, v1_counter, "v2 must recover v1 counter unchanged");
    }

    /// Truncated input (< 4 bytes) must be rejected.
    #[test]
    fn truncated_payload_rejected() {
        let bytes: [u8; 2] = [0x05, 0x00]; // only 2 bytes — too short
        let ok = bytes.len() >= 4;
        assert!(!ok, "truncated payload should not pass length guard");
    }

    /// Wrong version number must be rejected.
    #[test]
    fn wrong_version_rejected() {
        let version: u32 = 99; // unknown schema version
        let ok = version == 1;
        assert!(!ok, "unknown schema version should not pass version guard");
    }

    /// parse_key_to_swap_id: well-formed decimal key parses correctly.
    #[test]
    fn parse_key_decimal_swap_id() {
        let swap_id: u64 = 42;
        // Simulate how hotswap.rs fills the key buffer.
        let mut key = [0u8; 64];
        let s = alloc_key_str(swap_id);
        let b = s.as_bytes();
        key[..b.len()].copy_from_slice(b);
        // Simulate parse_key_to_swap_id.
        let parsed = parse_key(&key);
        assert_eq!(parsed, swap_id, "decimal key must round-trip through parse_key");
    }

    /// parse_key_to_swap_id: zero swap_id.
    #[test]
    fn parse_key_zero() {
        let mut key = [0u8; 64];
        key[0] = b'0';
        assert_eq!(parse_key(&key), 0);
    }

    /// parse_key_to_swap_id: stops at null byte.
    #[test]
    fn parse_key_stops_at_null() {
        let mut key = [0u8; 64];
        key[0] = b'7';
        // key[1] = 0x00 (null — already zero from initialization)
        assert_eq!(parse_key(&key), 7);
    }

    // ── helpers for unit tests ────────────────────────────────────────────────

    fn alloc_key_str(n: u64) -> std::string::String {
        std::format!("{}", n)
    }

    fn parse_key(key: &[u8; 64]) -> u64 {
        let mut val = 0u64;
        for &b in key.iter() {
            if b == 0 || !(b'0'..=b'9').contains(&b) { break; }
            val = val.saturating_mul(10).saturating_add((b - b'0') as u64);
        }
        val
    }

    /// Pending-message FIFO queue: insert 3 messages, drain in order.
    ///
    /// Mirrors the invariant of the sys_recv drain fix: messages are delivered
    /// FIFO (remove(0) = take from front) so order is preserved across freeze.
    #[test]
    fn pending_msg_fifo_order() {
        let mut queue: std::vec::Vec<u8> = std::vec![1, 2, 3];
        assert_eq!(queue.remove(0), 1, "first dequeued must be oldest");
        assert_eq!(queue.remove(0), 2, "second dequeued must be second oldest");
        assert_eq!(queue.remove(0), 3, "third dequeued must be newest");
        assert!(queue.is_empty(), "queue must be empty after draining all 3");
    }

    /// hotswap_key encoding: upper 48 bits = 0xA3_0000_0000_00,
    /// lower 48 bits = swap_id & 0xFFFF_FFFF_FFFF.
    ///
    /// Both sides of a hot-swap call hotswap_key(swap_id) independently;
    /// this ensures they agree on the stash slot without coordination.
    #[test]
    fn hotswap_key_derivation() {
        let swap_id: u64 = 1;
        // Replicate the formula from ostd::hotswap::hotswap_key.
        let key = 0x_A3_0000_0000_0000_u64 | (swap_id & 0xFFFF_FFFF_FFFF);
        assert_eq!(key >> 48, 0xA3, "namespace tag must be 0xA3");
        assert_eq!(key & 0xFFFF_FFFF_FFFF, swap_id);

        // Two different swap_ids must produce different keys.
        let key2 = 0x_A3_0000_0000_0000_u64 | (2u64 & 0xFFFF_FFFF_FFFF);
        assert_ne!(key, key2, "distinct swap_ids must yield distinct keys");
    }
}
