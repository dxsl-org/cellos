//! Private Phase02 driver. It exercises the real kernel/input/VFS owners and
//! prints `P02 PASS|FAIL <name>` observations; the host runner supplies the
//! independent verdict from serial output. Not part of any ordinary image.

#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::cell_session::{ChildRefV1, ObjectRefV1, Status};
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::child_context::{ChildContext, Grant, GrantSlot};
use ostd::io::println;
use ostd::syscall::{sys_exit, sys_set_timer};
use terminal_sdk::input::EventKind;
use terminal_sdk::session::{ForegroundChild, TerminalSession};
use terminal_sdk::stream::{Endpoint, EndpointRef, EndpointRights, Operation};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    RecvTimeout,
    Reply,
    Log,
    Exit,
    Yield,
    Heartbeat,
    LookupService,
    GetTime,
    SetTimer,
    MemInfo,
    // PrepareChild/CommitChild require the governed-launch bit.
    SpawnFromPath,
];

ostd::cell_main!(cell_main);

const CHILD: &str = "/bin/phase02-session-child";
static FAILURES: AtomicUsize = AtomicUsize::new(0);
static CHECKS: AtomicUsize = AtomicUsize::new(0);

fn now() -> u64 {
    ostd::session::monotonic_ns().unwrap_or(0)
}

fn sleep_ms(ms: u64) {
    let end = now() + ms * 1_000_000;
    while now() < end {
        let _ = sys_set_timer(1);
    }
}

fn check(name: &str, ok: bool, detail: &str) {
    CHECKS.fetch_add(1, Ordering::Relaxed);
    if ok {
        println(&format!("P02 PASS {name}"));
    } else {
        FAILURES.fetch_add(1, Ordering::Relaxed);
        println(&format!("P02 FAIL {name} {detail}"));
    }
}

fn status<T>(result: &Result<T, Status>) -> String {
    match result {
        Ok(_) => String::from("Ok"),
        Err(status) => format!("{status:?}"),
    }
}

fn text_of(event: terminal_sdk::input::Event) -> Option<String> {
    match event.kind {
        EventKind::Text(text) => Some(text),
        _ => None,
    }
}

fn probe_grants(grants: &[(GrantSlot, EndpointRights)]) -> Result<Vec<Grant>, Status> {
    let mut out = Vec::new();
    for (slot, rights) in grants {
        out.push(Grant::endpoint(*slot, ObjectRefV1 { id: 1, generation: 1 }, rights.bits() as u16)?);
    }
    Ok(out)
}

/// Commit one child under `operation` with exactly the delegated endpoints.
fn spawn(
    operation: &Operation,
    argv: &[&str],
    grants: &[(GrantSlot, Endpoint, EndpointRights)],
) -> Result<ChildRefV1, Status> {
    let shape: Vec<(GrantSlot, EndpointRights)> = grants.iter().map(|(s, _, r)| (*s, *r)).collect();
    let probe = ChildContext::new(operation.reference().raw(), argv, b"", &[], &probe_grants(&shape)?)?;
    let mut prepared = operation.prepare_child(CHILD, probe.as_bytes().len())?;
    let mut real = Vec::new();
    for (slot, endpoint, rights) in grants {
        let delegated = endpoint.delegate(&prepared, *rights)?;
        real.push(Grant::endpoint(*slot, delegated.reference().raw(), rights.bits() as u16)?);
    }
    let context = ChildContext::new(operation.reference().raw(), argv, b"", &[], &real)?;
    prepared.write_context(&context)?;
    prepared.commit()
}

fn settle(session: &mut TerminalSession, child: &mut ForegroundChild) -> Result<(), Status> {
    let deadline = now() + 10_000_000_000;
    loop {
        match session.finish_foreground(child) {
            Ok(()) => return Ok(()),
            Err(Status::WouldBlock) => {}
            Err(other) => return Err(other),
        }
        if now() >= deadline {
            return Err(Status::TimedOut);
        }
        let _ = sys_set_timer(1);
    }
}

fn drain_until(session: &mut TerminalSession, stderr: bool, needle: &str, wait_ms: u64) -> String {
    let end = now() + wait_ms * 1_000_000;
    let mut all = Vec::new();
    loop {
        let mut buffer = [0u8; 512];
        let read = if stderr {
            session.drain_stderr(&mut buffer)
        } else {
            session.drain_stdout(&mut buffer)
        };
        if let Ok(n) = read {
            all.extend_from_slice(&buffer[..n]);
        }
        let text = String::from_utf8_lossy(&all).into_owned();
        if text.contains(needle) || now() >= end {
            return text;
        }
        let _ = sys_set_timer(1);
    }
}

