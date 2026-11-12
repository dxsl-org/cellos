//! Local service-call lifecycle witness (x86_64, QEMU q35, BIOS/Limine ISO).
//!
//! Boots the isolated witness image built by
//! `scripts/build-x86_64-c2c-lifecycle-ci.sh` and runs
//! `bench local-service-lifecycle`. Both the synchronous SDK and asynchronous
//! operation path must terminate with PeerGone when a provider consumes a request
//! and exits without replying. The same boot also exercises named VFS resolution
//! and a typed request/reply through the SDK, full64 saturation, real deadline
//! expiry and removal of requests belonging to an exited caller.
//!
//! Skips gracefully when the ISO or QEMU is absent (hard-fails under `CI=`).

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_x86, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
/// Existing async-lifecycle budget is unchanged.
const SCENARIO_TIMEOUT: u64 = 90;
/// Only the local witness adds the real 3000-tick (~30 s) expiry window.
const LOCAL_SCENARIO_TIMEOUT: u64 = SCENARIO_TIMEOUT + 45;
/// Phase 03d adds the real 3000-tick (~30 s) operation expiry plus the M5 and
/// sustained-load legs to `bench async-lifecycle`.
const MSW_SCENARIO_TIMEOUT: u64 = SCENARIO_TIMEOUT + 120;

const START: &str = "[local-lifecycle] START";
/// Leg 0: the additive `LookupServiceBound = 429` resolved a real service.
const VFS_BINDING_PREFIX: &str = "[local-lifecycle] VFS-BINDING";
/// Leg 0 negative: an id with no provider is reported as no binding.
const ABSENT_BINDING: &str = "[local-lifecycle] ABSENT-BINDING";
const PROVIDER_READY: &str = "[local-lifecycle] provider-ready";
const SYNC_CALLER_START: &str = "[local-lifecycle] sync-caller-start";
/// The SDK must return a typed failure, not remain stranded or decode a reply.
const SYNC_RETURNED: &str = "[local-lifecycle] SYNC-RESULT=RETURNED error=PeerGone";
const SYNC_LEG_DONE: &str = "[local-lifecycle] SYNC-LEG=complete";
const ASYNC_PEER_GONE: &str = "[local-lifecycle] ASYNC-TERMINAL=PEER-GONE";
/// Leg S: the SDK's caching handle resolved the same binding the raw opcode reports.
const SDK_BINDING: &str = "[local-lifecycle] SDK-BINDING";
/// Leg S: the liveness query answers true for a resolved binding, false for an unbound one.
const SDK_BINDING_LIVE: &str = "[local-lifecycle] SDK-BINDING-LIVE";
/// Leg S: a typed call through that handle still works end to end.
const SDK_VFS_CALL: &str = "[local-lifecycle] SDK-VFS-CALL=OK";
/// Leg S: a service with no live provider is refused, not sent to.
const SDK_ABSENT_BINDING: &str = "[local-lifecycle] SDK-ABSENT-BINDING=REFUSED";
const PASS: &str = "[local-lifecycle] PASS";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Path to the isolated witness ISO. Override with `CELLOS_X86_C2C_LIFECYCLE_ISO`.
fn lifecycle_iso_path() -> String {
    if let Ok(path) = std::env::var("CELLOS_X86_C2C_LIFECYCLE_ISO") {
        if !path.is_empty() {
            return path;
        }
    }
    repo_root()
        .join("build/vicell-x86-c2c-lifecycle.iso")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let iso = lifecycle_iso_path();
    let iso_exists = PathBuf::from(&iso).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_exists {
        eprintln!("SKIP x86_64 local-service lifecycle: witness ISO not built ({iso})");
        eprintln!("  Run: bash scripts/build-x86_64-c2c-lifecycle-ci.sh");
    }
    if !qemu_ok {
        eprintln!("SKIP x86_64 local-service lifecycle: qemu-system-x86_64 not found");
    }
    vicell_integration_tests::ci_guard(iso_exists && qemu_ok)
}

