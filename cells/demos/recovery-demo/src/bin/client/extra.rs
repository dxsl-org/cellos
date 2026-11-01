//! Separate client invocations: E4 foreign principal, E8 missing borrower
//! acknowledgement (needs the `recovery-fixture` VFS), E9 untracked export
//! (terminal for the boot: the provider then needs node recovery).

use alloc::format;
use ostd::io::println;
use recovery_demo::ledger::METHOD_INC;
use recovery_demo::wire::{Fault, Msg, Refusal};
use recovery_demo::{crash, state, WORKER_SERVICE};

use crate::common;
use crate::driver::{fail, free_frames, state_name, Client};

/// E4 (second principal): reusing another caller's request id is a separate
/// key; it neither replays nor conflicts with that caller's binding.
pub fn foreign(client: &mut Client, request_id: u64) {
    let (counter, _, _) = client.get();
    match client.mutate(request_id, METHOD_INC, 1, Fault::None) {
        Some(Msg::Done { replayed: false, result, .. }) if result == counter + 1 => {}
        other => fail(&format!("E4 foreign principal: {other:?}")),
    }
    println(&format!("[recovery-client] E4 PASS foreign principal reusing request id {request_id}: independent binding, applied once (counter {})", counter + 1));
}

/// E8: the worker dies while VFS still holds its grant lease. The pages stay
/// quarantined (charged, counted) until VFS completes, then return.
pub fn missing_ack(client: &mut Client) {
    let v = client.crash(crash::HOLD_LEASE, 0);
    if state_name(&v) != "Quarantined" || (v.vfs_leases_pending == 0 && v.quarantined_pages == 0) {
        fail(&format!("E8: retirement {} vfs={} qpages={}", state_name(&v), v.vfs_leases_pending, v.quarantined_pages));
    }
    let held = free_frames();
    println(&format!("[recovery-client] E8 quarantined: state=Quarantined vfs_leases={} qpages={} reasons={:#x} free_while_held={held}",
        v.vfs_leases_pending, v.quarantined_pages, v.reasons));
    for _ in 0..200 {
        common::sleep(10);
        let now = free_frames();
        if now >= held + v.quarantined_pages {
            println(&format!("[recovery-client] E8 PASS missing borrower ack quarantined {} page(s); released after VFS completed (free {held} -> {now})",
                v.quarantined_pages));
            return;
        }
    }
    fail("E8: quarantined pages never returned");
}

/// E9: an untracked SHM export makes containment unproven: no restart, no
/// Ready, and healing is refused (node recovery required).
pub fn untracked_export(client: &mut Client) {
    if client.worker_call(Msg::Crash { mode: crash::SHM_EXPORT, arg: 0 }) != Some(Msg::Ack) {
        fail("E9: crash not acknowledged");
    }
    let status = client.wait_status("node recovery required", |s| s.state == state::NODE_RECOVERY_REQUIRED && s.worker == 0);
    let Some(v) = status.verdict else { fail("E9: no verdict") };
    if state_name(&v) != "Unproven" || v.reasons & api::retirement::reason::SHM_UNTRACKED == 0 || v.shm_frames_released == 0 {
        fail(&format!("E9: retirement {} reasons={:#x}", state_name(&v), v.reasons));
    }
    common::sleep(100);
    if common::lookup(WORKER_SERVICE).is_some() {
        fail("E9: a replacement was published");
    }
    match client.authority_call(Msg::Heal) {
        Some(Msg::Refused(Refusal::Denied)) => {}
        other => fail(&format!("E9 heal: {other:?}")),
    }
    println(&format!("[recovery-client] E9 PASS untracked SHM export ({} frame(s)): Unproven, no restart, no Ready, heal refused (node recovery required)",
        v.shm_frames_released));
}