fn cell_main() {
    println("P02 START");
    let mut a = match TerminalSession::create_serial(24, 80) {
        Ok(session) => session,
        Err(status) => {
            println(&format!("P02 FAIL create-a {status:?}"));
            sys_exit(2);
        }
    };
    let mut b = match TerminalSession::create_serial(25, 81) {
        Ok(session) => session,
        Err(status) => {
            println(&format!("P02 FAIL create-b {status:?}"));
            sys_exit(2);
        }
    };
    two_sessions(&mut a, &mut b);
    context_and_foreground(&mut a);
    fault_and_cancel(&mut a, &mut b);
    stream_semantics();
    descendants();
    consumer_close_and_cancel(&mut b);
    forged_and_prepare();
    let _ = a.close();
    let _ = b.close();
    let failures = FAILURES.load(Ordering::Relaxed);
    println(&format!(
        "P02 NOT-EXERCISED issuer-death commit-race-wall-clock cross-process-frontend gui-bridge"
    ));
    if failures == 0 {
        println(&format!("P02-SESSION: PASS checks={}", CHECKS.load(Ordering::Relaxed)));
    } else {
        println(&format!("P02-SESSION: FAIL failures={failures}"));
    }
    orphan();
    sys_exit(if failures == 0 { 0 } else { 1 });
}

fn two_sessions(a: &mut TerminalSession, b: &mut TerminalSession) {
    let _ = a.submit_text("alpha");
    let _ = b.submit_text("beta");
    let deadline = now() + 3_000_000_000;
    let ea = a.next_event(deadline);
    let eb = b.next_event(deadline);
    check(
        "two-session-routing",
        ea.clone().ok().and_then(text_of).as_deref() == Some("alpha")
            && eb.clone().ok().and_then(text_of).as_deref() == Some("beta"),
        &format!("a={} b={}", status(&ea), status(&eb)),
    );
    check(
        "two-session-no-leak",
        a.next_event(0) == Err(Status::WouldBlock) && b.next_event(0) == Err(Status::WouldBlock),
        "event crossed or lingered",
    );
    let resize = a.resize(30, 100);
    check("resize-owner", resize.is_ok(), &status(&resize));
    let ctl = a.next_event(now() + 2_000_000_000);
    check(
        "resize-control-frame",
        matches!(ctl.as_ref().map(|e| &e.kind), Ok(EventKind::Resize { rows: 30, cols: 100 })),
        &status(&ctl),
    );
    check("resize-isolated", b.next_event(0) == Err(Status::WouldBlock), "resize leaked to b");
}

fn context_and_foreground(a: &mut TerminalSession) {
    let argv = ["ctx", "", "two words", "h\u{e9}llo"];
    let env = [("A", "1"), ("EMPTY", ""), ("Z", "last")];
    // A cell that inherited a directory grant is path-sealed: its child's cwd
    // must derive from that grant, never from an ambient path lookup.
    let cwd_source = ChildContext::read_own().ok().and_then(|own| own.cwd_grant());
    let non_utf8 = a.spawn_foreground(CHILD, &["ctx"], b"/\xff", &[], cwd_source);
    check("non-utf8-cwd-refused-before-launch", matches!(non_utf8, Err(Status::UnsupportedInput)), &status(&non_utf8));
    let big = "x".repeat(600);
    let over = a.spawn_foreground(CHILD, &[big.as_str()], b"", &[], None);
    check("argv-limit-refused-before-launch", matches!(over, Err(Status::QuotaExceeded)), &status(&over));

    let spawned = a.spawn_foreground(CHILD, &argv, b"/", &env, cwd_source);
    check("foreground-spawn", spawned.is_ok(), &status(&spawned));
    let Ok(mut child) = spawned else { return };
    check("foreground-activation", child.activation_error().is_none(), "reconciliation error");
    let expected = format!(
        "ctx|argc=4|argv={:?}|cwd=/|cwdgrant=true|env=A=1;EMPTY=;Z=last;",
        argv.iter().map(|s| String::from(*s)).collect::<Vec<_>>()
    );
    let out = drain_until(a, false, &expected, 4000);
    check("context-exact-at-first-instruction", out.contains(&expected), &out);
    let err = drain_until(a, true, "diag|ctx", 2000);
    check("stderr-separate-from-stdout", err.contains("diag|ctx") && !out.contains("diag|"), &err);
    let _ = a.submit_text("typed-1");
    let got = drain_until(a, false, "got|typed-1", 3000);
    check("typed-input-to-foreground-only", got.contains("got|typed-1"), &got);
    let ended = a.end_input();
    check("end-input-half-close", ended.is_ok(), &status(&ended));
    let eof = drain_until(a, false, "eof|seen", 3000);
    check("eof-reaches-child-outputs-preserved", eof.contains("eof|seen"), &eof);
    let settled = settle(a, &mut child);
    check("foreground-restored-after-exit", settled.is_ok(), &status(&settled));
    let _ = a.submit_text("after-exit");
    let back = a.next_event(now() + 3_000_000_000);
    check(
        "baseline-input-after-restore",
        back.ok().and_then(text_of).as_deref() == Some("after-exit"),
        "baseline did not regain input",
    );
}

