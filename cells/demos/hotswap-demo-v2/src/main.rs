// SPDX-License-Identifier: MPL-2.0

//! hotswap-demo-v2 — Phase 04 hot-migration demo (upgraded version).
//!
//! Replaces hotswap-demo-v1 via live hot-swap. Reads the v1 counter from the
//! kernel stash (schema v1 is forward-compatible) and responds to "get" with
//! a "v2:" prefix so callers can verify both the counter value AND that the
//! replacement cell is running.
//!
//! Schema v1 wire format (4-byte LE counter) is UNCHANGED — v2 reads v1 data
//! directly without migration. If the schema ever changes, bump SCHEMA_VERSION
//! and add a migration branch in `deserialize`.
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

ostd::app_entry!(spawn = true, handler = main_handler);

// ── State ─────────────────────────────────────────────────────────────────────

/// Same layout as v1's DemoState; SCHEMA_VERSION = 1 so it reads v1 stash data.
struct DemoState {
    counter: u32,
}

impl DemoState {
    const fn new() -> Self {
        Self { counter: 0 }
    }
}

impl ViStateTransfer for DemoState {
    /// Schema version matches v1 — v2 reads v1 stash data without migration.
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

static STATE: Mutex<DemoState> = Mutex::new(DemoState::new());

// ── Event handler ─────────────────────────────────────────────────────────────

fn main_handler(ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            println("[hotswap-demo-v2] ready — send 'inc' or 'get'");
        }

        AppEvent::Snapshot { swap_id } => {
            let key = hotswap_key(swap_id);
            let state = STATE.lock();
            match stash_transfer(key, &*state) {
                Ok(()) => println("[hotswap-demo-v2] state stashed"),
                Err(_) => println("[hotswap-demo-v2] WARN: stash failed — next v starts cold"),
            }
        }

        AppEvent::Restore { key } => {
            let swap_id = parse_key_to_swap_id(&key);
            let stash_key = hotswap_key(swap_id);
            match restore_transfer::<DemoState>(stash_key) {
                Ok(restored) => {
                    let mut state = STATE.lock();
                    state.counter = restored.counter;
                    println("[hotswap-demo-v2] state restored from v1");
                }
                Err(_) => {
                    println("[hotswap-demo-v2] WARN: no stash found — starting cold");
                }
            }
            sys_hotswap_ready();
        }

        AppEvent::Message { sender_tid, data } => {
            match data.as_slice() {
                b"inc" => {
                    let mut state = STATE.lock();
                    state.counter = state.counter.saturating_add(1);
                    ctx.send(sender_tid, b"ok").ok();
                }
                b"get" => {
                    // v2 prefix distinguishes this version from v1 in tests.
                    let state = STATE.lock();
                    let mut resp = [0u8; 7];
                    resp[0] = b'v';
                    resp[1] = b'2';
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
