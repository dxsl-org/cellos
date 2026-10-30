//! Private Phase 06 S6-5 load fixture (`phase-06-s5-proposal.md` §3). Packaged only by
//! the private terminal runner, at the sandbox path `/data/bin/top-load` (EMPTY launch
//! ceiling, DV-13); never part of an ordinary image.
//!
//! `top-load W` (1 ≤ W ≤ 31) spawns W worker threads of this Cell, waits until every one
//! is parked and prints `TOPLOAD ready workers=W tids=t1,…,tW`. It then reads one command
//! per typed line from its session (canonical typed events: text, then Enter) and answers
//! each with exactly one line on its delegated stdout, printed after the effect holds:
//!
//! | Command   | Effect                                              | Acknowledgement                     |
//! |-----------|-----------------------------------------------------|-------------------------------------|
//! | `busy K`  | the first K live workers spin, the rest park        | `TOPLOAD busy tids=…` (spinners)    |
//! | `idle`    | every worker parks                                  | `TOPLOAD idle`                      |
//! | `exit K`  | the first K live workers return and are joined      | `TOPLOAD exited tids=…`             |
//! | `ping`    | none                                                | `TOPLOAD pong live=L`               |
//! | `bench N` | N timed `GetTime(0)`, then N timed `Yield`          | `TOPLOAD bench clock_ns=… yield_ns=…` (per call) |
//! | `quit`    | the root exits, the Cell retires (also end of input) | `TOPLOAD bye`                       |
//!
//! Anything else (or a count out of range) prints `TOPLOAD error` and changes nothing; a
//! state that does not settle within [`SETTLE_NS`] prints `TOPLOAD error timeout`.
//!
//! A spinner runs a pure arithmetic loop (no syscall) and re-reads its command word every
//! [`SPIN_CHUNK`] iterations; a parked worker sleeps in `SetTimer(PARK_TICKS)` (about
//! 100 ms) between reads, so a park costs about no CPU. Workers run at the default
//! priority (the CPU-monopoly watchdog only applies to RealTime tasks).
//!
//! Exit statuses: 0 `quit` or end of input; 2 usage; 3 session, stream or spawn failure;
//! 130 cancelled (Ctrl-C).

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

mod command;

use api::cell_session::Status;
use command::{parse, parse_workers, per_call_ns, Command, InputLine, OutputLine, Tids, MAX_WORKERS};
use core::fmt::Write;
use core::sync::atomic::{AtomicU8, Ordering};
use ostd::child_context::ChildContext;
use ostd::syscall::{sys_exit, sys_get_time, sys_get_timer_freq, sys_set_timer, sys_wait, sys_yield, SyscallResult};
use terminal_sdk::input::EventKind;
use terminal_sdk::session::TerminalSession;

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
    Spawn,
    Wait,
];

ostd::cell_main!(cell_main);

const OK: i32 = 0;
const USAGE: i32 = 2;
const FAILURE: i32 = 3;
const CANCELLED: i32 = 130;
/// No deadline: block until the reader takes the bytes, closes or cancels.
const FOREVER: u64 = u64::MAX;
/// The Enter key symbol of a typed key event.
const ENTER: u32 = api::session::key_code::ENTER;
/// Parked workers re-read their command word every this many scheduler ticks.
const PARK_TICKS: usize = 10;
/// Spin iterations between two reads of the command word.
const SPIN_CHUNK: u32 = 4096;
/// Longest wait for every live worker to report the commanded state.
const SETTLE_NS: u64 = 30_000_000_000;

const PARK: u8 = 0;
const SPIN: u8 = 1;
const EXIT: u8 = 2;
/// Not yet reported (a worker that has not run).
const UNSEEN: u8 = 255;

#[allow(clippy::declare_interior_mutable_const)]
const PARKED: AtomicU8 = AtomicU8::new(PARK);
#[allow(clippy::declare_interior_mutable_const)]
const NOT_SEEN: AtomicU8 = AtomicU8::new(UNSEEN);
/// The root's command to each worker.
static COMMAND: [AtomicU8; MAX_WORKERS] = [PARKED; MAX_WORKERS];
/// The state each worker last acted on.
static SEEN: [AtomicU8; MAX_WORKERS] = [NOT_SEEN; MAX_WORKERS];