fn fault_and_cancel(a: &mut TerminalSession, b: &mut TerminalSession) {
    for (label, mode) in [("fault", "panic"), ("raw-mode-fault", "raw-panic"), ("timeout", "timeout")] {
        let spawned = a.spawn_foreground(CHILD, &[mode], b"", &[], None);
        let Ok(mut child) = spawned else {
            check(label, false, &status(&spawned));
            continue;
        };
        let settled = settle(a, &mut child);
        let _ = a.submit_text("typed");
        let typed = a.next_event(now() + 3_000_000_000);
        check(
            &format!("{label}-restores-foreground-and-mode"),
            settled.is_ok() && typed.ok().and_then(text_of).as_deref() == Some("typed"),
            &status(&settled),
        );
    }
    for label in ["cancel", "ctrl-c"] {
        let spawned = a.spawn_foreground(CHILD, &["hang"], b"", &[], None);
        let Ok(mut child) = spawned else {
            check(label, false, &status(&spawned));
            continue;
        };
        sleep_ms(600);
        if label == "cancel" {
            let _ = a.cancel_foreground(&child, 1);
        } else {
            let _ = a.submit_key(0x63, 2, 1);
        }
        let settled = settle(a, &mut child);
        check(&format!("{label}-restores-foreground"), settled.is_ok(), &status(&settled));
        let _ = b.submit_text("independent");
        let other = b.next_event(now() + 3_000_000_000);
        check(
            &format!("{label}-other-session-unaffected"),
            other.ok().and_then(text_of).as_deref() == Some("independent"),
            "unrelated session disturbed",
        );
    }
}

