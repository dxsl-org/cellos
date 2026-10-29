//! Private Phase02 child fixture. Mode is `argv[0]`; every observation is made
//! through the real owner syscalls and printed as a `P02C` marker.
//!
//! Never part of an ordinary image: reachable only through the exact
//! `phase02-terminal-profile` launch rows.

#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::cell_session::{ObjectRefV1, Status};
use ostd::child_context::{ChildContext, Grant, GrantSlot};
use ostd::io::println;
use ostd::syscall::{sys_exit, sys_set_timer};
use terminal_sdk::input::EventKind;
use terminal_sdk::session::TerminalSession;
use terminal_sdk::stream::{Endpoint, EndpointRef, EndpointRights, Operation, OperationRef};

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

const CHILD_PATH: &str = "/bin/phase02-session-child";

fn now() -> u64 {
    ostd::session::monotonic_ns().unwrap_or(0)
}

fn sleep_ms(ms: u64) {
    let end = now() + ms * 1_000_000;
    while now() < end {
        let _ = sys_set_timer(1);
    }
}

fn mark(text: &str) {
    println(&format!("P02C {text}"));
}

fn parse_ref(id: &str, generation: &str) -> Option<ObjectRefV1> {
    Some(ObjectRefV1 {
        id: id.parse().ok()?,
        generation: generation.parse().ok()?,
    })
}

/// The committed endpoint a parent delegated into `slot`.
fn granted(context: &ChildContext, slot: GrantSlot) -> Option<Endpoint> {
    let operation = OperationRef::from_raw(context.operation()).ok()?;
    let grant = context.grants().find(|g| g.slot == slot && g.kind == 1)?;
    Some(Endpoint::new(EndpointRef::from_raw(grant.object).ok()?, operation))
}

fn argv_of(context: &ChildContext) -> Vec<String> {
    context.argv().map(String::from).collect()
}

fn cell_main() {
    let context = match ChildContext::read_own() {
        Ok(context) => context,
        Err(status) => {
            mark(&format!("FAIL read_own status={status:?}"));
            sys_exit(2);
        }
    };
    let argv = argv_of(&context);
    let mode = argv.first().map(String::as_str).unwrap_or("");
    match mode {
        "ctx" => ctx(&context, &argv),
        "raw-panic" => raw_panic(&context),
        "hang" => hang(&context),
        "timeout" => timeout(&context),
        "writer" => writer(&context, &argv),
        "descend" => descend(&context, &argv),
        "blocked-writer" => blocked_writer(&context),
        "blocked-reader" => blocked_reader(&context),
        "forged" => forged(&argv),
        "orphan" => orphan(&context),
        "panic" => panic!("P02C deliberate fault"),
        other => {
            mark(&format!("FAIL unknown mode {other}"));
            sys_exit(2);
        }
    }
    sys_exit(0);
}

/// Report exactly what the kernel handed this child at its first instruction,
/// on both owner-enforced output streams, then consume typed input to EOF.
fn ctx(context: &ChildContext, argv: &[String]) {
    let cwd = core::str::from_utf8(context.cwd()).unwrap_or("<non-utf8>");
    let mut env = String::new();
    for (key, value) in context.environment() {
        env.push_str(&format!("{key}={value};"));
    }
    let summary = format!(
        "ctx|argc={}|argv={:?}|cwd={}|cwdgrant={}|env={}",
        argv.len(),
        argv,
        cwd,
        context.cwd_grant().is_some(),
        env
    );
    mark(&summary);
    let mut session = match TerminalSession::from_own_context(context) {
        Ok(session) => session,
        Err(status) => {
            mark(&format!("FAIL from_own_context status={status:?}"));
            sys_exit(2);
        }
    };
    let _ = session.write_stdout(summary.as_bytes(), 0);
    let _ = session.write_stderr(b"diag|ctx", 0);
    let deadline = now() + 8_000_000_000;
    loop {
        match session.next_event(deadline) {
            Ok(event) => match event.kind {
                EventKind::Text(text) => {
                    mark(&format!("ctx text epoch={} seq={}", event.epoch, event.sequence));
                    let _ = session.write_stdout(format!("got|{text}").as_bytes(), 0);
                }
                EventKind::Eof => {
                    mark("ctx eof");
                    let _ = session.write_stdout(b"eof|seen", 0);
                    return;
                }
                _ => {}
            },
            Err(status) => {
                mark(&format!("ctx event status={status:?}"));
                return;
            }
        }
    }
}