fn worker(index: usize) {
    let mut value = index as u64 + 1;
    loop {
        match COMMAND[index].load(Ordering::Acquire) {
            SPIN => {
                SEEN[index].store(SPIN, Ordering::Release);
                while COMMAND[index].load(Ordering::Acquire) == SPIN {
                    for _ in 0..SPIN_CHUNK {
                        value = value.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                    }
                    value = core::hint::black_box(value);
                }
            }
            EXIT => {
                SEEN[index].store(EXIT, Ordering::Release);
                return;
            }
            _ => {
                SEEN[index].store(PARK, Ordering::Release);
                let _ = sys_set_timer(PARK_TICKS);
            }
        }
    }
}

fn now_ns() -> u64 {
    ostd::session::monotonic_ns().unwrap_or(0)
}

/// The workers of this Cell: their TIDs and which have not exited yet.
struct Workers {
    tids: [usize; MAX_WORKERS],
    live: [bool; MAX_WORKERS],
    count: usize,
}

impl Workers {
    fn spawn(count: usize) -> Result<Workers, ()> {
        let mut workers = Workers { tids: [0; MAX_WORKERS], live: [false; MAX_WORKERS], count };
        for index in 0..count {
            match ostd::task::spawn(move || worker(index)) {
                SyscallResult::Ok(tid) => {
                    workers.tids[index] = tid;
                    workers.live[index] = true;
                }
                SyscallResult::Err(_) => return Err(()),
            }
        }
        Ok(workers)
    }

    fn live(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.count).filter(|&index| self.live[index])
    }

    fn live_count(&self) -> usize {
        self.live().count()
    }

    /// Wait until every live worker reports the state it was commanded.
    fn settled(&self) -> bool {
        let end = now_ns().saturating_add(SETTLE_NS);
        loop {
            if self.live().all(|i| SEEN[i].load(Ordering::Acquire) == COMMAND[i].load(Ordering::Acquire)) {
                return true;
            }
            if now_ns() >= end {
                return false;
            }
            let _ = sys_set_timer(1);
        }
    }

    /// The first `spinners` live workers spin, the others park; their TIDs.
    fn set_busy(&self, spinners: usize, out: &mut [usize; MAX_WORKERS]) -> usize {
        let mut chosen = 0;
        for index in self.live() {
            let spin = chosen < spinners;
            if spin {
                out[chosen] = self.tids[index];
                chosen += 1;
            }
            COMMAND[index].store(if spin { SPIN } else { PARK }, Ordering::Release);
        }
        chosen
    }

    /// The first `count` live workers return; each is joined (`Wait`) so it has left the
    /// live task table when this returns. A worker reaped before the join is gone too.
    fn exit(&mut self, count: usize, out: &mut [usize; MAX_WORKERS]) -> usize {
        let chosen: usize = {
            let mut chosen = 0;
            for index in self.live() {
                if chosen == count {
                    break;
                }
                COMMAND[index].store(EXIT, Ordering::Release);
                out[chosen] = index;
                chosen += 1;
            }
            chosen
        };
        for slot in out.iter_mut().take(chosen) {
            let index = *slot;
            let _ = sys_wait(self.tids[index]);
            self.live[index] = false;
            *slot = self.tids[index];
        }
        chosen
    }
}

/// Write every byte of `data` to the session's stdout.
fn write_all(session: &mut TerminalSession, mut data: &[u8]) -> Result<(), i32> {
    while !data.is_empty() {
        match session.write_stdout(data, FOREVER) {
            Ok(count) if count > 0 && count <= data.len() => data = &data[count..],
            Ok(_) => return Err(FAILURE),
            Err(status) => return Err(failure(status)),
        }
    }
    Ok(())
}

fn failure(status: Status) -> i32 {
    match status {
        Status::Cancelled | Status::Revoked => CANCELLED,
        _ => FAILURE,
    }
}

fn say(session: &mut TerminalSession, args: core::fmt::Arguments<'_>) -> Result<(), i32> {
    let mut line = OutputLine::new();
    line.write_fmt(args).map_err(|_| FAILURE)?;
    line.write_str("\n").map_err(|_| FAILURE)?;
    write_all(session, line.as_bytes())
}

