// SPDX-License-Identifier: MPL-2.0

//! B0 witness: a supervisor tree declared by an application, not by `/bin/init`.
//!
//! The supervisor uses `ostd::actor::{Actor, ActorCtx}` and
//! `ostd::actor::supervisor::{Tree, ChildSpec, Policy, Strategy, Backoff}`
//! (ADR-0021) over one worker binary. The script is deterministic and
//! self-driving: the cell kills its own children and asserts what the library
//! did, so the integration test only has to launch it and read markers.
//!
//! Markers (integration-test contract):
//!   `[backend] supervisor up: children w0 w1 w2 path=/bin/backend-worker`
//!   `[backend] typed call to w0 ok`
//!   `[backend] exit observed child=w0 tid=<n> reason=0x<hex>`
//!   `[backend] restart-latency ticks=<n> bound=100`
//!   `[backend] restart-latency OK (<1s)`
//!   `[backend] give-up OK: w1 abandoned`
//!   `[backend] survivors OK: w0 and w2 answered after the give-up`
//!   `[backend] one-for-all OK: w3 <old> -> <new>, w4 <old> -> <new>`
//!   `ACTOR-SUPERVISOR: PASS`
//!
//! On any failed assertion it prints `[backend] FAIL: <reason>` and
//! `ACTOR-SUPERVISOR: FAIL` and exits non-zero.
//!
//! # What this witness does and does not cover
//!
//! Covered end to end: dynamic child table, typed call/reply between cells,
//! `Transient` restart on an abnormal exit, restart latency, per-child intensity
//! with give-up that leaves the other children running, and `one_for_all`
//! expansion over two children.
//!
//! Not covered here: `rest_for_one` and capped exponential backoff. Both are
//! pure decisions unit-tested on the host in `libs/ostd/src/actor/supervisor.rs`
//! (`strategy_scope_is_declaration_ordered`, `backoff_*`); `one_for_all` shares
//! the sibling-termination path they use.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use ostd::actor::{self, exit_reason, Actor, ActorCtx, Backoff, ChildSpec, Policy, Strategy, Tree};
use ostd::app::AppEvent;
use ostd::io::println;
use ostd::syscall;
use serde::{Deserialize, Serialize};

/// Supervisor → worker requests.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum WorkerMsg {
    Ping { seq: u32 },
    Exit { code: u32 },
}

/// Worker → supervisor replies.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum WorkerReply {
    Pong { seq: u32 },
}

/// Path every child is spawned from; the supervisor's only reviewed launch edge.
pub const WORKER_PATH: &str = "/bin/backend-worker";

/// B0 acceptance: a restarted worker is back within one second (100 ticks).
const LATENCY_BOUND_TICKS: u64 = 100;
/// Abnormal exits aimed at the storm child before the budget must be exhausted.
const STORM_KILLS: u32 = 6;
/// Give up on the script rather than hang the runner if a step never completes.
const SCRIPT_WATCHDOG_TICKS: u64 = 6_000;

// ── Worker ────────────────────────────────────────────────────────────────────

/// The smallest useful actor: answers pings, exits on request.
pub struct BackendWorker;

impl Actor for BackendWorker {
    type Msg = WorkerMsg;

    fn on_start(&mut self, _ctx: &mut ActorCtx<'_>) {
        println("[backend-worker] up");
    }

    fn on_message(&mut self, ctx: &mut ActorCtx<'_>, from: usize, msg: WorkerMsg) {
        match msg {
            WorkerMsg::Ping { seq } => {
                if ctx.reply(from, &WorkerReply::Pong { seq }).is_err() {
                    println(&format!(
                        "[backend-worker] reply to tid={from} failed (caller gone)"
                    ));
                }
            }
            WorkerMsg::Exit { code } => ctx.exit(code as usize),
        }
    }
}

/// Run the worker actor; never returns.
pub fn run_worker() -> ! {
    actor::run(BackendWorker)
}

// ── Supervisor ────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Step {
    Boot,
    PingW0,
    WaitW0Restart,
    StormKill,
    StormVerify,
    OneForAllStart,
    OneForAllKill,
    OneForAllVerify,
    Finished,
}

