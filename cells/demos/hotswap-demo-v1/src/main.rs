// SPDX-License-Identifier: MPL-2.0

//! hotswap-demo-v1 — Phase 04 hot-migration demo (old version).
//!
//! Maintains a u32 counter, replies to "inc"/"get" messages, and implements
//! the full ViStateTransfer protocol so a live upgrade to hotswap-demo-v2
//! preserves the counter value across the cell boundary.
//!
//! # Sequence
//! 1. Run v1, send "inc" N times, send "get" → receives "v1:<counter_le4>".
//! 2. Call `sys_hotswap(cell_id, "/bin/hotswap-demo-v2")` from the shell.
//! 3. v1 receives AppEvent::Snapshot → serializes counter → stashes.
//! 4. v2 spawns, receives AppEvent::Restore → deserializes counter.
//! 5. Send "get" to v2 → receives "v2:<counter_le4>" with the same counter.
//!
//! # Law 4 compliance
//! `#![forbid(unsafe_code)]` — global state is guarded by `spin::Mutex`.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use api::ViError;
use ostd::app::{AppContext, AppEvent};
use ostd::hotswap::{hotswap_key, restore_transfer, stash_transfer, ViStateTransfer};
use ostd::io::println;
use ostd::sync::Mutex;
use ostd::syscall::sys_hotswap_ready;

// spawn = true grants StateStash / StateRestore / HotSwapReady allowlist bits
// (bits 33, 34, 32) that the hotswap state-transfer protocol requires.
// The cell does NOT use SpawnCap to spawn other cells, but the bits live in the
// same allowlist group in runtime.rs:app_syscall_set.
ostd::app_entry!(spawn = true, handler = main_handler);

// ── State ─────────────────────────────────────────────────────────────────────

/// Persistent counter — the state that survives a hot-swap to v2.
struct DemoState {
    counter: u32,
}

impl DemoState {
    const fn new() -> Self {
        Self { counter: 0 }
    }
}

impl ViStateTransfer for DemoState {
    /// Schema version 1: 4-byte little-endian counter.
    const SCHEMA_VERSION: u32 = 1;

    fn serialize(&self) -> Result<Box<[u8]>, ViError> {
        Ok(Box::from(self.counter.to_le_bytes().as_slice()))
    }

    fn deserialize(version: u32, bytes: &[u8]) -> Result<Self, ViError> {
        if version != 1 || bytes.len() < 4 {
            return Err(ViError::InvalidInput);
        }
        let counter = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        Ok(Self { counter })
    }
}

// Single-threaded cell — Mutex<DemoState> ensures Sync without unsafe.
static STATE: Mutex<DemoState> = Mutex::new(DemoState::new());

// ── Event handler ─────────────────────────────────────────────────────────────

fn main_handler(ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            println("[hotswap-demo-v1] ready — send 'inc' or 'get'");
        }

        AppEvent::Snapshot { swap_id } => {
            // Old cell: serialize counter and stash it so v2 can restore.
            // hotswap_key(swap_id) must match what hotswap.rs writes — both
            // derive the same numeric key from the monotonic swap_id.
            let key = hotswap_key(swap_id);
            let state = STATE.lock();
            match stash_transfer(key, &*state) {
                Ok(()) => {
                    println("[hotswap-demo-v1] state stashed");
                }
                Err(e) => {
                    // Log and continue — kernel will proceed with an empty stash
                    // (v2 starts cold) rather than aborting the swap.
                    let _ = e;
                    println("[hotswap-demo-v1] WARN: stash failed — v2 starts cold");
                }
            }
        }

        AppEvent::Restore { key } => {
            // New instance of this cell (upgraded-to path); restore from stash.
            // key is a null-terminated decimal string of swap_id.
            let swap_id = parse_key_to_swap_id(&key);
            let stash_key = hotswap_key(swap_id);
            match restore_transfer::<DemoState>(stash_key) {
                Ok(restored) => {
                    let mut state = STATE.lock();
                    state.counter = restored.counter;
                    println("[hotswap-demo-v1] state restored");
                }
                Err(_) => {
                    // Cold start (no prior stash) — counter stays at 0.
                    println("[hotswap-demo-v1] WARN: no stash found — starting cold");
                }
            }
            // MUST be the last call in the Restore handler.
            // Unblocks the hotswap orchestrator's Step 4 wait.
            sys_hotswap_ready();
        }

        AppEvent::Message { sender_tid, data } => {
            match data.as_slice() {
                b"inc" => {
                    let mut state = STATE.lock();
                    state.counter = state.counter.saturating_add(1);
                    // Reply with raw "ok" bytes so tests can assert on a simple string.
                    ctx.send(sender_tid, b"ok").ok();
                }
                b"get" => {
                    // Reply with "v1:" + 4 LE bytes of counter so the test can assert
                    // the version prefix AND the counter value in one message.
                    let state = STATE.lock();
                    let mut resp = [0u8; 7];
                    resp[0] = b'v';
                    resp[1] = b'1';
                    resp[2] = b':';
                    resp[3..7].copy_from_slice(&state.counter.to_le_bytes());
                    ctx.send(sender_tid, &resp).ok();
                }
                _ => {
                    ctx.send(sender_tid, b"unknown").ok();
                }
            }
        }

        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
            ostd::syscall::sys_exit(0);
        }

        _ => {}
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse a null-terminated decimal ASCII key (from AppEvent::Restore) to u64.
///
/// The hotswap orchestrator writes the decimal swap_id into a 64-byte field
/// with a null terminator. We stop at the first 0x00 byte or non-ASCII-digit.
fn parse_key_to_swap_id(key: &[u8; 64]) -> u64 {
    let mut val = 0u64;
    for &b in key.iter() {
        if b == 0 || !b.is_ascii_digit() {
            break;
        }
        val = val.saturating_mul(10).saturating_add((b - b'0') as u64);
    }
    val
}
