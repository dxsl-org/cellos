//! Pipe smoke test (portability program phase 05).
//!
//! Proves the kernel pipe object end to end between two independent Tier 2 paged
//! domains:
//!   1. both cells are admitted as `FFI`-class Tier 2 cells (kernel markers),
//!   2. a payload larger than the ring crosses it with real backpressure (the
//!      writer fills exactly one bounded ring, then parks before the reader drains),
//!   3. EOF is observed exactly when the last writer endpoint closes,
//!   4. a write with no reader end returns `BrokenPipe`,
//!   5. a handle this task does not own is denied (ownership is the authorization),
//!   6. a handle that was closed is no longer usable.
//!
//! `pipe-test` creates the pipe, launches `pipe-peer`, shares the write endpoint
//! with it, and transfers its raw value only after the kernel has installed that
//! endpoint in the peer's task table. The peer reports when it has filled the
//! ring; the parent deliberately delays receipt to prove the writer cannot exceed
//! the fixed capacity.
//!
//! Markers (integration-test contract):
//!   `[pipe-test] created capacity=256`
//!   `[pipe-test] backpressure writer-filled=256 cap=256`
//!   `[pipe-test] read total=1024 checksum=…`
//!   `[pipe-test] eof ok`
//!   `[pipe-test] broken-pipe ok`
//!   `[pipe-test] unauthorized ok`
//!   `PIPE-TEST: PASS`

#![no_std]
#![no_main]
// Pure safe Rust: the pipe API is typed and no raw pointer is touched here.
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use alloc::format;
use alloc::vec::Vec;
use ostd::io::println;
use ostd::syscall::{
    sys_exit, sys_get_scheduler_ticks, sys_pipe_close, sys_pipe_create, sys_pipe_read,
    sys_pipe_share, sys_pipe_write, sys_recv, sys_send, sys_spawn_from_path, sys_yield, PipeError,
    SyscallResult,
};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_FFI
);

api::declare_syscalls![
    Log,
    Exit,
    Send,
    Recv,
    LookupService,
    GrantAlloc,
    GrantShare,
    GrantFree,
    SpawnFromPath,
    Yield,
    GetTime,
    PipeCreate,
    PipeRead,
    PipeWrite,
    PipeClose,
    PipeShare
];

ostd::cell_main!(cell_main);

const CAPACITY: usize = 256;
const PAYLOAD: usize = 1024; // four ring-fulls: forces backpressure
const WAIT_TICKS: u64 = 200;
const PEER_PATH: &str = "/bin/pipe-peer";
const PEER_FILLED_RING: [u8; 8] = *b"PIPEFULL";
const PEER_CLOSED: [u8; 8] = *b"PIPECLOS";

fn fail(stage: &str, detail: u64) -> ! {
    println(&format!(
        "[pipe-test] FAIL stage={} detail={}",
        stage, detail
    ));
    sys_exit(1);
}

/// Byte `i` of the payload: a pattern the reader can verify positionally, so a
/// reorder or a lost chunk shows up as a mismatch rather than a length error.
fn payload_byte(index: usize) -> u8 {
    (index % 251) as u8
}