pub struct BackendSupervisor {
    /// Three workers, `one_for_one`: the shape the B0 acceptance describes.
    tree_a: Tree,
    /// Two workers, `one_for_all`: proves a second strategy end to end.
    tree_b: Option<Tree>,
    step: Step,
    next_at: u64,
    started_at: u64,
    kill_tick: Option<u64>,
    killed_tid: Option<usize>,
    storm_kills: u32,
    seq: u32,
    pair_before: Option<(usize, usize)>,
}

impl BackendSupervisor {
    pub fn new() -> Self {
        let tree_a = Tree::new(
            Strategy::OneForOne,
            [
                // The restarted child pays a real backoff before it comes back.
                ChildSpec::new("w0", WORKER_PATH)
                    .with_policy(Policy::Transient)
                    .with_backoff(Backoff {
                        base_ticks: 50,
                        cap_ticks: 200,
                    }),
                // The storm child has to burn six exits inside one 1_000-tick
                // window, so its backoff stays short and constant.
                ChildSpec::new("w1", WORKER_PATH)
                    .with_policy(Policy::Transient)
                    .with_backoff(Backoff {
                        base_ticks: 10,
                        cap_ticks: 10,
                    }),
                ChildSpec::new("w2", WORKER_PATH)
                    .with_policy(Policy::Transient)
                    .with_backoff(Backoff {
                        base_ticks: 50,
                        cap_ticks: 200,
                    }),
            ],
        );
        Self {
            tree_a,
            tree_b: None,
            step: Step::Boot,
            next_at: 0,
            started_at: 0,
            kill_tick: None,
            killed_tid: None,
            storm_kills: 0,
            seq: 0,
            pair_before: None,
        }
    }

    fn fail(&mut self, ctx: &mut ActorCtx<'_>, why: &str) -> ! {
        println(&format!("[backend] FAIL: {why}"));
        println("ACTOR-SUPERVISOR: FAIL");
        ctx.exit(1)
    }

    /// Typed call with a bounded retry: a ping can lose a race with a pending
    /// child-exit notification, which is not a liveness failure.
    fn ping(&mut self, ctx: &mut ActorCtx<'_>, tid: usize, seq: u32) -> Option<u32> {
        for attempt in 0..3u32 {
            match ctx.call::<WorkerMsg, WorkerReply>(tid, &WorkerMsg::Ping { seq }) {
                Ok(WorkerReply::Pong { seq: got }) => return Some(got),
                Err(e) => println(&format!(
                    "[backend] ping tid={tid} attempt={attempt} error={e:?}"
                )),
            }
            syscall::sys_yield();
        }
        None
    }

    fn ping_or_fail(&mut self, ctx: &mut ActorCtx<'_>, name: &str, seq: u32) {
        let Some(tid) = self.tree_a.tid_of(name) else {
            self.fail(ctx, "child has no live tid to ping");
        };
        match self.ping(ctx, tid, seq) {
            Some(got) if got == seq => println(&format!("[backend] typed call to {name} ok")),
            _ => self.fail(ctx, "typed call did not return the expected reply"),
        }
    }
}

impl Actor for BackendSupervisor {
    /// The supervisor drives its children with `ActorCtx::call`, whose reply is
    /// consumed by the masked receive inside that call. A typed message reaching
    /// this loop is therefore unexpected traffic and is reported, not swallowed.
    type Msg = WorkerMsg;

    fn on_start(&mut self, ctx: &mut ActorCtx<'_>) {
        self.started_at = ctx.now_ticks();
        self.tree_a.start_all(ctx);
        println(&format!(
            "[backend] supervisor up: children w0 w1 w2 path={WORKER_PATH}"
        ));
        self.step = Step::PingW0;
        self.next_at = self.started_at + 5;
    }

    fn on_message(&mut self, _ctx: &mut ActorCtx<'_>, from: usize, msg: WorkerMsg) {
        println(&format!(
            "[backend] WARNING unexpected typed message from tid={from}: {msg:?}"
        ));
    }

