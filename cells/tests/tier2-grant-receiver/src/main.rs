//! Tier-2 grant pair — receiver cell.
//!
//! Public-syscall receive half of the phase-03 step-5 pair. It is an ordinary
//! Tier-2 (`PROTECTION_CLASS_UNTRUSTED`) private-root domain cell, distinct from
//! the owner, so its `GrantSlice` reaches the production lifecycle with a real
//! domain caller.
//!
//! Command line (staged by the launcher):
//!   argv[0] = `rw` | `ro` | `downgrade` | `unregister` | `exit`
//!   argv[1..5] = the owner's GrantAlloc / GrantRegister / exit-phase GrantAlloc /
//!                same-recipient downgrade GrantAlloc ids — *optional*: the
//!                interactive lane types them after reading the owner's `HANDOFF`
//!                line, but they are kernel-assigned at run time, so a launcher
//!                that cannot spend a console round trip on them (the AArch64
//!                boot order) stages none and this cell asks the owner for them
//!                over the protocol channel instead (`WANT_IDS`, answered with
//!                four little-endian `u64`s).
//!   `peer=<task name>` = the owner's task name, i.e. the basename of the path its
//!                launcher installed it at. Default `/bin/tier2-smoke`'s name, the
//!                interactive lane's reviewed edge.
//!
//! Each mode ends in a store to the address the kernel handed back, which must
//! fault *after* the owner revoked the mapping — the deliberate fault is the
//! witness the lane classifies. The owner is discovered through the public
//! `GetProcs` surface rather than by a private handshake.
//!
//! `downgrade` is the one mode that keeps its mapping across the owner's action:
//! it proves a ReadWrite mapping first (a store reads back), then the owner
//! re-shares the same grant ReadOnly to this same recipient, and the mode proves
//! both halves of the downgrade — the byte written before the re-share is still
//! readable through the address afterwards (so the page was not merely unmapped)
//! and the store to it now faults (so the old writable PTE is gone).
//!
//! Marker grammar (`<arch>` is `RV64` or `AARCH64`, matching the architecture the
//! cell was built for):
//!   S22-<arch>-GRANT-PAIR-RECEIVER-BEGIN: mode=<m> ids=<a>,<b>,<c>,<d>
//!   S22-<arch>-GRANT-PAIR-RECEIVER-ALLOC: OK id=<n>|DENY
//!   S22-<arch>-GRANT-PAIR-RECEIVER-HANDOFF: REFUSED
//!   S22-<arch>-GRANT-PAIR-RECEIVER-OWNER: MISSING
//!   S22-<arch>-GRANT-PAIR-RECEIVER-MODE: UNKNOWN
//!   S22-<arch>-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: DENY|MAPPED
//!   S22-<arch>-GRANT-PAIR-RECEIVER-SLICE-RO: OK|FAIL
//!   S22-<arch>-GRANT-PAIR-RECEIVER-SLICE-RW: OK|FAIL
//!   S22-<arch>-GRANT-PAIR-RECEIVER-RW: OK|MISMATCH
//!   S22-<arch>-GRANT-PAIR-RECEIVER-DOWNGRADE-RW: OK|FAIL
//!   S22-<arch>-GRANT-PAIR-RECEIVER-DOWNGRADE-READ: OK (read 0xa5)|MISMATCH|MISSING
//!   S22-<arch>-GRANT-PAIR-RECEIVER-DOWNGRADE-WRITE: FAULT-EXPECTED
//!   S22-<arch>-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED|REUSED
//!   S22-<arch>-GRANT-PAIR-RECEIVER-RO-WRITE: FAULT-EXPECTED
//!   S22-<arch>-GRANT-PAIR-RECEIVER-REVOKE-FAULT: FAULT-EXPECTED
//!   S22-<arch>-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: FAULT-EXPECTED
//!   S22-<arch>-GRANT-PAIR-RECEIVER-EXIT-FAULT: FAULT-EXPECTED

