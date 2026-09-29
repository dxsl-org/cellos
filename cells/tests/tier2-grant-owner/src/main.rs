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
//! Command line (all optional):
//!   `peer=<task name>` — the receiver's task name, i.e. the basename of the path
//!   the launcher installed it at. The interactive lane installs it at the
//!   reviewed `/bin/tier2-exploit`, which is also the default; the AArch64
//!   boot-order lane has no shell and installs the pair on two other reviewed
//!   edges, so its launcher names the receiver explicitly.
//!
//! Grant ids never travel on a command line: they are kernel-assigned after this
//! cell starts. A receiver that has none asks for them with a `WANT_IDS` request
//! and gets them back as four little-endian `u64`s on the same IPC channel — the
//! handoff the interactive lane gets by reading this cell's `HANDOFF` line.
//!
//! Marker grammar (the QEMU lanes grep these verbatim; `<arch>` is `RV64` or
//! `AARCH64`, matching the architecture the cell was built for):
//!   S22-<arch>-GRANT-PAIR-OWNER-ALLOC: OK id=<n>|DENY
//!   S22-<arch>-GRANT-PAIR-OWNER-REGISTER: OK id=<n>|DENY
//!   S22-<arch>-GRANT-PAIR-OWNER-MAPPED: OK|MISMATCH*
//!   S22-<arch>-GRANT-PAIR-OWNER-REG-MAPPED: OK|MISMATCH*
//!   S22-<arch>-GRANT-PAIR-OWNER-SHARE-FOREIGN: DENY|ALLOWED
//!   S22-<arch>-GRANT-PAIR-OWNER-SHARE-WO: DENY|ALLOWED
//!   S22-<arch>-GRANT-PAIR-OWNER-FREE: OK|FAIL
//!   S22-<arch>-GRANT-PAIR-OWNER-UNREGISTER: OK|FAIL
//!   S22-<arch>-GRANT-PAIR-OWNER-DOWNGRADE-RESHARE: OK|FAIL
//!   S22-<arch>-GRANT-PAIR-OWNER-EXIT: OK
//!   S22-<arch>-GRANT-PAIR-HANDOFF id1=<n> id2=<n> id3=<n> id4=<n>
//!   S22-<arch>-GRANT-PAIR-OWNER: PASS|FAIL

#![no_std]
#![no_main]
#![allow(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
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

/// Marker prefix. An architecture that emits another architecture's tag is
/// claiming evidence it did not produce — the AArch64 lane refuses an
/// `S22-RV64-*` grant marker outright — so the pair tags its own markers with the
/// architecture it was built for, exactly as the kernel's `ADMISSION_TAG` does.
#[cfg(target_arch = "riscv64")]
const TAG: &str = "S22-RV64";
#[cfg(target_arch = "aarch64")]
const TAG: &str = "S22-AARCH64";
#[cfg(target_arch = "x86_64")]
const TAG: &str = "S22-X86";
#[cfg(not(any(
    target_arch = "riscv64",
    target_arch = "aarch64",
    target_arch = "x86_64"
)))]
const TAG: &str = "S22-UNKNOWN";

/// One page keeps the fixture inside the smallest supported grant quota.
const PAGE: usize = 4096;
/// Yields allowed while a `GrantUnregister` reports itself not-yet-drained. The
/// drain waits on a remote acknowledgement and the reaper completes it on the
/// next ticks, so this is generous rather than tight.
const UNREGISTER_RETRY_LIMIT: usize = 100_000;
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
/// In-band id handoff (owner → receiver answer only): the receiver asks for the
/// four grant ids over this same channel when its launcher could not put them on
/// its command line. The ids are kernel-assigned at run time, so a launcher that
/// has no console round trip to spend — the AArch64 boot order — cannot know them.
const WANT_IDS: u8 = 10;
/// `WANT_IDS` answer: `ACK` then the four ids as little-endian `u64`s.
const IDS_REPLY_LEN: usize = 1 + 4 * core::mem::size_of::<u64>();
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