    fn on_event(&mut self, ctx: &mut ActorCtx<'_>, ev: AppEvent) {
        match ev {
            AppEvent::RawMessage { sender_tid, data } => {
                let Some(reason) = exit_reason(&data) else {
                    println(&format!(
                        "[backend] WARNING raw message from tid={sender_tid} is not an exit notification"
                    ));
                    return;
                };
                let index_a = self.tree_a.index_of_tid(sender_tid);
                let index_b = self
                    .tree_b
                    .as_ref()
                    .and_then(|tree| tree.index_of_tid(sender_tid));
                match (index_a, index_b) {
                    (Some(index), _) => {
                        let name = self.tree_a.child(index).map(|c| c.name()).unwrap_or("?");
                        println(&format!(
                            "[backend] exit observed child={name} tid={sender_tid} reason=0x{reason:x}"
                        ));
                        self.tree_a.handle_exit(ctx, sender_tid, reason);
                    }
                    (None, Some(index)) => {
                        let name = self
                            .tree_b
                            .as_ref()
                            .and_then(|tree| tree.child(index))
                            .map(|c| c.name())
                            .unwrap_or("?");
                        println(&format!(
                            "[backend] exit observed child={name} tid={sender_tid} reason=0x{reason:x}"
                        ));
                        if let Some(tree_b) = self.tree_b.as_mut() {
                            tree_b.handle_exit(ctx, sender_tid, reason);
                        }
                    }
                    (None, None) => println(&format!(
                        "[backend] WARNING exit notification from unwatched tid={sender_tid}"
                    )),
                }
            }
            AppEvent::CapRevoked { mask } => {
                println(&format!("[backend] WARNING capability revoked: 0x{mask:x}"))
            }
            other => println(&format!("[backend] unhandled event: {other:?}")),
        }
    }

