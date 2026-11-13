//! `recovery-worker`: serves the demo counter but owns no business state. It
//! attaches to its authority with the one-shot nonce its parent staged, refuses
//! to serve unless the restored schema is its own, and commits every mutation at
//! the authority before replying (CP1 §4.2). Registration (Ready) is the
//! authority's decision, never the worker's.

#![no_std]
#![no_main]
#![deny(unsafe_code)]

extern crate alloc;

#[path = "../common.rs"]
mod common;
#[cfg(feature = "fault-inject")]
mod fault;
mod serve;

use alloc::format;
use ostd::io::println;
use recovery_demo::ledger::SCHEMA;
use recovery_demo::wire::Msg;
use recovery_demo::{AUTHORITY_SERVICE, EXIT_RECOVERY_FAILED};

abi::declare_manifest!(block_io = false, network = false, spawn = false);

abi::declare_syscalls![
    Log,
    Exit,
    Send,
    SendGather,
    Recv,
    RecvTimeout,
    SetTimer,
    LookupServiceBound,
    StateRestore,
    Spawn,
    GrantAlloc,
    GrantShare,
    GrantSlice,
    ShmAlloc,
    ShmMap,
    GetTime,
];

ostd::cell_main!(cell_main);

/// Attach round trip with the authority, admission included.
const ATTACH_TICKS: u64 = 500;

fn recovery_failed(why: &str) -> ! {
    println(&format!("[recovery-worker] RecoveryFailed: {why}"));
    ostd::syscall::sys_exit(EXIT_RECOVERY_FAILED)
}

fn staged_nonce() -> Option<u64> {
    let args = ostd::args();
    let value = args.iter().find_map(|arg| arg.strip_prefix("nonce="))?;
    u64::from_str_radix(value, 16).ok()
}

fn cell_main() {
    let Some(nonce) = staged_nonce() else {
        recovery_failed("no attach nonce")
    };
    let Some(authority) = common::wait_lookup(AUTHORITY_SERVICE, 500) else {
        recovery_failed("authority unavailable")
    };
    let attached = common::call(authority, Msg::Attach { nonce }, ATTACH_TICKS);
    let (epoch, revision, counter) = match attached {
        Some(Msg::Attached {
            epoch,
            revision,
            schema,
            counter,
        }) => {
            if schema != SCHEMA {
                // One-way: the authority fences and answers nothing.
                common::post(authority, Msg::RestoreFailed { epoch });
                recovery_failed(&format!("schema {schema} is not {SCHEMA}"));
            }
            (epoch, revision, counter)
        }
        Some(Msg::Refused(refusal)) => recovery_failed(&format!("attach refused {refusal:?}")),
        _ => recovery_failed("no attach reply"),
    };
    // One-way: publication is the authority's decision and it answers nothing.
    if !common::post(authority, Msg::Restored { epoch, revision }) {
        recovery_failed("authority unreachable");
    }
    println(&format!(
        "[recovery-worker] restored epoch={epoch} revision={revision} counter={counter}"
    ));
    serve::run(serve::Worker {
        authority,
        epoch,
        revision,
        counter,
    });
}