/// The receiver's task name on the interactive lane: that lane's reviewed shell
/// edge installs the receiver ELF at `/bin/tier2-exploit`, and the task is named
/// after the path it was launched from.
const INTERACTIVE_RECEIVER_NAME: &str = "tier2-exploit";

/// This cell's `peer=<task name>` argument, when the launcher staged one.
///
/// The pair is launched from two different reviewed edges: the interactive lane
/// types the receiver's command line at the shell, and the boot-order lane names
/// the receiver by the path the kernel's init profile allows it to launch. The
/// task name is therefore a property of the lane, not of the cell, and the owner
/// learns it from the one argv channel the ABI already has.
fn peer_arg() -> Option<String> {
    ostd::args()
        .into_iter()
        .find_map(|arg| arg.strip_prefix("peer=").map(String::from))
}

fn name_eq(row: &[u8; 32], want: &[u8]) -> bool {
    let len = row.iter().position(|byte| *byte == 0).unwrap_or(row.len());
    &row[..len] == want
}

/// Is `tid` the live receiver this owner is serving?
///
/// The scheduler may hand this owner records that are not protocol requests (a
/// wake, or a peer-death notice for a task it did not spawn). Only a live task
/// on the receiver's launch path may move the owner forward.
fn is_receiver(tid: usize, receiver_name: &[u8]) -> bool {
    let mut rows = [api::syscall::ProcessInfo::default(); 64];
    match sys_get_procs(&mut rows) {
        Ok(count) => rows
            .iter()
            .take(count)
            .any(|row| row.id == tid && row.state != 3 && name_eq(&row.name, receiver_name)),
        Err(_) => false,
    }
}

/// Blocking receive of one protocol byte from a live receiver. Returns
/// `(sender, byte)`.
fn recv_byte(receiver_name: &[u8]) -> (usize, u8) {
    loop {
        let mut buf = [0u8; 8];
        if let SyscallResult::Ok(sender) = sys_recv(0, &mut buf) {
            if buf[0] != 0 && is_receiver(sender, receiver_name) {
                return (sender, buf[0]);
            }
        }
        sys_yield();
    }
}

/// Answer a receiver's in-band id handoff over the protocol channel.
fn send_ids(target: usize, ids: [Option<usize>; 4]) {
    let mut reply = [0u8; IDS_REPLY_LEN];
    reply[0] = ACK;
    for (index, id) in ids.iter().enumerate() {
        let offset = 1 + index * core::mem::size_of::<u64>();
        let bytes = (id.unwrap_or(0) as u64).to_le_bytes();
        reply[offset..offset + bytes.len()].copy_from_slice(&bytes);
    }
    let _ = sys_send(target, &reply);
}

fn ack(target: usize, byte: u8) {
    let _ = sys_send(target, &[ACK, byte]);
}