    fn on_tick(&mut self, ctx: &mut ActorCtx<'_>) {
        let now = ctx.now_ticks();
        if now.saturating_sub(self.started_at) > SCRIPT_WATCHDOG_TICKS {
            self.fail(ctx, "script watchdog expired before PASS");
        }

        // Backoff timers are the library's job; ticking them is the actor's.
        self.tree_a.handle_tick(ctx, now);
        if let Some(tree_b) = self.tree_b.as_mut() {
            tree_b.handle_tick(ctx, now);
        }

        if now < self.next_at || self.step == Step::Finished {
            return;
        }

        match self.step {
            Step::Boot => {
                self.step = Step::PingW0;
                self.next_at = now;
            }

            Step::PingW0 => {
                self.seq += 1;
                let seq = self.seq;
                self.ping_or_fail(ctx, "w0", seq);
                let Some(tid) = self.tree_a.tid_of("w0") else {
                    self.fail(ctx, "w0 disappeared before the kill");
                };
                self.killed_tid = Some(tid);
                self.kill_tick = Some(now);
                match ctx.force_exit(tid) {
                    Ok(()) => println(&format!("[backend] killed w0 tid={tid}")),
                    Err(e) => {
                        println(&format!("[backend] ForceExit w0 tid={tid} failed: {e:?}"));
                        self.fail(ctx, "ForceExit on w0 failed");
                    }
                }
                self.step = Step::WaitW0Restart;
                self.next_at = now;
            }

            Step::WaitW0Restart => {
                let Some(dead) = self.killed_tid else {
                    self.fail(ctx, "no killed tid recorded");
                };
                match self.tree_a.tid_of("w0") {
                    Some(tid) if tid != dead => {
                        let elapsed = now.saturating_sub(self.kill_tick.unwrap_or(now));
                        println(&format!(
                            "[backend] restart-latency ticks={elapsed} bound={LATENCY_BOUND_TICKS}"
                        ));
                        if elapsed >= LATENCY_BOUND_TICKS {
                            self.fail(ctx, "restart latency exceeded the 1 s bound");
                        }
                        println("[backend] restart-latency OK (<1s)");
                        self.step = Step::StormKill;
                    }
                    _ => self.next_at = now + 1,
                }
            }

            Step::StormKill => {
                if self.storm_kills >= STORM_KILLS {
                    self.step = Step::StormVerify;
                    self.next_at = now + 1;
                    return;
                }
                let Some(tid) = self.tree_a.tid_of("w1") else {
                    // Waiting for the previous respawn to land.
                    self.next_at = now + 1;
                    return;
                };
                match ctx.force_exit(tid) {
                    Ok(()) => {
                        self.storm_kills += 1;
                        println(&format!(
                            "[backend] storm kill {}/{} on w1 tid={tid}",
                            self.storm_kills, STORM_KILLS
                        ));
                    }
                    Err(e) => {
                        println(&format!("[backend] ForceExit w1 tid={tid} failed: {e:?}"));
                        self.fail(ctx, "ForceExit on the storm child failed");
                    }
                }
                self.next_at = now + 2;
            }

            Step::StormVerify => {
                let Some(index) = self.tree_a.index_of_name("w1") else {
                    self.fail(ctx, "w1 is not declared");
                };
                let gave_up = self
                    .tree_a
                    .child(index)
                    .map(|child| child.gave_up())
                    .unwrap_or(false);
                if !gave_up {
                    self.next_at = now + 1;
                    return;
                }
                println(
                    "[backend] give-up OK: w1 abandoned after the restart budget was exhausted",
                );
                self.seq += 2;
                let seq = self.seq;
                self.ping_or_fail(ctx, "w0", seq);
                self.ping_or_fail(ctx, "w2", seq + 1);
                println("[backend] survivors OK: w0 and w2 answered after the give-up");
                self.step = Step::OneForAllStart;
                self.next_at = now + 2;
            }

            Step::OneForAllStart => {
                let mut tree_b = Tree::new(
                    Strategy::OneForAll,
                    [
                        ChildSpec::new("w3", WORKER_PATH).with_policy(Policy::Transient),
                        ChildSpec::new("w4", WORKER_PATH).with_policy(Policy::Transient),
                    ],
                );
                tree_b.start_all(ctx);
                match (tree_b.tid_of("w3"), tree_b.tid_of("w4")) {
                    (Some(three), Some(four)) => {
                        self.pair_before = Some((three, four));
                        println(&format!(
                            "[backend] one-for-all tree up: w3 tid={three} w4 tid={four}"
                        ));
                    }
                    _ => self.fail(ctx, "one_for_all children did not start"),
                }
                self.seq += 1;
                let seq = self.seq;
                let Some(tid) = tree_b.tid_of("w3") else {
                    self.fail(ctx, "w3 has no live tid");
                };
                self.tree_b = Some(tree_b);
                match self.ping(ctx, tid, seq) {
                    Some(got) if got == seq => println("[backend] typed call to w3 ok"),
                    _ => self.fail(ctx, "typed call to w3 did not return the expected reply"),
                }
                self.step = Step::OneForAllKill;
                self.next_at = now + 2;
            }

            Step::OneForAllKill => {
                let tid = self.tree_b.as_ref().and_then(|tree| tree.tid_of("w3"));
                let Some(tid) = tid else {
                    self.fail(ctx, "w3 disappeared before the one_for_all kill");
                };
                match ctx.force_exit(tid) {
                    Ok(()) => println(&format!("[backend] one-for-all: killed w3 tid={tid}")),
                    Err(e) => {
                        println(&format!("[backend] ForceExit w3 tid={tid} failed: {e:?}"));
                        self.fail(ctx, "ForceExit on w3 failed");
                    }
                }
                self.step = Step::OneForAllVerify;
                self.next_at = now + 1;
            }

            Step::OneForAllVerify => {
                let Some((old_three, old_four)) = self.pair_before else {
                    self.fail(ctx, "no recorded one_for_all tids");
                };
                let new_three = self.tree_b.as_ref().and_then(|tree| tree.tid_of("w3"));
                let new_four = self.tree_b.as_ref().and_then(|tree| tree.tid_of("w4"));
                match (new_three, new_four) {
                    (Some(three), Some(four)) if three != old_three && four != old_four => {
                        println(&format!(
                            "[backend] one-for-all OK: w3 {old_three} -> {three}, w4 {old_four} -> {four}"
                        ));
                        println("ACTOR-SUPERVISOR: PASS");
                        self.step = Step::Finished;
                        ctx.exit(0)
                    }
                    _ => self.next_at = now + 1,
                }
            }

            Step::Finished => {}
        }
    }
}

impl Default for BackendSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the supervisor actor; never returns.
pub fn run_supervisor() -> ! {
    actor::run(BackendSupervisor::new())
}