#![no_std]
#![no_main]
#![allow(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::ToString;
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
/// Marker prefix. See the owner cell: a lane refuses another architecture's tag,
/// so the pair tags its markers with the architecture it was built for.
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
/// Wire protocol: one byte, shared with the owner.
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
/// In-band id handoff: ask the owner for the four grant ids. Sent only when this
/// generation was launched without them (see the command-line note above).
const WANT_IDS: u8 = 10;
/// `WANT_IDS` answer length: `ACK` then four little-endian `u64` ids.
const IDS_REPLY_LEN: usize = 1 + 4 * core::mem::size_of::<u64>();
const ACK: u8 = 0x80;
/// The byte this cell writes through the ReadWrite mapping and must still read
/// back after the owner re-shares the same grant ReadOnly.
const DOWNGRADE_BYTE: u8 = 0xA5;
/// An address no grant record can occupy: the allocator only hands out frames
/// from RAM, far above page 1.
const IMPOSSIBLE_ID: usize = 0x1000;
/// Bounded retries for a poll that must observe a kernel-side state change.
const POLL_LIMIT: usize = 4_000_000;

/// Store attempts allowed while waiting for an *asynchronous* revocation to reach
/// this CPU: the owner's exit (whose reaper revokes in its own time) and its
/// `GrantUnregister` (which reports itself drained once every hart has published
/// the invalidation, while a hart that has not — this one — can still run).
/// Yields are cheap and the reaper is not, so this is far above the observed
/// latency; it is still a bound, because the lane's boot window is 35 s and a cell
/// that spins it out hides the very stall the retry is meant to expose.
const EXIT_RETRY_LIMIT: usize = 24_000_000;

fn name_eq(row: &[u8; 32], want: &[u8]) -> bool {
    let len = row.iter().position(|byte| *byte == 0).unwrap_or(row.len());
    &row[..len] == want
}

/// The owner's task name on the interactive lane: that lane's reviewed shell edge
/// installs the owner ELF at `/bin/tier2-smoke`, and the task is named after the
/// path it was launched from.
const INTERACTIVE_OWNER_NAME: &str = "tier2-smoke";