/// Per-call cost of `calls` `GetTime(0)` and `calls` `Yield`, in ns.
fn bench(calls: u64) -> (Option<u64>, Option<u64>) {
    let frequency = sys_get_timer_freq().unwrap_or(0);
    let start = sys_get_time();
    for _ in 0..calls {
        core::hint::black_box(sys_get_time());
    }
    let clock = sys_get_time().saturating_sub(start);
    let start = sys_get_time();
    for _ in 0..calls {
        sys_yield();
    }
    let yields = sys_get_time().saturating_sub(start);
    (per_call_ns(clock, frequency, calls), per_call_ns(yields, frequency, calls))
}

/// Run one command; `Ok(true)` ends the fixture.
fn execute(session: &mut TerminalSession, workers: &mut Workers, command: Command) -> Result<bool, i32> {
    let mut tids = [0usize; MAX_WORKERS];
    match command {
        Command::Busy(spinners) => {
            let chosen = workers.set_busy(spinners, &mut tids);
            if workers.settled() {
                say(session, format_args!("TOPLOAD busy tids={}", Tids(&tids[..chosen])))?;
            } else {
                say(session, format_args!("TOPLOAD error timeout"))?;
            }
        }
        Command::Idle => {
            workers.set_busy(0, &mut tids);
            if workers.settled() {
                say(session, format_args!("TOPLOAD idle"))?;
            } else {
                say(session, format_args!("TOPLOAD error timeout"))?;
            }
        }
        Command::Exit(count) => {
            let chosen = workers.exit(count, &mut tids);
            say(session, format_args!("TOPLOAD exited tids={}", Tids(&tids[..chosen])))?;
        }
        Command::Ping => say(session, format_args!("TOPLOAD pong live={}", workers.live_count()))?,
        Command::Bench(calls) => {
            let (clock, yields) = bench(calls);
            match (clock, yields) {
                (Some(clock), Some(yields)) => {
                    say(session, format_args!("TOPLOAD bench clock_ns={clock} yield_ns={yields}"))?
                }
                _ => say(session, format_args!("TOPLOAD error clock"))?,
            }
        }
        Command::Quit => {
            say(session, format_args!("TOPLOAD bye"))?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn run() -> i32 {
    let Ok(context) = ChildContext::read_own() else {
        return USAGE;
    };
    let Ok(mut session) = TerminalSession::from_own_context(&context) else {
        return FAILURE;
    };
    let Some(count) = context.argv().next().and_then(parse_workers) else {
        let _ = session.write_stderr(b"usage: top-load WORKERS (1-31)\n", FOREVER);
        return USAGE;
    };
    let Ok(mut workers) = Workers::spawn(count) else {
        let _ = say(&mut session, format_args!("TOPLOAD error spawn"));
        return FAILURE;
    };
    if !workers.settled() {
        let _ = say(&mut session, format_args!("TOPLOAD error timeout"));
        return FAILURE;
    }
    if let Err(code) = say(&mut session, format_args!("TOPLOAD ready workers={count} tids={}", Tids(&workers.tids[..count]))) {
        return code;
    }
    let mut line = InputLine::new();
    loop {
        let event = match session.next_event(FOREVER) {
            Ok(event) => event,
            Err(status) => return failure(status),
        };
        let command = match &event.kind {
            EventKind::Text(text) => {
                line.push(text);
                continue;
            }
            EventKind::Key { code: ENTER, modifiers: 0, pressed: 1 | 2 } => line.take().and_then(|l| parse(l, workers.live_count())),
            EventKind::Eof => Some(Command::Quit),
            EventKind::Interrupt => return CANCELLED,
            _ => continue,
        };
        let done = match command {
            Some(command) => execute(&mut session, &mut workers, command),
            None => say(&mut session, format_args!("TOPLOAD error")).map(|()| false),
        };
        match done {
            Ok(true) => return OK,
            Ok(false) => {}
            Err(code) => return code,
        }
    }
}

fn cell_main() {
    sys_exit(run() as u32 as usize);
}
