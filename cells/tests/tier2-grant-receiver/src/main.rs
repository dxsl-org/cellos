//! Tier-2 grant pair — receiver cell.
//!
//! Public-syscall receive half of the phase-03 step-5 pair. It is an ordinary
//! Tier-2 (`PROTECTION_CLASS_UNTRUSTED`) private-root domain cell, distinct from
//! the owner, so its `GrantSlice` reaches the production lifecycle with a real
//! domain caller.
//!
//! Command line (staged by the shell):
//!   argv[0] = `rw` | `ro` | `unregister` | `exit`
//!   argv[1] = the owner's GrantAlloc id
//!   argv[2] = the owner's GrantRegister id
//!   argv[3] = the owner's exit-phase GrantAlloc id
//!
//! Each mode ends in a store to the address the kernel handed back, which must
//! fault *after* the owner revoked the mapping — the deliberate fault is the
//! witness the runner classifies. The owner is discovered through the public
//! `GetProcs` surface rather than by a private handshake.
//!
//! Marker grammar:
//!   S22-RV64-GRANT-PAIR-RECEIVER-BEGIN: mode=<m> ids=<a>,<b>,<c>
//!   S22-RV64-GRANT-PAIR-RECEIVER-ALLOC: OK id=<n>|DENY
//!   S22-RV64-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: DENY|MAPPED
//!   S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RO: OK|FAIL
//!   S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RW: OK|FAIL
//!   S22-RV64-GRANT-PAIR-RECEIVER-RW: OK|MISMATCH
//!   S22-RV64-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED|REUSED
//!   S22-RV64-GRANT-PAIR-RECEIVER-RO-WRITE: FAULT-EXPECTED
//!   S22-RV64-GRANT-PAIR-RECEIVER-REVOKE-FAULT: FAULT-EXPECTED
//!   S22-RV64-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: FAULT-EXPECTED
//!   S22-RV64-GRANT-PAIR-RECEIVER-EXIT-FAULT: FAULT-EXPECTED

#![no_std]
#![no_main]
#![allow(unsafe_code)]

extern crate alloc;

use alloc::format;
use ostd::io::println;
use ostd::syscall::{
    sys_exit, sys_get_procs, sys_grant_alloc, sys_grant_slice, sys_recv, sys_send, sys_yield,
    SyscallResult,
};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED
);

api::declare_syscalls![
    Log,
    Exit,
    StateRestore,
    GetProcs,
    Send,
    Recv,
    GrantAlloc,
    GrantSlice
];

ostd::cell_main!(cell_main);

const PAGE: usize = 4096;
/// Wire protocol: one byte, shared with the owner.
const WANT_RW: u8 = 1;
const WANT_RO: u8 = 2;
const WANT_UNREGISTER: u8 = 3;
const WANT_EXIT: u8 = 4;
const REQ_FREE: u8 = 5;
const REQ_UNREGISTER: u8 = 6;
const REQ_EXIT: u8 = 7;
const ACK: u8 = 0x80;
/// An address no grant record can occupy: the allocator only hands out frames
/// from RAM, far above page 1.
const IMPOSSIBLE_ID: usize = 0x1000;
/// Bounded retries for a poll that must observe a kernel-side state change.
const POLL_LIMIT: usize = 4_000_000;

fn name_eq(row: &[u8; 32], want: &[u8]) -> bool {
    let len = row.iter().position(|byte| *byte == 0).unwrap_or(row.len());
    &row[..len] == want
}

/// The owner is the live `tier2-smoke` task the runner spawned first.
fn find_owner() -> Option<usize> {
    for _ in 0..POLL_LIMIT {
        let mut rows = [api::syscall::ProcessInfo::default(); 64];
        if let Ok(count) = sys_get_procs(&mut rows) {
            for row in rows.iter().take(count) {
                if row.state != 3 && name_eq(&row.name, b"tier2-smoke") {
                    return Some(row.id);
                }
            }
        }
        sys_yield();
    }
    None
}

fn send_byte(target: usize, byte: u8) {
    let _ = sys_send(target, &[byte]);
}

/// Blocking receive of one protocol byte from `target`.
fn recv_byte_from(target: usize) -> Option<u8> {
    for _ in 0..64 {
        let mut buf = [0u8; 8];
        if let SyscallResult::Ok(sender) = sys_recv(target, &mut buf) {
            if sender == target && buf[0] != 0 {
                return Some(buf[0]);
            }
        }
    }
    None
}

/// Wait until `grant_id` no longer resolves, i.e. the kernel has revoked it.
fn wait_until_revoked(grant_id: usize) -> bool {
    for _ in 0..POLL_LIMIT {
        if sys_grant_slice(grant_id).is_none() {
            return true;
        }
        sys_yield();
    }
    false
}

/// Store to `pointer`. The store is the observation: a mapping the kernel has
/// revoked must fault, and the runner reads the fault line as the witness.
fn deliberate_store(pointer: *mut u8) {
    // SAFETY: deliberate probe. A live mapping absorbs the store; a revoked one
    // traps and terminates this cell, which is exactly what every caller of this
    // helper is asserting.
    unsafe {
        core::ptr::write_volatile(pointer, 0x5A);
    }
}

