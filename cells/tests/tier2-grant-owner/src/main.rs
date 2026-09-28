//! Tier-2 grant pair — owner (producer) cell.
//!
//! Public-syscall producer half of the phase-03 step-5 pair. It is an ordinary
//! Tier-2 (`PROTECTION_CLASS_UNTRUSTED`) private-root domain cell, so every
//! `Grant*` call below reaches the production `handle_syscall` lifecycle with a
//! real domain caller — not a synthetic kernel task.
//!
//! The owner allocates an identity-mapped RW owner page through both
//! `GrantAlloc` and `GrantRegister` and proves the returned pointer is writable
//! by the owner. It then serves five receiver generations over IPC, one per
//! property the phase must witness:
//!
//!   1. `rw`         — the receiver writes the ReadWrite mapping, the owner
//!                     `GrantFree`s it, the receiver's old address faults;
//!   2. `ro`         — a ReadOnly mapping; the receiver's deliberate write faults;
//!   3. `downgrade`  — the receiver takes a ReadWrite mapping of a third grant,
//!                     then the owner re-shares the *same* grant ReadOnly to the
//!                     *same* recipient; the receiver's old writable PTE must be
//!                     gone (its store faults) while a read of the address still
//!                     returns the byte the ReadWrite phase wrote;
//!   4. `unregister` — the owner `GrantUnregister`s, the receiver's address faults;
//!   5. `exit`       — the owner exits, its reaper revokes, the receiver faults.
//!
//! Denials that stay denials are asserted here too: a non-private-root peer and
//! a WriteOnly domain pair both keep the phase-01 sentinel.
//!
//! Marker grammar (the QEMU runner greps these verbatim):
//!   S22-RV64-GRANT-PAIR-OWNER-ALLOC: OK id=<n>|DENY
//!   S22-RV64-GRANT-PAIR-OWNER-REGISTER: OK id=<n>|DENY
//!   S22-RV64-GRANT-PAIR-OWNER-MAPPED: OK|MISMATCH*
//!   S22-RV64-GRANT-PAIR-OWNER-REG-MAPPED: OK|MISMATCH*
//!   S22-RV64-GRANT-PAIR-OWNER-SHARE-FOREIGN: DENY|ALLOWED
//!   S22-RV64-GRANT-PAIR-OWNER-SHARE-WO: DENY|ALLOWED
//!   S22-RV64-GRANT-PAIR-OWNER-FREE: OK|FAIL
//!   S22-RV64-GRANT-PAIR-OWNER-UNREGISTER: OK|FAIL
//!   S22-RV64-GRANT-PAIR-OWNER-DOWNGRADE-RESHARE: OK|FAIL
//!   S22-RV64-GRANT-PAIR-OWNER-EXIT: OK
//!   S22-RV64-GRANT-PAIR-HANDOFF id1=<n> id2=<n> id3=<n> id4=<n>
//!   S22-RV64-GRANT-PAIR-OWNER: PASS|FAIL

#![no_std]
#![no_main]
#![allow(unsafe_code)]

extern crate alloc;

use alloc::format;
use ostd::io::println;
use ostd::syscall::{
    sys_exit, sys_get_procs, sys_grant_alloc, sys_grant_free, sys_grant_register, sys_grant_share,
    sys_grant_slice, sys_grant_unregister, sys_recv, sys_send, sys_yield, SyscallResult,
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
    GrantRegister,
    GrantShare,
    GrantSlice,
    GrantFree,
    GrantUnregister
];

ostd::cell_main!(cell_main);

/// One page keeps the fixture inside the smallest supported grant quota.
const PAGE: usize = 4096;
/// `GrantPerm` encoding: 0 = ReadOnly, 1 = WriteOnly, 2 = ReadWrite.
const PERM_RO: u8 = 0;
const PERM_WO: u8 = 1;
const PERM_RW: u8 = 2;
/// Wire protocol: one byte, receiver → owner requests and owner → receiver acks.
const WANT_RW: u8 = 1;
const WANT_RO: u8 = 2;
const WANT_UNREGISTER: u8 = 3;
const WANT_EXIT: u8 = 4;
const REQ_FREE: u8 = 5;
const REQ_UNREGISTER: u8 = 6;
const REQ_EXIT: u8 = 7;
/// Same-recipient downgrade: publish ReadWrite, then re-share ReadOnly.
const WANT_DOWNGRADE_RW: u8 = 8;
const WANT_DOWNGRADE_RO: u8 = 9;
const ACK: u8 = 0x80;