/// The owner is the live task the launcher named, found through the public
/// `GetProcs` surface — the boot-order lane has no shell to type a tid at.
fn find_owner(owner_name: &[u8]) -> Option<usize> {
    for _ in 0..POLL_LIMIT {
        let mut rows = [api::syscall::ProcessInfo::default(); 64];
        if let Ok(count) = sys_get_procs(&mut rows) {
            for row in rows.iter().take(count) {
                if row.state != 3 && name_eq(&row.name, owner_name) {
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

/// Ask the owner for the four grant ids over the protocol channel.
///
/// This is the in-band handoff: the ids are kernel-assigned after the owner
/// started, so no launcher can put them on this cell's command line. The
/// interactive lane avoids the question by reading the owner's `HANDOFF` line and
/// typing the ids at the shell; the AArch64 boot order has no console round trip
/// to spend, so it gets them here instead — same `Send`/`Recv` ABI, no new call.
fn request_ids(owner: usize) -> Option<[usize; 4]> {
    send_byte(owner, WANT_IDS);
    for _ in 0..POLL_LIMIT {
        let mut buf = [0u8; IDS_REPLY_LEN + 8];
        if let SyscallResult::Ok(sender) = sys_recv(owner, &mut buf) {
            if sender != owner || buf[0] != ACK {
                continue;
            }
            let mut ids = [0usize; 4];
            for (index, id) in ids.iter_mut().enumerate() {
                let offset = 1 + index * core::mem::size_of::<u64>();
                let bytes: [u8; 8] = buf[offset..offset + 8].try_into().ok()?;
                *id = u64::from_le_bytes(bytes) as usize;
            }
            if ids.iter().all(|id| *id != 0) {
                return Some(ids);
            }
            return None;
        }
        sys_yield();
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
    // The interactive lane types the four ids after the mode; a boot-order launch
    // cannot (they do not exist yet), so it stages `peer=<owner task name>` alone
    // and this generation fetches the ids from the owner below.
    let mut ids = [parsed(1), parsed(2), parsed(3), parsed(4)];
    let owner_name = args
        .iter()
        .find_map(|arg| arg.strip_prefix("peer="))
        .unwrap_or(INTERACTIVE_OWNER_NAME)
        .to_string();

    let Some(owner) = find_owner(owner_name.as_bytes()) else {
        println(&format!("{TAG}-GRANT-PAIR-RECEIVER-OWNER: MISSING"));
        sys_exit(1);
    };
    if ids.iter().all(|id| *id == 0) {
        match request_ids(owner) {
            Some(fetched) => ids = fetched,
            None => {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-HANDOFF: REFUSED"));
                sys_exit(1);
            }
        }
    }
    let [id_alloc, id_reg, id_exit, id_down] = ids;

    println(&format!(
        "{TAG}-GRANT-PAIR-RECEIVER-BEGIN: mode={mode} ids={id_alloc},{id_reg},{id_exit},{id_down}"
    ));

    // A second private root allocates independently: the capability rule is
    // per-task, not a global "one domain grant" policy.
    match sys_grant_alloc(PAGE) {
        Some(id) => println(&format!("{TAG}-GRANT-PAIR-RECEIVER-ALLOC: OK id={id}")),
        None => println(&format!("{TAG}-GRANT-PAIR-RECEIVER-ALLOC: DENY")),
    }

    let want = match mode.as_str() {
        "rw" => Some(WANT_RW),
        "ro" => Some(WANT_RO),
        "downgrade" => Some(WANT_DOWNGRADE_RW),
        "unregister" => Some(WANT_UNREGISTER),
        "exit" => Some(WANT_EXIT),
        _ => None,
    };
    let Some(want) = want else {
        println(&format!("{TAG}-GRANT-PAIR-RECEIVER-MODE: UNKNOWN"));
        sys_exit(1);
    };
    send_byte(owner, want);
    let Some(reply) = recv_byte_from(owner) else {
        println(&format!("{TAG}-GRANT-PAIR-RECEIVER-SHARE: NO-ACK"));
        sys_exit(1);
    };
    if reply & ACK == 0 {
        println(&format!("{TAG}-GRANT-PAIR-RECEIVER-SHARE: REFUSED"));
        sys_exit(1);
    }

    match mode.as_str() {
        "rw" => {
            // An unknown id keeps the `usize::MAX` "not authorized" sentinel.
            match sys_grant_slice(IMPOSSIBLE_ID) {
                None => println(&format!("{TAG}-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: DENY")),
                Some(_) => {
                    println(&format!("{TAG}-GRANT-PAIR-RECEIVER-SLICE-UNKNOWN: MAPPED"));
                    sys_exit(1);
                }
            }
            let Some(pointer) = sys_grant_slice(id_alloc) else {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-SLICE-RW: FAIL"));
                sys_exit(1);
            };
            let wrote = unsafe {
                core::ptr::write_volatile(pointer, 0x3C);
                core::ptr::read_volatile(pointer) == 0x3C
            };
            println(&format!(
                "{TAG}-GRANT-PAIR-RECEIVER-RW: {}",
                if wrote { "OK" } else { "MISMATCH" }
            ));
            send_byte(owner, REQ_FREE);
            if recv_byte_from(owner).is_none() {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-FREE: NO-ACK"));
                sys_exit(1);
            }
            // The revoked record must not resolve again — reused frames stay
            // private — and the mapping already handed out must fault.
            match sys_grant_slice(id_alloc) {
                None => println(&format!("{TAG}-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED")),
                Some(_) => {
                    println(&format!("{TAG}-GRANT-PAIR-RECEIVER-FRAME-REUSE: REUSED"));
                    sys_exit(1);
                }
            }
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-REVOKE-FAULT: FAULT-EXPECTED"));
            deliberate_store(pointer);
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-REVOKE-FAULT: WROTE"));
        }
        "ro" => {
            let Some(pointer) = sys_grant_slice(id_reg) else {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-SLICE-RO: FAIL"));
                sys_exit(1);
            };
            // A read-only page must still be readable, or the refusal would be
            // indistinguishable from a missing mapping.
            let observed = unsafe { core::ptr::read_volatile(pointer) };
            println(&format!(
                "{TAG}-GRANT-PAIR-RECEIVER-SLICE-RO: OK (read {observed:#x})"
            ));
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-RO-WRITE: FAULT-EXPECTED"));
            deliberate_store(pointer);
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-RO-WRITE: WROTE"));
        }
        "downgrade" => {
            // ReadWrite first: this proves the mapping is writable and fixes the
            // byte that must survive the downgrade.
            let Some(rw_pointer) = sys_grant_slice(id_down) else {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-DOWNGRADE-RW: FAIL"));
                sys_exit(1);
            };
            let rw_address = rw_pointer as usize;
            let wrote = unsafe {
                core::ptr::write_volatile(rw_pointer, DOWNGRADE_BYTE);
                core::ptr::read_volatile(rw_pointer) == DOWNGRADE_BYTE
            };
            println(&format!(
                "{TAG}-GRANT-PAIR-RECEIVER-DOWNGRADE-RW: {}",
                if wrote { "OK" } else { "FAIL" }
            ));
            if !wrote {
                sys_exit(1);
            }
            // Same grant, same recipient, stricter rights.
            send_byte(owner, WANT_DOWNGRADE_RO);
            if recv_byte_from(owner).is_none() {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-DOWNGRADE-RESHARE: NO-ACK"));
                sys_exit(1);
            }
            // The address must still resolve, still hold the byte written above
            // (a dropped mapping would fault on the load instead), and refuse the
            // store (the old writable PTE is what the re-share had to remove).
            let Some(ro_pointer) = sys_grant_slice(id_down) else {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-DOWNGRADE-READ: MISSING"));
                sys_exit(1);
            };
            if ro_pointer as usize != rw_address {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-DOWNGRADE-READ: MOVED"));
                sys_exit(1);
            }
            let observed = unsafe { core::ptr::read_volatile(ro_pointer) };
            if observed != DOWNGRADE_BYTE {
                println(&format!(
                    "{TAG}-GRANT-PAIR-RECEIVER-DOWNGRADE-READ: MISMATCH {observed:#x}"
                ));
                sys_exit(1);
            }
            println(&format!(
                "{TAG}-GRANT-PAIR-RECEIVER-DOWNGRADE-READ: OK (read {observed:#x})"
            ));
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-DOWNGRADE-WRITE: FAULT-EXPECTED"));
            deliberate_store(ro_pointer);
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-DOWNGRADE-WRITE: WROTE"));
        }
        "unregister" => {
            let Some(pointer) = sys_grant_slice(id_reg) else {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-SLICE-RO: FAIL"));
                sys_exit(1);
            };
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-SLICE-RO: OK"));
            send_byte(owner, REQ_UNREGISTER);
            if recv_byte_from(owner).is_none() {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-UNREGISTER: NO-ACK"));
                sys_exit(1);
            }
            if wait_until_revoked(id_reg) {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-FRAME-REUSE: REFUSED"));
            } else {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-FRAME-REUSE: REUSED"));
            }
            // Same shape as the exit phase: the owner's `GrantUnregister` reports
            // itself drained once every hart has acknowledged the invalidation,
            // but a hart that has not published its epoch yet can still be running
            // this cell — so the first store may land. Store until the revocation
            // traps (bounded; the trap is the witness).
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: FAULT-EXPECTED"));
            for _ in 0..EXIT_RETRY_LIMIT {
                deliberate_store(pointer);
                sys_yield();
            }
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-UNREGISTER-FAULT: WROTE"));
        }
        "exit" => {
            let Some(pointer) = sys_grant_slice(id_exit) else {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-SLICE-RW: FAIL"));
                sys_exit(1);
            };
            let wrote = unsafe {
                core::ptr::write_volatile(pointer, 0x77);
                core::ptr::read_volatile(pointer) == 0x77
            };
            println(&format!(
                "{TAG}-GRANT-PAIR-RECEIVER-SLICE-RW: {}",
                if wrote { "OK" } else { "FAIL" }
            ));
            // The owner exits on this request; its retirement reaper must revoke
            // the receiver mapping before this cell can write the old address.
            send_byte(owner, REQ_EXIT);
            if !wait_until_revoked(id_exit) {
                println(&format!("{TAG}-GRANT-PAIR-RECEIVER-EXIT-FAULT: STILL-MAPPED"));
                sys_exit(1);
            }
            // A reaper revokes asynchronously: on more than one CPU the record
            // above can be gone while this hart's translation is still live, so a
            // store that *lands* means "not revoked yet on this CPU", not
            // "revocation is broken". Store until the revocation traps — bounded,
            // and the trap is what the lane reads as the witness.
            //
            // The bound is generous because the reaper is asynchronous by design:
            // a peer hart can be non-preemptible for seconds, and the measured
            // confirmation latency on a two-hart boot reached ~235 reaper ticks
            // (~2.3 s) before the invalidation landed.
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-EXIT-FAULT: FAULT-EXPECTED"));
            for _ in 0..EXIT_RETRY_LIMIT {
                deliberate_store(pointer);
                sys_yield();
            }
            println(&format!("{TAG}-GRANT-PAIR-RECEIVER-EXIT-FAULT: WROTE"));
        }
        _ => {}
    }

    println(&format!("{TAG}-GRANT-PAIR-RECEIVER: FAIL"));
    sys_exit(1);
}