fn stream_semantics() {
    let Ok(op) = Operation::create(None) else {
        check("stream-operation", false, "create");
        return;
    };
    let Ok((r, w)) = op.create_stream(64) else {
        check("stream-create", false, "create_stream");
        return;
    };
    // Raw bytes (invalid UTF-8, NUL, DEL, ESC) cross a stream unchanged.
    {
        let raw = [0xffu8, 0x00, 0xfe, 0x80, 0x7f, 0x1b, 0xc3, 0x28, 0xa0, 0xa1];
        let sent = w.write(&raw, 0);
        let mut got = [0u8; 16];
        let read = r.read(&mut got, now() + 2_000_000_000);
        check(
            "non-utf8-nul-bytes-preserved",
            matches!((&sent, &read), (Ok(10), Ok(10))) && got[..10] == raw,
            &format!("sent={} read={}", status(&sent), status(&read)),
        );
    }
    let big = [7u8; 200];
    let first = w.write(&big, 0);
    check("partial-write-bounded", matches!(first, Ok(n) if n >= 1 && n <= 64), &status(&first));
    let mut filled = first.unwrap_or(0);
    loop {
        match w.write(&big, 0) {
            Ok(n) => filled += n,
            Err(status) => {
                check("full-ring-would-block", status == Status::WouldBlock && filled == 64, &format!("{status:?} filled={filled}"));
                break;
            }
        }
        if filled > 64 {
            check("full-ring-would-block", false, "ring exceeded capacity");
            break;
        }
    }
    let mut buffer = [0u8; 128];
    let mut drained = 0usize;
    while let Ok(n) = r.read(&mut buffer, 0) {
        drained += n;
    }
    check("bytes-preserved-count", drained == filled, &format!("drained={drained} filled={filled}"));
    check("empty-nonblocking-is-would-block", r.read(&mut buffer, 0) == Err(Status::WouldBlock), "");
    let timed = r.read(&mut buffer, now() + 30_000_000);
    check("timeout-distinct-from-eof", timed == Err(Status::TimedOut), &status(&timed));
    let duplicate = w.duplicate(EndpointRights::WRITE_ONLY);
    let _ = w.close();
    check(
        "retained-duplicate-prevents-eof",
        duplicate.is_ok() && r.read(&mut buffer, 0) == Err(Status::WouldBlock),
        "EOF despite retained writer",
    );
    if let Ok(duplicate) = duplicate {
        let _ = duplicate.close();
    }
    let eof = r.read(&mut buffer, 0);
    check("last-writer-eof", eof == Err(Status::Eof), &status(&eof));
    check("closed-ref-stale", r.close().is_ok() && r.read(&mut buffer, 0).is_err(), "closed ref still usable");
    if let Ok((r2, w2)) = op.create_stream(64) {
        let _ = r2.close();
        let broken = w2.write(b"x", 0);
        check("no-reader-write-refused", matches!(broken, Err(Status::BrokenPipe | Status::Revoked | Status::Stale)), &status(&broken));
    }
    // The consumer close above retired `op`; a forged reference needs a live one.
    match Operation::create(None).and_then(|fresh| fresh.create_stream(64).map(|ends| (fresh, ends))) {
        Ok((fresh, (r3, w3))) => {
            let raw = w3.reference().raw();
            let forged = EndpointRef::from_raw(ObjectRefV1 { id: raw.id + 7777, generation: raw.generation })
                .map(|reference| Endpoint::new(reference, fresh.reference()));
            let attempt = forged.map(|e| e.write(b"x", 0));
            check("forged-endpoint-denied", matches!(attempt, Ok(Err(_))), &format!("{attempt:?}"));
            let _ = (r3.close(), w3.close());
            let _ = fresh.close();
        }
        Err(status) => check("forged-endpoint-denied", false, &format!("setup {status:?}")),
    }
    let _ = op.close();
}

fn descendants() {
    let Ok(op) = Operation::create(None) else { return };
    let Ok((r, w)) = op.create_stream(64) else { return };
    // The child nests its descendant under the operation it was delegated into.
    let spawned = spawn(&op, &["descend", "400"], &[(GrantSlot::Stdout, w, EndpointRights::WRITE_DELEGATE)]);
    check("descendant-spawn", spawned.is_ok(), &status(&spawned));
    let duplicate = w.duplicate(EndpointRights::WRITE_ONLY);
    let _ = w.close();
    let mut buffer = [0u8; 16];
    check(
        "descendant-and-duplicate-hold-writer",
        r.read(&mut buffer, 0) == Err(Status::WouldBlock),
        "EOF while descendant/duplicate writer retained",
    );
    if let Ok(duplicate) = duplicate {
        let _ = duplicate.close();
    }
    let data = r.read(&mut buffer, now() + 4_000_000_000);
    check("descendant-bytes-arrive", matches!(data, Ok(2)) && &buffer[..2] == b"gc", &status(&data));
    let eof = r.read(&mut buffer, now() + 4_000_000_000);
    check("eof-after-descendant-exit", eof == Err(Status::Eof), &status(&eof));
    let _ = op.close();

    // Holder death: the child faults while holding the delegated writer.
    let Ok(op) = Operation::create(None) else { return };
    let Ok((r, w)) = op.create_stream(64) else { return };
    let spawned = spawn(&op, &["panic"], &[(GrantSlot::Stdout, w, EndpointRights::WRITE_DELEGATE)]);
    let _ = w.close();
    let eof = r.read(&mut buffer, now() + 5_000_000_000);
    check("holder-death-closes-writer", spawned.is_ok() && eof == Err(Status::Eof), &status(&eof));
    let _ = op.close();
}

/// Poll the owner until the operation's delegated subtree has drained.
fn subtree_drained(op: &Operation, want_cancelled: bool) -> (bool, String) {
    let end = now() + 5_000_000_000;
    let mut last = String::from("no query");
    loop {
        match op.query() {
            Ok(state) => {
                last = format!("state={} holders={} children={}", state.state, state.holders, state.children);
                if state.children == 0 && (!want_cancelled || state.state == 1) {
                    return (true, last);
                }
            }
            Err(status) => last = format!("{status:?}"),
        }
        if now() >= end {
            return (false, last);
        }
        sleep_ms(50);
    }
}

