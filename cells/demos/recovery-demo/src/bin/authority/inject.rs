//! Fault-inject builds only: evidence-client operations that fence the writer,
//! corrupt the stored state, heal it, or lend the worker a writable page.

use alloc::format;
use ostd::io::println;
use ostd::syscall;
use recovery_demo::ledger::{Committed, SCHEMA};
use recovery_demo::wire::{Msg, Refusal};
use recovery_demo::{corrupt, state};

use crate::lifecycle::Provider;

pub fn handle(provider: &mut Provider, msg: Msg) -> Msg {
    match msg {
        // Planned fence of a live writer: it learns `StaleEpoch` at its next
        // commit and exits; its death then restarts a replacement.
        Msg::Fence => {
            let watermark = provider.auth.fence();
            println(&format!("[recovery-authority] fence: writer ended at revision {watermark}"));
            Msg::Ack
        }
        Msg::Corrupt { kind } => corrupt_state(provider, kind),
        Msg::Heal => {
            if provider.state == state::NODE_RECOVERY_REQUIRED {
                return Msg::Refused(Refusal::Denied);
            }
            let Some(pristine) = provider.pristine.take() else { return Msg::Refused(Refusal::Denied) };
            provider.auth.replace_committed(Some(pristine));
            provider.state = state::RECOVERING;
            println(&format!("[recovery-authority] healed: committed revision {} restored", pristine.revision));
            if provider.worker.is_none() {
                provider.spawn_worker();
            }
            Msg::Ack
        }
        Msg::Lend => {
            let Some(worker) = provider.worker else { return Msg::Refused(Refusal::Denied) };
            let Some(grant) = syscall::sys_grant_alloc(4096) else { return Msg::Refused(Refusal::QuotaExceeded) };
            let _ = syscall::sys_grant_copy_from_slice(grant, &[0u8; 8]);
            if !syscall::sys_grant_share(grant, worker, 2) {
                syscall::sys_grant_free(grant);
                return Msg::Refused(Refusal::Denied);
            }
            provider.lent = Some(grant);
            Msg::Lent { grant: grant as u64 }
        }
        _ => Msg::Refused(Refusal::Denied),
    }
}

/// Fence first (the watermark is the pre-corruption revision), then replace the
/// stored state and kill the worker so its replacement must restore from it.
fn corrupt_state(provider: &mut Provider, kind: u8) -> Msg {
    let Some(current) = provider.auth.committed().filter(Committed::intact) else {
        return Msg::Refused(Refusal::Denied);
    };
    provider.auth.fence();
    let corrupted = match kind {
        corrupt::CHECKSUM => Some(Committed { counter: current.counter ^ 0xff, ..current }),
        corrupt::MISSING => None,
        corrupt::SCHEMA => Some(Committed::new(SCHEMA + 1, current.revision, current.counter)),
        corrupt::REVISION => Some(Committed::new(current.schema, current.revision + 1, current.counter)),
        _ => return Msg::Refused(Refusal::Denied),
    };
    provider.pristine = Some(current);
    provider.auth.replace_committed(corrupted);
    println(&format!("[recovery-authority] injected corruption kind={kind}"));
    if let Some(worker) = provider.worker {
        let _ = syscall::sys_force_exit(worker);
    }
    Msg::Ack
}