/// One page of the owner's own WA is enough to prove the owner mapping works.
fn owner_page_writable(ptr: *mut u8) -> bool {
    // SAFETY: `ptr` is the kernel-resolved owner mapping for a grant this cell
    // owns, and the region is at least one page.
    unsafe {
        core::ptr::write_volatile(ptr, 0xA5);
        core::ptr::read_volatile(ptr) == 0xA5
    }
}

/// The receiver's task name, as the lane's reviewed launch path names it.
const RECEIVER_NAME: &[u8] = b"tier2-exploit";

fn name_eq(row: &[u8; 32], want: &[u8]) -> bool {
    let len = row.iter().position(|byte| *byte == 0).unwrap_or(row.len());
    &row[..len] == want
}

/// Is `tid` the live receiver this owner is serving?
///
/// The scheduler may hand this owner records that are not protocol requests (a
/// wake, or a peer-death notice for a task it did not spawn). Only a live task
/// on the receiver's launch path may move the owner forward.
fn is_receiver(tid: usize) -> bool {
    let mut rows = [api::syscall::ProcessInfo::default(); 64];
    match sys_get_procs(&mut rows) {
        Ok(count) => rows
            .iter()
            .take(count)
            .any(|row| row.id == tid && row.state != 3 && name_eq(&row.name, RECEIVER_NAME)),
        Err(_) => false,
    }
}

/// Blocking receive of one protocol byte from a live receiver. Returns
/// `(sender, byte)`.
fn recv_byte() -> (usize, u8) {
    loop {
        let mut buf = [0u8; 8];
        if let SyscallResult::Ok(sender) = sys_recv(0, &mut buf) {
            if buf[0] != 0 && is_receiver(sender) {
                return (sender, buf[0]);
            }
        }
        sys_yield();
    }
}

fn ack(target: usize, byte: u8) {
    let _ = sys_send(target, &[ACK, byte]);
}