fn cell_main() {
    println(&format!("{TAG}-GRANT-PAIR-OWNER-BEGIN: public Grant* owner path"));
    let mut ok = true;
    let receiver_name = peer_arg().unwrap_or_else(|| String::from(INTERACTIVE_RECEIVER_NAME));
    let receiver_name = receiver_name.as_bytes();

    // 1. Owner allocation through both entry points.
    let alloc = sys_grant_alloc(PAGE);
    match alloc {
        Some(id) => println(&format!("{TAG}-GRANT-PAIR-OWNER-ALLOC: OK id={id}")),
        None => {
            println(&format!("{TAG}-GRANT-PAIR-OWNER-ALLOC: DENY"));
            ok = false;
        }
    }
    let reg = sys_grant_register(PAGE);
    match reg {
        Some(id) => println(&format!("{TAG}-GRANT-PAIR-OWNER-REGISTER: OK id={id}")),
        None => {
            println(&format!("{TAG}-GRANT-PAIR-OWNER-REGISTER: DENY"));
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
            println(&format!("{TAG}-GRANT-PAIR-OWNER-MAPPED: OK"))
        }
        _ => {
            println(&format!("{TAG}-GRANT-PAIR-OWNER-MAPPED: MISMATCH"));
            ok = false;
        }
    }
    match reg.map(sys_grant_slice) {
        Some(Some(ptr)) if owner_page_writable(ptr) => {
            println(&format!("{TAG}-GRANT-PAIR-OWNER-REG-MAPPED: OK"))
        }
        _ => {
            println(&format!("{TAG}-GRANT-PAIR-OWNER-REG-MAPPED: MISMATCH"));
            ok = false;
        }
    }

    // 3. Denials that stay denials. A non-private-root peer (tid 0, the kernel
    //    sentinel) cannot become a domain receiver.
    if let Some(grant_id) = alloc {
        if sys_grant_share(grant_id, 0, PERM_RO) {
            println(&format!("{TAG}-GRANT-PAIR-OWNER-SHARE-FOREIGN: ALLOWED"));
            ok = false;
        } else {
            println(&format!("{TAG}-GRANT-PAIR-OWNER-SHARE-FOREIGN: DENY"));
        }
    }

    println(&format!(
        "{TAG}-GRANT-PAIR-HANDOFF id1={} id2={} id3={} id4={}",
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
        let (sender, request) = recv_byte(receiver_name);
        match request {
            WANT_IDS => {
                // The receiver has no command line carrying the ids (a boot-order
                // launch: they are kernel-assigned after this cell started), so it
                // asks for them here. Same IPC channel, no new ABI.
                send_ids(sender, [alloc, reg, exit_grant, down_grant]);
            }
            WANT_RW => {
                if let Some(grant_id) = alloc {
                    // Write-only has no ordinary-page representation on a domain
                    // pair: it must stay refused, and the refusal must not touch
                    // the tuple the ReadWrite share below publishes.
                    if sys_grant_share(grant_id, sender, PERM_WO) {
                        if !wo_reported {
                            println(&format!("{TAG}-GRANT-PAIR-OWNER-SHARE-WO: ALLOWED"));
                            wo_reported = true;
                            ok = false;
                        }
                    } else if !wo_reported {
                        println(&format!("{TAG}-GRANT-PAIR-OWNER-SHARE-WO: DENY"));
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
                        "{TAG}-GRANT-PAIR-OWNER-DOWNGRADE-RESHARE: {}",
                        if shared { "OK" } else { "FAIL" }
                    ));
                    ok &= shared;
                    ack(sender, u8::from(shared));
                }
            }
            REQ_FREE => {
                let freed = alloc.is_some_and(|grant_id| sys_grant_free(grant_id));
                println(&format!(
                    "{TAG}-GRANT-PAIR-OWNER-FREE: {}",
                    if freed { "OK" } else { "FAIL" }
                ));
                ok &= freed;
                ack(sender, u8::from(freed));
                served += 1;
            }
            REQ_UNREGISTER => {
                // `GrantUnregister` removes the mapping and then waits for every
                // hart to acknowledge the invalidation. On more than one CPU the
                // peer may not have answered yet, and the call reports that as a
                // refusal while keeping the record `Revoking` for an idempotent
                // retry — so a refusal here is "not yet", not "denied". Retry,
                // bounded: a refusal that never clears still prints FAIL.
                let mut freed = false;
                for _ in 0..UNREGISTER_RETRY_LIMIT {
                    freed = reg.is_some_and(|grant_id| sys_grant_unregister(grant_id));
                    if freed {
                        break;
                    }
                    sys_yield();
                }
                println(&format!(
                    "{TAG}-GRANT-PAIR-OWNER-UNREGISTER: {}",
                    if freed { "OK" } else { "FAIL" }
                ));
                ok &= freed;
                ack(sender, u8::from(freed));
                // Phases 1-3 are complete, so the terminal may be published
                // before phase 4's deliberate owner exit.
                if ok {
                    println(&format!("{TAG}-GRANT-PAIR-OWNER: PASS"));
                } else {
                    println(&format!("{TAG}-GRANT-PAIR-OWNER: FAIL"));
                }
                served += 1;
            }
            REQ_EXIT => {
                println(&format!("{TAG}-GRANT-PAIR-OWNER-EXIT: OK"));
                // Exit with the third grant still mapped in the receiver: the
                // retirement reaper must revoke it synchronously.
                sys_exit(if ok { 0 } else { 1 });
            }
            _ => {}
        }
    }
}