fn consumer_close_and_cancel(b: &mut TerminalSession) {
    // Final consumer close must wake/reap a producer blocked in a child subtree.
    if let Ok(op) = Operation::create(None) {
        if let Ok((r, w)) = op.create_stream(64) {
            let spawned = spawn(&op, &["blocked-writer"], &[(GrantSlot::Stdout, w, EndpointRights::WRITE_DELEGATE)]);
            let _ = w.close();
            sleep_ms(700);
            let before = op.query().map(|s| s.children).unwrap_or(99);
            let closed = r.close();
            check("consumer-close-issued", spawned.is_ok() && closed.is_ok() && before >= 1, &format!("{} children_before={before}", status(&closed)));
            let (drained, detail) = subtree_drained(&op, false);
            check("consumer-close-reaps-blocked-producer-subtree", drained, &detail);
        }
    }
    // Root cancellation revokes the subtree; an unrelated session continues.
    if let Ok(op) = Operation::create(None) {
        if let Ok((r, w)) = op.create_stream(64) {
            let spawned = spawn(&op, &["blocked-reader"], &[(GrantSlot::Stdin, r, EndpointRights::READ_DELEGATE)]);
            sleep_ms(700);
            let before = op.query().map(|s| s.children).unwrap_or(99);
            let cancelled = op.cancel(2);
            check("root-cancel-issued", spawned.is_ok() && cancelled.is_ok() && before >= 1, &format!("{} children_before={before}", status(&cancelled)));
            let (drained, detail) = subtree_drained(&op, true);
            check("root-cancel-reaps-blocked-consumer-subtree", drained, &detail);
            let mut buffer = [0u8; 4];
            let after = w.write(b"x", 0);
            check("cancelled-subtree-endpoints-revoked", after.is_err() || r.read(&mut buffer, 0).is_err(), "cancelled endpoints live");
        }
    }
    let _ = b.submit_text("still-alive");
    let alive = b.next_event(now() + 3_000_000_000);
    check("unrelated-session-survives-cancel", alive.ok().and_then(text_of).as_deref() == Some("still-alive"), "");
}

fn forged_and_prepare() {
    let (Ok(private), Ok(foreign)) = (Operation::create(None), Operation::create(None)) else { return };
    if let Ok((pr, pw)) = private.create_stream(64) {
        let _ = pw.write(b"keep", 0);
        let op = private.reference().raw();
        let ep = pr.reference().raw();
        let (id, gen, eid, egen) = (format!("{}", op.id), format!("{}", op.generation), format!("{}", ep.id), format!("{}", ep.generation));
        let spawned = spawn(&foreign, &["forged", &id, &gen, &eid, &egen], &[]);
        sleep_ms(800);
        let mut buffer = [0u8; 8];
        let intact = pr.read(&mut buffer, 0);
        check(
            "foreign-child-cannot-touch-private-endpoint",
            spawned.is_ok() && matches!(intact, Ok(4)) && private.query().map(|s| s.state == 0).unwrap_or(false),
            &status(&intact),
        );
    }
    // Aborted and expired preparations never become runnable children.
    if let Ok(op) = Operation::create(None) {
        let probe = ChildContext::new(op.reference().raw(), &["unexpected-run"], b"", &[], &[]);
        if let Ok(probe) = probe {
            if let Ok(mut prepared) = op.prepare_child(CHILD, probe.as_bytes().len()) {
                let wrote = prepared.write_context(&probe);
                let aborted = prepared.abort();
                let late = prepared.commit();
                check("abort-then-commit-refused", wrote.is_ok() && aborted.is_ok() && late.is_err(), &status(&late));
            }
            if let Ok(mut prepared) = op.prepare_child(CHILD, probe.as_bytes().len()) {
                let _ = prepared.write_context(&probe);
                sleep_ms(5500);
                let late = prepared.commit();
                check("expired-prepare-refuses-commit", late.is_err(), &status(&late));
            }
        }
    }
}

/// Last act: exit while a child still blocks on a stream whose only writer this
/// cell holds. Its observation of the parent's death is read from the log.
fn orphan() {
    if let Ok(op) = Operation::create(None) {
        if let Ok((r, _w)) = op.create_stream(64) {
            let _ = spawn(&op, &["orphan"], &[(GrantSlot::Stdin, r, EndpointRights::READ_DELEGATE)]);
            sleep_ms(500);
        }
    }
}