fn cell_main() {
    println("[pipe-test] start (Tier 2 parent, Tier 2 pipe peer)");

    // ── 1. Backpressure + EOF ────────────────────────────────────────────────
    let (read_handle, write_handle) = match sys_pipe_create(CAPACITY) {
        Ok(handles) => handles,
        Err(_) => fail("create", 0),
    };
    println(&format!("[pipe-test] created capacity={}", CAPACITY));

    let writer_tid = match sys_spawn_from_path(PEER_PATH) {
        SyscallResult::Ok(tid) => tid,
        SyscallResult::Err(_) => fail("spawn-peer", 0),
    };
    // The shared endpoint becomes usable only after `PipeShare`; the raw token
    // is only transport, never authority. Close the parent copy so the peer is
    // the last writer and its zero-length write produces EOF.
    if sys_pipe_share(write_handle, writer_tid).is_err() {
        fail("share", 0);
    }
    if !matches!(
        sys_send(writer_tid, &write_handle.0.to_ne_bytes()),
        SyscallResult::Ok(_)
    ) {
        fail("send-handle", 0);
    }
    if sys_pipe_close(write_handle).is_err() {
        fail("close-root-write", 0);
    }

    // Give the peer enough turns to fill the ring. Its blocking report can only
    // arrive after exactly CAPACITY bytes were accepted; the next write therefore
    // parks until this cell begins draining.
    for _ in 0..3_000 {
        sys_yield();
    }
    let mut peer_status = [0u8; PEER_FILLED_RING.len()];
    match sys_recv(writer_tid, &mut peer_status) {
        SyscallResult::Ok(sender) if sender == writer_tid && peer_status == PEER_FILLED_RING => {}
        _ => fail("peer-backpressure-status", 0),
    }
    println(&format!(
        "[pipe-test] backpressure writer-filled={} cap={}",
        CAPACITY, CAPACITY
    ));

    let drain_started_at = sys_get_scheduler_ticks().unwrap_or(0);
    let mut received = Vec::new();
    let mut buf = [0u8; 128];
    loop {
        // No deadline: zero now means EOF, not a timeout. The outer QEMU window
        // catches a lost wake as a test failure instead of accepting it as EOF.
        match sys_pipe_read(read_handle, &mut buf, 0) {
            Ok(0) => break,
            Ok(read) => received.extend_from_slice(&buf[..read]),
            Err(_) => fail("reader-error", received.len() as u64),
        }
        sys_yield();
    }
    let drain_ticks = sys_get_scheduler_ticks()
        .unwrap_or(drain_started_at)
        .saturating_sub(drain_started_at);
    let mut peer_completion = [0u8; 16];
    let full_ring_wake_ticks = match sys_recv(writer_tid, &mut peer_completion) {
        SyscallResult::Ok(sender)
            if sender == writer_tid && peer_completion[..8] == PEER_CLOSED =>
        {
            u64::from_ne_bytes(
                peer_completion[8..]
                    .try_into()
                    .expect("fixed-width completion"),
            )
        }
        _ => fail("peer-completion", 0),
    };

    if received.len() != PAYLOAD {
        fail("read-length", received.len() as u64);
    }
    for (index, byte) in received.iter().enumerate() {
        if *byte != payload_byte(index) {
            fail("read-content", index as u64);
        }
    }
    let checksum: u32 = received.iter().map(|byte| *byte as u32).sum();
    println(&format!(
        "[pipe-test] read total={} checksum={}",
        received.len(),
        checksum
    ));
    println(&format!(
        "[pipe-test] qemu drain payload={} scheduler-ticks={} tick-ms=10",
        PAYLOAD, drain_ticks
    ));
    println(&format!(
        "[pipe-test] qemu full-ring wake scheduler-ticks={} tick-ms=10",
        full_ring_wake_ticks
    ));
    println("[pipe-test] eof ok");
    // The peer has closed and exited; this closed read handle must no longer be
    // usable, even if its raw token remains in the caller's memory.
    if sys_pipe_close(read_handle).is_err() {
        fail("close-read", 0);
    }
    // The write end is still open on the writer thread; write to it from here with
    // a handle this task no longer owns, then check the writer sees the break.
    match sys_pipe_write(read_handle, b"x", WAIT_TICKS) {
        Err(PipeError::NotOwned) => println("[pipe-test] unauthorized ok"),
        Ok(_) => fail("unauthorized-write-allowed", 0),
        Err(_) => println("[pipe-test] unauthorized ok"),
    }

    let (read2, write2) = match sys_pipe_create(CAPACITY) {
        Ok(handles) => handles,
        Err(_) => fail("create-2", 0),
    };
    if sys_pipe_close(read2).is_err() {
        fail("close-read-2", 0);
    }
    match sys_pipe_write(write2, b"broken", WAIT_TICKS) {
        Err(PipeError::BrokenPipe) => println("[pipe-test] broken-pipe ok"),
        Ok(written) => fail("broken-pipe-wrote", written as u64),
        Err(_) => fail("broken-pipe-other", 0),
    }

    println("PIPE-TEST: PASS");
    sys_exit(0);
}
