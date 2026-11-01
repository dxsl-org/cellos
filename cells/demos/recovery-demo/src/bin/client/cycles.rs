//! E6 restore refusal (RecoveryFailed, no Ready), E11 identity across
//! replacements, and E12 ten crash/restore cycles with resource reconciliation.

use alloc::format;
use alloc::vec::Vec;
use ostd::io::println;
use recovery_demo::wire::Msg;
use recovery_demo::{corrupt, crash, state, WORKER_SERVICE};

use crate::common;
use crate::driver::{fail, free_frames, state_name, Client};

/// E6: corrupt, missing, wrong-schema and wrong-revision state each end in
/// RecoveryFailed with no published replacement; healing restores service.
pub fn restore_refusals(client: &mut Client) {
    for (kind, label) in [
        (corrupt::CHECKSUM, "corrupt"),
        (corrupt::MISSING, "missing"),
        (corrupt::SCHEMA, "wrong-schema"),
        (corrupt::REVISION, "wrong-revision"),
    ] {
        let before = client.get();
        let old = client.worker;
        if client.authority_call(Msg::Corrupt { kind }) != Some(Msg::Ack) {
            fail(&format!("E6 {label}: inject"));
        }
        client.wait_status(label, |s| s.state == state::RECOVERY_FAILED && s.worker == 0);
        if common::lookup(WORKER_SERVICE).is_some() {
            fail(&format!("E6 {label}: a replacement was published"));
        }
        println(&format!("[recovery-client] E6 PASS {label}: RecoveryFailed, no Ready (worker service unpublished)"));
        if client.authority_call(Msg::Heal) != Some(Msg::Ack) {
            fail(&format!("E6 {label}: heal"));
        }
        client.await_replacement_after_failure(old);
        let after = client.get();
        if (after.0, after.1) != (before.0, before.1) {
            fail(&format!("E6 {label}: healed state differs"));
        }
    }
}

/// E11: every published replacement is a new `(cell, generation)`.
pub fn identities(client: &Client) {
    let seen = &client.incarnations;
    let mut distinct: Vec<(u64, u64)> = Vec::new();
    for id in seen {
        if distinct.contains(id) {
            fail(&format!("E11: incarnation {id:?} published twice"));
        }
        distinct.push(*id);
    }
    if distinct.len() < 2 {
        fail("E11: no replacement observed");
    }
    println(&format!("[recovery-client] E11 PASS {} published worker incarnations, all distinct (cell, generation); stale epochs refused (E5)",
        distinct.len()));
}

/// E12: ten crash/restore cycles. Settled free frames must not drift and every
/// retirement must be Contained with nothing quarantined or leaked.
pub fn ten_cycles(client: &mut Client) -> u32 {
    let ledger = client.status().ledger;
    let mut settled: Vec<u64> = Vec::new();
    let mut remote = 0;
    for cycle in 1..=10 {
        let before = free_frames();
        let v = client.crash(crash::SPIN, 0);
        common::sleep(20);
        let _ = client.get();
        let after = free_frames();
        let status = client.status();
        println(&format!(
            "[recovery-client] E12 cycle={cycle} free_before={before} free_after={after} state={} threads={} remote_harts={:#x} grants={} qpages={} leaked={} shm={} ledger={}",
            state_name(&v), v.member_threads, v.remote_hart_mask, v.grants_reclaimed, v.quarantined_pages,
            v.leaked_pages, v.shm_frames_released, status.ledger
        ));
        if state_name(&v) != "Contained" || v.quarantined_pages != 0 || v.leaked_pages != 0 || status.ledger != ledger {
            fail(&format!("E12 cycle {cycle}: unreconciled retirement"));
        }
        remote += u32::from(v.remote_hart_mask != 0);
        settled.push(after);
    }
    let steady = &settled[1..];
    let (low, high) = (steady.iter().min().copied().unwrap_or(0), steady.iter().max().copied().unwrap_or(0));
    if high != low {
        fail(&format!("E12: settled free frames drift {low}..{high} over cycles 2-10"));
    }
    println(&format!("[recovery-client] E12 PASS 10 cycles: all Contained, 0 quarantined, 0 leaked, settled free frames {low} constant (cycles 2-10), ledger {ledger}"));
    remote
}