fn cell_main() {
    let args = ostd::args();
    let mode = args.first().cloned().unwrap_or_default();
    let parsed = |index: usize| -> usize {
        args.get(index)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    };
    let (id_alloc, id_reg, id_exit) = (parsed(1), parsed(2), parsed(3));

    println(&format!(
        "S22-RV64-GRANT-PAIR-RECEIVER-BEGIN: mode={mode} ids={id_alloc},{id_reg},{id_exit}"
    ));

    // A second private root allocates independently: the capability rule is
    // per-task, not a global "one domain grant" policy.
    match sys_grant_alloc(PAGE) {
        Some(id) => println(&format!("S22-RV64-GRANT-PAIR-RECEIVER-ALLOC: OK id={id}")),
        None => println("S22-RV64-GRANT-PAIR-RECEIVER-ALLOC: DENY"),
    }

    let Some(owner) = find_owner() else {
        println("S22-RV64-GRANT-PAIR-RECEIVER-OWNER: MISSING");
        sys_exit(1);
    };

    let want = match mode.as_str() {
        "rw" => Some(WANT_RW),
        "ro" => Some(WANT_RO),
        "unregister" => Some(WANT_UNREGISTER),
        "exit" => Some(WANT_EXIT),
        _ => None,
    };
    let Some(want) = want else {
        println("S22-RV64-GRANT-PAIR-RECEIVER-MODE: UNKNOWN");
        sys_exit(1);
    };
    send_byte(owner, want);
    let Some(reply) = recv_byte_from(owner) else {
        println("S22-RV64-GRANT-PAIR-RECEIVER-SHARE: NO-ACK");
        sys_exit(1);
    };
    if reply & ACK == 0 {
        println("S22-RV64-GRANT-PAIR-RECEIVER-SHARE: REFUSED");
        sys_exit(1);
    }

    match mode.as_str() {
        "rw" => {
            // An unknown id keeps the `usize::MAX` "not authorized" sentinel.
            match sys_grant_slice(IMPOSSIBLE_ID) {
                None => println("S22-RV64-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: DENY"),
                Some(_) => {
                    println("S22-RV64-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: MAPPED");
                    sys_exit(1);
                }
            }
            let Some(pointer) = sys_grant_slice(id_alloc) else {
                println("S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RW: FAIL");
                sys_exit(1);
            };
            let wrote = unsafe {
                core::ptr::write_volatile(pointer, 0x3C);
                core::ptr::read_volatile(pointer) == 0x3C
            };
            println(&format!(
                "S22-RV64-GRANT-PAIR-RECEIVER-RW: {}",
                if wrote { "OK" } else { "MISMATCH" }
            ));
            send_byte(owner, REQ_FREE);
            if recv_byte_from(owner).is_none() {
                println("S22-RV64-GRANT-PAIR-RECEIVER-FREE: NO-ACK");
                sys_exit(1);
            }
            // The revoked record must not resolve again — reused frames stay
            // private — and the mapping already handed out must fault.
            match sys_grant_slice(id_alloc) {
                None => println("S22-RV64-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED"),
                Some(_) => {
                    println("S22-RV64-GRANT-PAIR-RECEIVER-FRAME-REUSE: REUSED");
                    sys_exit(1);
                }
            }
            println("S22-RV64-GRANT-PAIR-RECEIVER-REVOKE-FAULT: FAULT-EXPECTED");
            deliberate_store(pointer);
            println("S22-RV64-GRANT-PAIR-RECEIVER-REVOKE-FAULT: WROTE");
        }
        "ro" => {
            let Some(pointer) = sys_grant_slice(id_reg) else {
                println("S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RO: FAIL");
                sys_exit(1);
            };
            // A read-only page must still be readable, or the refusal would be
            // indistinguishable from a missing mapping.
            let observed = unsafe { core::ptr::read_volatile(pointer) };
            println(&format!(
                "S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RO: OK (read {observed:#x})"
            ));
            println("S22-RV64-GRANT-PAIR-RECEIVER-RO-WRITE: FAULT-EXPECTED");
            deliberate_store(pointer);
            println("S22-RV64-GRANT-PAIR-RECEIVER-RO-WRITE: WROTE");
        }
        "unregister" => {
            let Some(pointer) = sys_grant_slice(id_reg) else {
                println("S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RO: FAIL");
                sys_exit(1);
            };
            println("S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RO: OK");
            send_byte(owner, REQ_UNREGISTER);
            if recv_byte_from(owner).is_none() {
                println("S22-RV64-GRANT-PAIR-RECEIVER-UNREGISTER: NO-ACK");
                sys_exit(1);
            }
            if wait_until_revoked(id_reg) {
                println("S22-RV64-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED");
            } else {
                println("S22-RV64-GRANT-PAIR-RECEIVER-FRAME-REUSE: REUSED");
            }
            println("S22-RV64-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: FAULT-EXPECTED");
            deliberate_store(pointer);
            println("S22-RV64-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: WROTE");
        }
        "exit" => {
            let Some(pointer) = sys_grant_slice(id_exit) else {
                println("S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RW: FAIL");
                sys_exit(1);
            };
            let wrote = unsafe {
                core::ptr::write_volatile(pointer, 0x77);
                core::ptr::read_volatile(pointer) == 0x77
            };
            println(&format!(
                "S22-RV64-GRANT-PAIR-RECEIVER-SLICE-RW: {}",
                if wrote { "OK" } else { "FAIL" }
            ));
            // The owner exits on this request; its retirement reaper must revoke
            // the receiver mapping before this cell can write the old address.
            send_byte(owner, REQ_EXIT);
            if !wait_until_revoked(id_exit) {
                println("S22-RV64-GRANT-PAIR-RECEIVER-EXIT-FAULT: STILL-MAPPED");
                sys_exit(1);
            }
            println("S22-RV64-GRANT-PAIR-RECEIVER-EXIT-FAULT: FAULT-EXPECTED");
            deliberate_store(pointer);
            println("S22-RV64-GRANT-PAIR-RECEIVER-EXIT-FAULT: WROTE");
        }
        _ => {}
    }

    println("S22-RV64-GRANT-PAIR-RECEIVER: FAIL");
    sys_exit(1);
}