fn cell_main() {
    println("S22-RV64-GRANT-PAIR-OWNER-BEGIN: public Grant* owner path");
    let mut ok = true;

    // 1. Owner allocation through both entry points.
    let alloc = sys_grant_alloc(PAGE);
    match alloc {
        Some(id) => println(&format!("S22-RV64-GRANT-PAIR-OWNER-ALLOC: OK id={id}")),
        None => {
            println("S22-RV64-GRANT-PAIR-OWNER-ALLOC: DENY");
            ok = false;
        }
    }
    let reg = sys_grant_register(PAGE);
    match reg {
        Some(id) => println(&format!("S22-RV64-GRANT-PAIR-OWNER-REGISTER: OK id={id}")),
        None => {
            println("S22-RV64-GRANT-PAIR-OWNER-REGISTER: DENY");
            ok = false;
        }
    }
    // A third page for the exit-revoke phase, since phase 1 frees the first and
    // phase 4 unregisters the second, and a fourth for the same-recipient
    // downgrade phase, which must be its own grant so each deliberate fault
    // stays attributable to one address.
    let exit_grant = sys_grant_alloc(PAGE);
    let down_grant = sys_grant_alloc(PAGE);

    // 2. The owner's own pointer must be writable, from the allocation itself.
    match alloc.map(sys_grant_slice) {
        Some(Some(ptr)) if owner_page_writable(ptr) => {
            println("S22-RV64-GRANT-PAIR-OWNER-MAPPED: OK")
        }
        _ => {
            println("S22-RV64-GRANT-PAIR-OWNER-MAPPED: MISMATCH");
            ok = false;
        }
    }
    match reg.map(sys_grant_slice) {
        Some(Some(ptr)) if owner_page_writable(ptr) => {
            println("S22-RV64-GRANT-PAIR-OWNER-REG-MAPPED: OK")
        }
        _ => {
            println("S22-RV64-GRANT-PAIR-OWNER-REG-MAPPED: MISMATCH");
            ok = false;
        }
    }

    // 3. Denials that stay denials. A non-private-root peer (tid 0, the kernel
    //    sentinel) cannot become a domain receiver.
    if let Some(grant_id) = alloc {
        if sys_grant_share(grant_id, 0, PERM_RO) {
            println("S22-RV64-GRANT-PAIR-OWNER-SHARE-FOREIGN: ALLOWED");
            ok = false;
        } else {
            println("S22-RV64-GRANT-PAIR-OWNER-SHARE-FOREIGN: DENY");
        }
    }

    println(&format!(
        "S22-RV64-GRANT-PAIR-HANDOFF id1={} id2={} id3={} id4={}",
        alloc.unwrap_or(0),
        reg.unwrap_or(0),
        exit_grant.unwrap_or(0),
        down_grant.unwrap_or(0)
    ));

    // 4. Serve one receiver generation per phase. Each generation asks for the
    //    rights it needs, then asks for the teardown its mode witnesses.
    let mut served = 0usize;
    let mut wo_reported = false;
    while served < 5 {
        let (sender, request) = recv_byte();
        match request {
            WANT_RW => {
                if let Some(grant_id) = alloc {
                    // Write-only has no ordinary-page representation on a domain
                    // pair: it must stay refused, and the refusal must not touch
                    // the tuple the ReadWrite share below publishes.
                    if sys_grant_share(grant_id, sender, PERM_WO) {
                        if !wo_reported {
                            println("S22-RV64-GRANT-PAIR-OWNER-SHARE-WO: ALLOWED");
                            wo_reported = true;
                            ok = false;
                        }
                    } else if !wo_reported {
                        println("S22-RV64-GRANT-PAIR-OWNER-SHARE-WO: DENY");
                        wo_reported = true;
                    }
                    let shared = sys_grant_share(grant_id, sender, PERM_RW);
                    ack(sender, u8::from(shared));
                }
            }
            WANT_RO => {
                if let Some(grant_id) = reg {
                    let shared = sys_grant_share(grant_id, sender, PERM_RO);
                    ack(sender, u8::from(shared));
                }
            }
            WANT_UNREGISTER => {
                if let Some(grant_id) = reg {
                    let shared = sys_grant_share(grant_id, sender, PERM_RO);
                    ack(sender, u8::from(shared));
                }
            }
            WANT_EXIT => {
                if let Some(grant_id) = exit_grant {
                    let shared = sys_grant_share(grant_id, sender, PERM_RW);
                    ack(sender, u8::from(shared));
                }
            }
            WANT_DOWNGRADE_RW => {
                if let Some(grant_id) = down_grant {
                    let shared = sys_grant_share(grant_id, sender, PERM_RW);
                    ack(sender, u8::from(shared));
                }
            }
            WANT_DOWNGRADE_RO => {
                if let Some(grant_id) = down_grant {
                    // Same grant, same recipient, stricter rights. The kernel
                    // must replace the receiver's writable PTE, not keep it:
                    // `shared` here only says the republish was accepted, so the
                    // runner's witness is the receiver's store fault below.
                    let shared = sys_grant_share(grant_id, sender, PERM_RO);
                    println(&format!(
                        "S22-RV64-GRANT-PAIR-OWNER-DOWNGRADE-RESHARE: {}",
                        if shared { "OK" } else { "FAIL" }
                    ));
                    ok &= shared;
                    ack(sender, u8::from(shared));
                }
            }
            REQ_FREE => {
                let freed = alloc.is_some_and(|grant_id| sys_grant_free(grant_id));
                println(&format!(
                    "S22-RV64-GRANT-PAIR-OWNER-FREE: {}",
                    if freed { "OK" } else { "FAIL" }
                ));
                ok &= freed;
                ack(sender, u8::from(freed));
                served += 1;
            }
            REQ_UNREGISTER => {
                let freed = reg.is_some_and(|grant_id| sys_grant_unregister(grant_id));
                println(&format!(
                    "S22-RV64-GRANT-PAIR-OWNER-UNREGISTER: {}",
                    if freed { "OK" } else { "FAIL" }
                ));
                ok &= freed;
                ack(sender, u8::from(freed));
                // Phases 1-3 are complete, so the terminal may be published
                // before phase 4's deliberate owner exit.
                if ok {
                    println("S22-RV64-GRANT-PAIR-OWNER: PASS");
                } else {
                    println("S22-RV64-GRANT-PAIR-OWNER: FAIL");
                }
                served += 1;
            }
            REQ_EXIT => {
                println("S22-RV64-GRANT-PAIR-OWNER-EXIT: OK");
                // Exit with the third grant still mapped in the receiver: the
                // retirement reaper must revoke it synchronously.
                sys_exit(if ok { 0 } else { 1 });
            }
            _ => {}
        }
    }
}