/// Fault while a scoped raw mode lease is held: restoration must not need us.
fn raw_panic(context: &ChildContext) {
    let mut session = match TerminalSession::from_own_context(context) {
        Ok(session) => session,
        Err(status) => {
            mark(&format!("FAIL raw from_own_context status={status:?}"));
            return;
        }
    };
    let guard = session.set_mode(1, 1);
    mark(&format!("raw lease acquired={}", guard.is_ok()));
    let guard = guard.ok();
    sleep_ms(200);
    core::mem::forget(guard);
    panic!("P02C fault while raw");
}

/// A foreground child whose bounded typed read times out and then exits.
fn timeout(context: &ChildContext) {
    let mut session = match TerminalSession::from_own_context(context) {
        Ok(session) => session,
        Err(status) => {
            mark(&format!("FAIL timeout from_own_context status={status:?}"));
            return;
        }
    };
    let outcome = session.next_event(now() + 700_000_000).err();
    mark(&format!("timeout status={outcome:?}"));
}

/// Block in a typed read until the owner cancels this operation.
fn hang(context: &ChildContext) {
    let mut session = match TerminalSession::from_own_context(context) {
        Ok(session) => session,
        Err(status) => {
            mark(&format!("FAIL hang from_own_context status={status:?}"));
            return;
        }
    };
    mark("hang ready");
    let status = session.next_event(u64::MAX).err();
    mark(&format!("hang woke status={status:?}"));
}

fn writer(context: &ChildContext, argv: &[String]) {
    let Some(out) = granted(context, GrantSlot::Stdout) else {
        mark("FAIL writer no stdout grant");
        return;
    };
    let delay: u64 = argv.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
    let tag = argv.get(2).map(String::as_str).unwrap_or("w");
    sleep_ms(delay);
    match out.write(tag.as_bytes(), 0) {
        Ok(n) => mark(&format!("writer {tag} wrote={n}")),
        Err(status) => mark(&format!("writer {tag} status={status:?}")),
    }
}

/// Create a child-owned descendant that retains a delegated writer after this
/// child has exited.
fn descend(context: &ChildContext, argv: &[String]) {
    let Some(out) = granted(context, GrantSlot::Stdout) else {
        mark("FAIL descend no stdout grant");
        return;
    };
    let delay = argv.get(1).map(String::as_str).unwrap_or("300");
    let parent = match OperationRef::from_raw(context.operation()) {
        Ok(parent) => parent,
        Err(status) => {
            mark(&format!("descend bad operation status={status:?}"));
            return;
        }
    };
    match spawn_writer(out, delay, "gc", parent) {
        Ok(()) => {
            mark("descend grandchild committed");
            // Drop this child's own writer so only the descendant retains one,
            // then stay alive: a creator's death drains the operations it owns.
            let _ = out.close();
            sleep_ms(2500);
        }
        Err(status) => mark(&format!("descend spawn status={status:?}")),
    }
}

