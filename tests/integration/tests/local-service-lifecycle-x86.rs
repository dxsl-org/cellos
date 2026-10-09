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
        "[local-lifecycle] MULTI-CALLER=OK a_held=64 a_busy=2 a_drained=64 b_replies=2 wrong_correlation=0 busy_delivered=0",
        "[local-lifecycle] PRESSURE-PROVIDER=OK a=128 b=64 duplicate=0 extra_delivery=0",
        "[local-lifecycle] PEER-PRESSURE=OK a=128 b=64 completions=192 initial_peer_busy=true duplicate=0 wrong_correlation=0",
    ] {
        assert!(
            output.contains(anchor),
            "missing lifecycle anchor {anchor:?}\n--- output ---\n{output}"
        );
    }

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
#[test]
fn x86_bounded_calls_complete_exactly_once_and_terminalise_on_peer_death() {
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
    qemu.send_line("bench async-lifecycle");
    qemu.wait_for("[async-lifecycle] PASS", SCENARIO_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "async lifecycle scenario did not complete: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });
    let output = qemu.dump();

    // The scenario's own failure marker must be absent: every leg asserts itself, so
    // reaching PASS is not enough evidence on its own.
    assert!(
        !output.contains("[async-lifecycle] FAIL"),
        "the async lifecycle scenario reported a failure\n--- output ---\n{output}"
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
}