#[test]
fn x86_dead_provider_terminates_sync_and_async_calls() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&lifecycle_iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 shell prompt not reached: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("bench local-service-lifecycle");
    qemu.wait_for(PASS, LOCAL_SCENARIO_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "lifecycle witness did not complete: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    qemu.wait_for(SYNC_RETURNED, LOCAL_SCENARIO_TIMEOUT).unwrap_or_else(|e| {
        panic!("synchronous RPC stranded: {e}\n--- output ---\n{}", qemu.dump())
    });
    let output = qemu.dump();

    // Every behavior is witnessed in the guest; no source-shape assertions.
    for anchor in [
        START,
        VFS_BINDING_PREFIX,
        ABSENT_BINDING,
        SDK_BINDING,
        SDK_BINDING_LIVE,
        SDK_VFS_CALL,
        SDK_ABSENT_BINDING,
        PROVIDER_READY,
        SYNC_CALLER_START,
        SYNC_LEG_DONE,
        "[local-lifecycle] NESTED-REPLY=OK retained=true duplicate_quarantined=true",
        "[local-lifecycle] SDK-SLOT-CLEANUP=OK errors=70 reply_len=2",
        "[local-lifecycle] CANCEL-LATE-REPLY=OK outcome=Indeterminate next_seq=2",
        "[local-lifecycle] SATURATION-PROVIDER=OK received=65 busy_delivered=0",
        "[local-lifecycle] SATURATION=OK accepted=64 busy_delivered=0 terminal_charged=64 drained=64 reuse=Reply",
        "[local-lifecycle] DEADLINE-PROVIDER=OK queued_delivered=0 late_reply=refused",
        "[local-lifecycle] DEADLINE=OK queued=PreDispatchTimeout dispatched=Indeterminate elapsed_ticks>=3000 queued_delivered=0 reuse=Reply",
        "[local-lifecycle] CALLER-DEATH-CHILD=EXITING queued=4",
        "[local-lifecycle] CALLER-DEATH-PROVIDER=OK dead_delivered=0 fresh=Reply",
        "[local-lifecycle] CALLER-DEATH=OK queued=4 exit_observed=true dead_delivered=0 fresh=Reply",
        "[local-lifecycle] RESTART=OK old=PeerGone replacement=Reply old_token=refused dead_submit=PeerGone",
        "[local-lifecycle] EVENT-COEXISTENCE=OK raw_event=received rpc_pending=true reply=correlated raw_reply_absent=true",
        "[local-lifecycle] CONCURRENT-INPUT=OK rpc_calls=8 input_events=32 rpc_settled=false rpc_drained=8 input_drained=32 misplaced=0",
        "[local-lifecycle] MULTI-CALLER=OK a_held=64 a_busy=2 a_drained=64 b_replies=2 wrong_correlation=0 busy_delivered=0",
        // Phase 03e: admission headroom. One Cell queues at most 56 of the 64
        // peer-mailbox slots; an independent caller is admitted at once and is
        // never refused while that Cell keeps its share full.
        "[local-lifecycle] PRESSURE-PROVIDER=OK b=64 duplicate=0 extra_delivery=0",
        "[local-lifecycle] PRESSURE-FILL a_fill=56 a_headroom_busy=true b_first_admitted=true",
        "[local-lifecycle] PEER-PRESSURE=OK a_fill=56 a_headroom_busy=true b_first_admitted=true b_fresh_refused=0 b_done=64 duplicate=0 wrong_correlation=0",
        // Phase 03e: an unbounded synchronous call held past the real expiry.
        "[local-lifecycle] SYNC-EXPIRY=OK unbounded=Indeterminate elapsed_ticks>=3000 late_reply=refused fresh=Reply",
    ] {
        assert!(
            output.contains(anchor),
            "missing lifecycle anchor {anchor:?}\n--- output ---\n{output}"
        );
    }
    // New probe legs must stay inside the bench allowlists.
    assert!(
        !output.contains("denied for tid"),
        "a syscall was denied during the local lifecycle\n--- output ---\n{output}"
    );

    // Leg S: the shipped SDK — not just the raw opcode — resolves the provider through
    // the binding, and a typed call through that handle still works. Without this the
    // binding would be an ABI nothing in the SDK consumed.
    let sdk_line = output
        .lines()
        .find(|line| line.contains(SDK_BINDING))
        .expect("SDK-BINDING line");
    assert!(
        sdk_line.contains("matches_raw=true"),
        "ServiceRef resolved a different binding than the raw lookup reports: {sdk_line}"
    );
    assert!(
        !sdk_line.contains("tid=None"),
        "ServiceRef did not resolve a live provider at all: {sdk_line}"
    );
    // Leg S: the binding the handle holds is verified against the registry, and an
    // unbound handle is not live rather than guessed at.
    let live_line = output
        .lines()
        .find(|line| line.contains(SDK_BINDING_LIVE))
        .expect("SDK-BINDING-LIVE line");
    assert!(
        live_line.contains("resolved=true") && live_line.contains("unresolved=false"),
        "the liveness query disagreed with the bindings it was asked about: {live_line}"
    );
    // Leg S negative: no live binding is a refusal, never a tid to send to.
    assert!(
        output.contains(SDK_ABSENT_BINDING),
        "a service with no live provider was not refused\n--- output ---\n{output}"
    );

    // Leg 0: the additive opcode resolved the real provider. The binding must name
    // the same task `LookupService` does, and must carry a live Cell identity — a
    // capture bug stores 0/0, which makes the kernel report *no* binding instead.
    let vfs_line = output
        .lines()
        .find(|line| line.contains(VFS_BINDING_PREFIX))
        .expect("VFS-BINDING line");
    assert!(
        vfs_line.contains("matches_lookup=true"),
        "LookupServiceBound disagreed with LookupService on the provider tid: {vfs_line}"
    );
    assert!(
        !vfs_line.contains("cell=0 ") && !vfs_line.contains("gen=0 "),
        "the binding carried no live Cell identity — the provider's generation was \
         not captured at registration: {vfs_line}"
    );

    // Leg 0 negative: an unregistered id is "no binding", stated the same way by
    // both opcodes.
    let absent_line = output
        .lines()
        .find(|line| line.contains(ABSENT_BINDING))
        .expect("ABSENT-BINDING line");
    assert!(
        absent_line.contains("lookup=None") && absent_line.contains("bound_is_none=true"),
        "an unregistered service must resolve to no provider and no binding: {absent_line}"
    );

    // Bounded exact-operation path: the kernel binds the peer by
    // (cell_id, cell_generation) and transitions the operation to a terminal.
    assert!(
        output.contains(ASYNC_PEER_GONE),
        "the exact-operation path did not report a PEER-GONE terminal\n--- output ---\n{output}"
    );

    assert!(
        output.contains(SYNC_RETURNED) && !output.contains("[local-lifecycle] FAIL"),
        "SDK RPC must terminate with PeerGone, without a scenario failure\n{output}"
    );
}

