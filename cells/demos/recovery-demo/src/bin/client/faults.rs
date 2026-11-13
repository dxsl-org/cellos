//! E1–E5, E7 and E10: crash points around the commit, conflicting bindings,
//! a fenced live writer, whole-Cell retirement and an abandoned lock word.

use alloc::format;
use ostd::io::println;
use recovery_demo::crash;
use recovery_demo::ledger::{METHOD_ADD_TWICE, METHOD_INC};
use recovery_demo::wire::{Fault, Msg, Refusal};

use crate::driver::{fail, state_name, Client};

fn expect_contained(step: &str, v: &abi::retirement::ViRetirementV1) {
    if state_name(v) != "Contained" {
        fail(&format!(
            "{step}: retirement {} reasons={:#x}",
            state_name(v),
            v.reasons
        ));
    }
}

/// E1 fault before commit, E2 fault after commit before reply.
pub fn commit_faults(client: &mut Client) {
    let (base, _, _) = client.get();
    let old = client.worker;
    if client
        .mutate(1001, METHOD_INC, 1, Fault::BeforeCommit)
        .is_some()
    {
        fail("E1: crashed worker replied");
    }
    expect_contained("E1", &client.await_replacement(old));
    match client.mutate(1001, METHOD_INC, 1, Fault::None) {
        Some(Msg::Done {
            replayed: false,
            result,
            ..
        }) if result == base + 1 => {}
        other => fail(&format!("E1 resubmit: {other:?}")),
    }
    if client.get().0 != base + 1 {
        fail("E1: committed value");
    }
    println(&format!("[recovery-client] E1 PASS fault-before-commit: old revision kept, resubmit applied once (counter {})", base + 1));

    let old = client.worker;
    if client
        .mutate(1002, METHOD_INC, 1, Fault::AfterCommit)
        .is_some()
    {
        fail("E2: crashed worker replied");
    }
    expect_contained("E2", &client.await_replacement(old));
    match client.mutate(1002, METHOD_INC, 1, Fault::None) {
        Some(Msg::Done {
            replayed: true,
            result,
            ..
        }) if result == base + 2 => {}
        other => fail(&format!("E2 resubmit: {other:?}")),
    }
    if client.get().0 != base + 2 {
        fail("E2: reapplied");
    }
    println(&format!("[recovery-client] E2 PASS fault-after-commit: recorded outcome returned, not reapplied (counter {})", base + 2));
}

/// E3 same id different payload, E4 same id different method (authority).
pub fn conflicts(client: &mut Client) {
    let before = client.get();
    for (step, method, amount) in [("E3", METHOD_INC, 5), ("E4", METHOD_ADD_TWICE, 1)] {
        match client.mutate(1002, method, amount, Fault::None) {
            Some(Msg::Failed {
                refusal: Refusal::Conflict,
                ..
            }) => {}
            other => fail(&format!("{step}: {other:?}")),
        }
        if client.get() != before {
            fail(&format!("{step}: state changed"));
        }
    }
    println("[recovery-client] E3 PASS same request id with a different payload refused (Conflict), state unchanged");
    println("[recovery-client] E4 PASS same request id with a different method/authority refused (Conflict), state unchanged");
}

/// E5: a live writer fenced at the authority cannot commit afterwards.
pub fn fenced_writer(client: &mut Client) {
    let (counter, revision, epoch) = client.get();
    let old = client.worker;
    match client.mutate(1003, METHOD_INC, 1, Fault::Stall) {
        Some(Msg::Staged { request_id: 1003 }) => {}
        other => fail(&format!("E5 stage: {other:?}")),
    }
    if client.authority_call(Msg::Fence) != Some(Msg::Ack) {
        fail("E5: fence");
    }
    // `Go` is its own request: the stalled worker answers it with the commit outcome.
    match crate::common::call(old, Msg::Go, 400) {
        Some(Msg::Failed {
            refusal: Refusal::StaleEpoch,
            ..
        }) => {}
        other => fail(&format!("E5 delayed commit: {other:?}")),
    }
    expect_contained("E5", &client.await_replacement(old));
    let (after, after_revision, after_epoch) = client.get();
    if (after, after_revision) != (counter, revision) || after_epoch <= epoch {
        fail("E5: stale writer changed state or epoch did not advance");
    }
    println(&format!("[recovery-client] E5 PASS old writer refused StaleEpoch after fence; revision {revision} kept; epoch {epoch} -> {after_epoch}"));
}

/// E7: helper thread spinning on another hart while the root faults.
pub fn whole_cell(client: &mut Client) -> u64 {
    let v = client.crash(crash::SPIN, 0);
    expect_contained("E7", &v);
    if v.member_threads < 2 {
        fail(&format!(
            "E7: only {} member thread(s) retired",
            v.member_threads
        ));
    }
    println(&format!("[recovery-client] E7 PASS whole-Cell retirement: threads={} remote_harts={:#x} state=Contained",
        v.member_threads, v.remote_hart_mask));
    v.remote_hart_mask
}

/// E10: the dead writer leaves a set lock word in a page it borrowed writable.
pub fn abandoned_lock(client: &mut Client) {
    let grant = match client.authority_call(Msg::Lend) {
        Some(Msg::Lent { grant }) => grant,
        other => fail(&format!("E10 lend: {other:?}")),
    };
    let before = client.get();
    let v = client.crash(crash::LOCK_AND_DIE, grant);
    if v.writable_borrows == 0 || v.reasons & abi::retirement::reason::WRITABLE_BORROW == 0 {
        fail(&format!(
            "E10: writable borrow not reported ({:#x})",
            v.reasons
        ));
    }
    let status = client.status();
    if status.lock_word != 1 {
        fail(&format!("E10: lock word {}", status.lock_word));
    }
    let after = client.get();
    if (after.0, after.1) != (before.0, before.1) {
        fail("E10: committed state changed");
    }
    println(&format!("[recovery-client] E10 PASS writable borrow reported ({}), lock word 1 left held and page discarded, state restored from committed revision {}",
        v.writable_borrows, after.1));
}