fn spawn_writer(out: Endpoint, delay: &str, tag: &str, parent: OperationRef) -> Result<(), Status> {
    let operation = Operation::create(Some(parent))?;
    let argv = ["writer", delay, tag];
    // Length is exact because the dummy grant has the same encoded size.
    let dummy = Grant::endpoint(
        GrantSlot::Stdout,
        ObjectRefV1 { id: 1, generation: 1 },
        30,
    )?;
    let probe = ChildContext::new(operation.reference().raw(), &argv, b"", &[], &[dummy])?;
    let mut prepared = operation.prepare_child(CHILD_PATH, probe.as_bytes().len())?;
    let delegated = out.delegate(&prepared, EndpointRights::WRITE_DELEGATE)?;
    let grant = Grant::endpoint(GrantSlot::Stdout, delegated.reference().raw(), 30)?;
    let context = ChildContext::new(operation.reference().raw(), &argv, b"", &[], &[grant])?;
    prepared.write_context(&context)?;
    prepared.commit()?;
    Ok(())
}

/// Fill the ring, then block in a write with no deadline until the consumer
/// closes or the operation is cancelled.
fn blocked_writer(context: &ChildContext) {
    let Some(out) = granted(context, GrantSlot::Stdout) else {
        mark("FAIL blocked-writer no stdout grant");
        return;
    };
    let block = [0x5au8; 512];
    let mut total = 0usize;
    mark("blocked-writer ready");
    loop {
        match out.write(&block, u64::MAX) {
            Ok(n) => total += n,
            Err(status) => {
                mark(&format!("blocked-writer woke status={status:?} total={total}"));
                return;
            }
        }
    }
}

fn blocked_reader(context: &ChildContext) {
    let Some(input) = granted(context, GrantSlot::Stdin) else {
        mark("FAIL blocked-reader no stdin grant");
        return;
    };
    let mut buffer = [0u8; 64];
    mark("blocked-reader ready");
    match input.read(&mut buffer, u64::MAX) {
        Ok(n) => mark(&format!("blocked-reader bytes={n}")),
        Err(status) => mark(&format!("blocked-reader woke status={status:?}")),
    }
}

/// Use references the parent holds privately; every use must be refused.
fn forged(argv: &[String]) {
    let operation = argv.get(1).and_then(|_| parse_ref(&argv[1], &argv[2]));
    let endpoint = argv.get(3).and_then(|_| parse_ref(&argv[3], &argv[4]));
    let (Some(operation), Some(endpoint)) = (operation, endpoint) else {
        mark("FAIL forged malformed argv");
        return;
    };
    let (Ok(operation), Ok(endpoint)) = (OperationRef::from_raw(operation), EndpointRef::from_raw(endpoint)) else {
        mark("FAIL forged refs");
        return;
    };
    let foreign = Endpoint::new(endpoint, operation);
    let mut buffer = [0u8; 8];
    let read = foreign.read(&mut buffer, 0);
    let write = foreign.write(b"x", 0);
    let duplicate = foreign.duplicate(EndpointRights::WRITE_ONLY);
    let close = foreign.close();
    let cancel = Operation::from_ref(operation).cancel(1);
    mark(&format!(
        "forged read_denied={} write_denied={} dup_denied={} close_denied={} cancel_denied={}",
        read.is_err(),
        write.is_err(),
        duplicate.is_err(),
        close.is_err(),
        cancel.is_err()
    ));
    mark(&format!("forged detail read={read:?} dup={:?}", duplicate.map(|_| ())));
}

/// Observe the death of the parent that held the only writer.
fn orphan(context: &ChildContext) {
    let Some(input) = granted(context, GrantSlot::Stdin) else {
        mark("FAIL orphan no stdin grant");
        return;
    };
    let mut buffer = [0u8; 16];
    mark("orphan ready");
    // Heartbeat until the owner reports a terminal condition. If the parent's
    // death retires this child outright, the heartbeat simply stops; the host
    // runner treats that cessation as the observation.
    for tick in 0..400u32 {
        match input.read(&mut buffer, now() + 100_000_000) {
            Ok(n) => mark(&format!("orphan bytes={n}")),
            Err(Status::TimedOut) => mark(&format!("orphan tick {tick}")),
            Err(status) => {
                mark(&format!("orphan woke after parent exit status={status:?}"));
                return;
            }
        }
    }
}
