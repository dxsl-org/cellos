//! `recovery-authority`: the demo provider's state authority and its worker's
//! lifecycle parent (CP1 option B). It commits request bindings atomically with
//! the state, fences writer epochs, restarts a crashed worker only after the
//! kernel reports the old incarnation's retirement contained, and publishes the
//! replacement (Ready) only after it restored and validated the committed state.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

#[path = "../common.rs"]
mod common;
#[cfg(feature = "fault-inject")]
mod inject;
mod lifecycle;

use alloc::format;
use ostd::io::println;
use recovery_demo::ledger::Sender;
use recovery_demo::state;
use recovery_demo::wire::{Msg, Refusal};
use recovery_demo::WORKER_SERVICE;

use lifecycle::Provider;

api::declare_manifest!(block_io = false, network = false, spawn = true);

api::declare_syscalls![
    Log,
    Exit,
    Send,
    Recv,
    RecvTimeout,
    SetTimer,
    LookupServiceBound,
    StateStash,
    SpawnFromPath,
    GetRandom,
    GrantAlloc,
    GrantShare,
    GrantSlice,
    GrantFree
];

ostd::cell_main!(cell_main);

fn cell_main() {
    let mut provider = Provider::new();
    println("[recovery-authority] up: committed state revision=0 (RAM-only, survives worker crash only)");
    provider.spawn_worker();
    loop {
        let Some(rx) = common::recv_attested(0) else {
            continue;
        };
        // A death notification carries the exit reason, not a frame. It is
        // trusted only for the current worker and only when the kernel attests
        // no live sender or already holds a retirement record for that root.
        if rx.msg.is_none() || rx.identity.is_none() {
            let dead =
                rx.identity.is_none() || ostd::syscall::sys_query_retirement(rx.sender).is_ok();
            if dead {
                let reason = u64::from_le_bytes(rx.bytes[..8].try_into().unwrap_or([0; 8]));
                provider.on_death(rx.sender, reason);
            }
            continue;
        }
        let Some((cell, generation)) = rx.principal() else {
            continue;
        };
        let from = Sender {
            tid: rx.sender,
            cell,
            generation,
        };
        let Some(msg) = rx.msg else { continue };
        dispatch(&mut provider, from, msg);
    }
}

fn dispatch(provider: &mut Provider, from: Sender, msg: Msg) {
    let reply = match msg {
        Msg::Attach { nonce } => match provider.auth.attach(nonce, from) {
            Ok((epoch, committed)) => {
                println(&format!(
                    "[recovery-authority] attached epoch={epoch} worker cell={} gen={} revision={} counter={}",
                    from.cell, from.generation, committed.revision, committed.counter
                ));
                Msg::Attached {
                    epoch,
                    revision: committed.revision,
                    schema: committed.schema,
                    counter: committed.counter,
                }
            }
            Err(refusal) => {
                if refusal == Refusal::RecoveryFailed {
                    provider.recovery_failed("stored state refused at attach");
                }
                Msg::Refused(refusal)
            }
        },
        Msg::Restored { epoch, revision } if provider.auth.is_writer(epoch, from) => {
            let published = matches!(
                ostd::syscall::sys_register_service(WORKER_SERVICE, from.tid, 0),
                ostd::syscall::SyscallResult::Ok(_)
            );
            provider.state = if published {
                state::SERVING
            } else {
                state::RECOVERING
            };
            println(&format!(
                "[recovery-authority] READY worker={} epoch={epoch} revision={revision} published={published}",
                from.tid
            ));
            return;
        }
        Msg::RestoreFailed { epoch } if provider.auth.is_writer(epoch, from) => {
            provider.auth.fence();
            provider.recovery_failed("replacement refused the restored schema");
            return;
        }
        Msg::Commit {
            epoch,
            expected_revision,
            binding,
            new_state,
        } => provider
            .auth
            .commit(epoch, from, expected_revision, &binding, new_state),
        Msg::Lookup { epoch, binding } => provider.auth.lookup(epoch, from, &binding),
        Msg::Report => {
            provider.report(from.tid);
            return;
        }
        #[cfg(feature = "fault-inject")]
        Msg::Fence | Msg::Corrupt { .. } | Msg::Heal | Msg::Lend => inject::handle(provider, msg),
        _ => return,
    };
    common::send(from.tid, reply);
}