/// Phase-03 step 1: the bounded path carries several outstanding calls to one peer,
/// each with exactly one correlated completion, and terminalises every outstanding
/// operation when that peer dies mid-flight. The measurements (latency, wait rounds)
/// are printed by the scenario and recorded in
/// `docs/evidence/c2c-async-lifecycle-x86.{txt,log}`; this test pins the contract.
/// Phase 03d adds the multi-source `WaitCompletion` legs (`MSW-*`).
#[test]
fn x86_bounded_calls_complete_exactly_once_and_terminalise_on_peer_death() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios(&lifecycle_iso_path());
    assert_async_lifecycle(qemu, "-smp 1");
}

/// Phase 03d A6 lane: the same scenario on a qualified two-CPU topology. The
/// interleaving is not forced; the deterministic host model is the race proof.
#[test]
fn x86_multi_source_wait_holds_on_two_cpus() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios_smp(&lifecycle_iso_path(), 2);
    assert_async_lifecycle(qemu, "-smp 2");
}

fn assert_async_lifecycle(mut qemu: QemuRunner, topology: &str) {
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 shell prompt not reached ({topology}): {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("bench async-lifecycle");
    qemu.wait_for("[async-lifecycle] PASS", MSW_SCENARIO_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "async lifecycle scenario did not complete ({topology}): {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
    let output = qemu.dump();
    if topology == "-smp 2" {
        assert!(
            output.contains("[x86-smp] topology online=2"),
            "two-CPU lane did not bring the second CPU online\n--- output ---\n{output}"
        );
    }

    // The scenario's own failure marker must be absent: every leg asserts itself, so
    // reaching PASS is not enough evidence on its own.
    assert!(
        !output.contains("[async-lifecycle] FAIL"),
        "the async lifecycle scenario reported a failure\n--- output ---\n{output}"
    );
    // Phase 03d: the SDK chooses its wait from the Cell's own allowlist, so a
    // Cell without WaitCompletion authority (bench) must never be denied one.
    assert!(
        !output.contains("denied for tid"),
        "a syscall was denied during the scenario ({topology})\n--- output ---\n{output}"
    );

    // Leg 1: eight outstanding calls, eight completions, none lost, none mis-correlated
    // (the provider answers in reverse, so arrival order cannot stand in for identity).
    let bounded_line = output
        .lines()
        .find(|line| line.contains("[async-lifecycle] BOUNDED "))
        .expect("BOUNDED summary line");
    for expected in [
        "completed=8",
        "lost=0",
        "wrong_seq=0",
        "freq_hz=",
        "wait_rounds=",
    ] {
        assert!(
            bounded_line.contains(expected),
            "bounded leg summary is missing {expected}: {bounded_line}"
        );
    }
    assert!(
        output.contains("[async-lifecycle] BOUNDED-RELEASED ok=true"),
        "the provider was not released after the completions were taken\n--- output ---\n{output}"
    );

    // Leg 2: `TrySend` refuses a non-blocking delivery in its *value*, not as an error.
    let try_line = output
        .lines()
        .find(|line| line.contains("[async-lifecycle] TRY-SEND"))
        .expect("TRY-SEND line");
    assert!(
        try_line.contains("parked_refused=false"),
        "try_send refused a frame to a parked receiver: {try_line}"
    );
    assert!(
        try_line.contains("busy_refused=true"),
        "try_send accepted a frame for a receiver that was not in Recv: {try_line}"
    );
    assert!(
        output.contains("[async-lifecycle] slow-provider-second queued=false"),
        "a frame the kernel refused was nevertheless queued\n--- output ---\n{output}"
    );

    // Leg 3: every outstanding operation reaches a terminal when the peer dies, and a
    // pre-dispatch refusal is counted as the definite outcome it is, not as a loss.
    let death_line = output
        .lines()
        .find(|line| line.contains("[async-lifecycle] DEATH "))
        .expect("DEATH summary line");
    for expected in ["unterminal=0", "lost=0", "terminal="] {
        assert!(
            death_line.contains(expected),
            "mid-flight-death leg is missing {expected}: {death_line}"
        );
    }

    // Phase 03d: the multi-source caller (completion path, bit 42) and its legs.
    assert!(
        output.contains("[async-lifecycle] MSW PASS"),
        "multi-source caller did not report success\n--- output ---\n{output}"
    );
    for (marker, expected) in [
        (
            "[async-lifecycle] MSW-REPLY ",
            &["records=8", "dup=0", "foreign=0", "rereport=0", "wrong_seq=0"][..],
        ),
        ("[async-lifecycle] MSW-M5 ", &["pass=true"][..]),
        (
            "[async-lifecycle] MSW-MIXED ",
            &[
                "timer=true",
                "interrupted_by_raw=true",
                "ipc_after_raw=true",
                "raw_ignored_without_netrx=true",
                "ipc_second=true",
                "netrx_records=0",
            ][..],
        ),
        (
            "[async-lifecycle] MSW-DEATH ",
            &["records=4", "peer_gone=4", "dup=0", "unterminal=0"][..],
        ),
        (
            "[async-lifecycle] MSW-STARVATION ",
            &["b_done=8", "b_during_flood=8", "lost=0", "dup=0", "foreign=0"][..],
        ),
        (
            "[async-lifecycle] MSW-DEADLINE ",
            &["timer=true", "op_expiry=true", "terminal=Indeterminate"][..],
        ),
        (
            "[async-lifecycle] MSW-STALE ",
            &[
                "late_reply_refused=true",
                "fresh_reply=true",
                "stale_records=0",
                "other_records=0",
                "quiet_after=true",
            ][..],
        ),
        // Phase 03e: dispatched cancellation is never "not executed" (masked
        // provider), the sync caller budget, and dropped interest.
        (
            "[async-lifecycle] CANCEL-RACE ",
            &[
                "total=256",
                "other=0",
                "cancelled_but_received=0",
                "received_but_definite=0",
                "delivered_twice=0",
                "unterminal=0",
            ][..],
        ),
        (
            "[async-lifecycle] SYNC-TIMEOUT-LATE ",
            &[
                "queued=Some(PreDispatchTimeout)",
                "queued_delivered=false",
                "first=Some(Indeterminate)",
                "late_explicit_refused=true",
                "late_send_raw_absent=true",
                "next_reply=true",
            ][..],
        ),
        (
            "[async-lifecycle] DROP-RECLAIM ",
            &["dropped=192", "fresh_accepted=64", "late_accepted=0"][..],
        ),
    ] {
        let line = output
            .lines()
            .find(|line| line.contains(marker))
            .unwrap_or_else(|| panic!("{marker} line missing ({topology})\n{output}"));
        for needle in expected {
            assert!(
                line.contains(needle),
                "{marker} is missing {needle} ({topology}): {line}"
            );
        }
    }
}

/// Phase 11 (CP1 §4.1): a client that sends to the supervisor, config or
/// compositor by raw `Send` and never receives must not park the service; each
/// one then answers a second client's bounded call.
#[test]
fn x86_services_do_not_park_on_a_client_that_never_receives() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&lifecycle_iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 shell prompt not reached: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("bench nonreceiving-client");
    qemu.wait_for("[nonreceiving] PASS", SCENARIO_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "non-receiving client witness did not complete: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
    let output = qemu.dump();
    assert!(
        !output.contains("[nonreceiving] FAIL"),
        "witness reported a failure\n--- output ---\n{output}"
    );
    for service in ["config", "supervisor", "compositor"] {
        let served =
            format!("[nonreceiving] {service} SERVED second=Reply first_client_receiving=false");
        assert!(
            output.contains(&served),
            "{service} was not witnessed serving a second client\n--- output ---\n{output}"
        );
    }
}
