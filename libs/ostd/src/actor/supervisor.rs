// SPDX-License-Identifier: MPL-2.0

//! Supervisor tree: restart policy, intensity, backoff, and strategies.
//!
//! The semantics mirror the ones `/bin/init` has proved since 2026-06-06
//! (`docs/specs/12-reliability.md` §4.3): a child is restarted according to its
//! policy, restart frequency is bounded per child inside a time window, and
//! exceeding the bound makes the supervisor **give up on that child only** — it
//! keeps supervising every other child and logs the reason loudly.
//!
//! What the library adds over `init` is a **dynamic** child table (the
//! application declares its own children), a per-child **backoff** before a
//! respawn, and **strategies** beyond `one_for_one`.
//!
//! This module holds the decision logic and the syscall-driving tree; the pure
//! decisions ([`Child::record_exit`], [`Backoff::delay_ticks`], [`Tree::scope`])
//! are unit-tested on the host, which is why they take `now` as a parameter
//! instead of reading the clock themselves.

use alloc::vec;
use alloc::vec::Vec;

use super::log;
use super::ActorCtx;

/// Restarts allowed per window before the supervisor gives up on a child.
/// Matches `init`'s `MAX_RESTARTS_PER_WINDOW` (`cells/tools/init/src/supervisor.rs`).
pub const DEFAULT_INTENSITY: u32 = 5;

/// Default intensity window in scheduler ticks — 1_000 ticks ≈ 10 s at 10 ms/tick,
/// the window `init` uses.
pub const DEFAULT_WINDOW_TICKS: u64 = 1_000;

/// What a supervisor does when a child dies with a given exit reason.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Always restart, whatever the exit reason.
    Permanent,
    /// Restart only after an abnormal exit (a non-zero reason: fault, `ForceExit`,
    /// watchdog). A clean `exit(0)` leaves the child down.
    Transient,
    /// Never restart.
    Temporary,
}

impl Policy {
    /// Whether an exit with `reason` should be restarted under this policy.
    pub const fn restarts(self, reason: u64) -> bool {
        match self {
            Policy::Permanent => true,
            Policy::Transient => reason != 0,
            Policy::Temporary => false,
        }
    }
}

/// Which children a single failure restarts.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Only the child that died.
    OneForOne,
    /// The child that died and every other child (siblings are terminated first).
    OneForAll,
    /// The child that died and every child declared *after* it.
    RestForOne,
}

/// Capped exponential backoff before a respawn.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Backoff {
    /// Delay after the first consecutive failure.
    pub base_ticks: u64,
    /// Upper bound on the computed delay (`0` means no cap beyond `base_ticks`).
    pub cap_ticks: u64,
}

impl Backoff {
    /// Respawn immediately (no delay).
    pub const NONE: Backoff = Backoff {
        base_ticks: 0,
        cap_ticks: 0,
    };

    /// `base * 2^(streak-1)`, saturating at `cap_ticks` when one is set.
    ///
    /// `streak` is 1 for the first consecutive failure.
    pub const fn delay_ticks(&self, streak: u32) -> u64 {
        if self.base_ticks == 0 {
            return 0;
        }
        // Capped exponential: base << (streak - 1), with the shift clamped so the
        // computation stays const-evaluable (core::cmp::min is not const-stable
        // on this toolchain).
        let shift = if streak <= 1 {
            0
        } else if streak - 1 > 32 {
            32
        } else {
            streak - 1
        };
        let scaled = self.base_ticks.saturating_mul(1u64 << shift);
        if self.cap_ticks != 0 && scaled > self.cap_ticks {
            self.cap_ticks
        } else {
            scaled
        }
    }
}

/// Declaration of one supervised child.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChildSpec {
    /// Supervisor-local identifier used in logs and lookups.
    pub name: &'static str,
    /// Cell path handed to `SpawnFromPath`.
    pub path: &'static str,
    pub policy: Policy,
    pub intensity: u32,
    pub window_ticks: u64,
    pub backoff: Backoff,
}

impl ChildSpec {
    /// A `Permanent` child with default intensity and no backoff.
    pub const fn new(name: &'static str, path: &'static str) -> Self {
        Self {
            name,
            path,
            policy: Policy::Permanent,
            intensity: DEFAULT_INTENSITY,
            window_ticks: DEFAULT_WINDOW_TICKS,
            backoff: Backoff::NONE,
        }
    }

    pub const fn with_policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }

    pub const fn with_intensity(mut self, intensity: u32, window_ticks: u64) -> Self {
        self.intensity = intensity;
        self.window_ticks = window_ticks;
        self
    }

    pub const fn with_backoff(mut self, backoff: Backoff) -> Self {
        self.backoff = backoff;
        self
    }
}

/// Outcome of one observed child exit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Respawn after `delay_ticks` (0 = immediately).
    Restart { delay_ticks: u64 },
    /// Restart budget exhausted: this child is given up, others keep running.
    GiveUp,
    /// Policy says stay down (e.g. `Transient` after a clean exit).
    LeaveDown,
}

/// One supervised child's state.
#[derive(Debug)]
pub struct Child {
    spec: ChildSpec,
    tid: Option<usize>,
    restarts_in_window: u32,
    window_start: u64,
    streak: u32,
    gave_up: bool,
    restart_at: Option<u64>,
    /// Set when the supervisor itself is terminating this child as part of a
    /// strategy expansion, so its exit is not counted as a fresh failure.
    terminated_by_scope: bool,
}

impl Child {
    fn new(spec: ChildSpec) -> Self {
        Self {
            spec,
            tid: None,
            restarts_in_window: 0,
            window_start: 0,
            streak: 0,
            gave_up: false,
            restart_at: None,
            terminated_by_scope: false,
        }
    }

    pub fn spec(&self) -> ChildSpec {
        self.spec
    }

    pub fn name(&self) -> &'static str {
        self.spec.name
    }

    pub fn path(&self) -> &'static str {
        self.spec.path
    }

    pub fn tid(&self) -> Option<usize> {
        self.tid
    }

    pub fn is_live(&self) -> bool {
        self.tid.is_some()
    }

    pub fn gave_up(&self) -> bool {
        self.gave_up
    }

    pub fn restarts_in_window(&self) -> u32 {
        self.restarts_in_window
    }

    /// Record an observed exit and decide what to do about it.
    ///
    /// `now` is the supervisor's monotonic tick reading.
    pub fn record_exit(&mut self, reason: u64, now: u64) -> Decision {
        self.tid = None;
        self.restart_at = None;
        self.terminated_by_scope = false;

        if !self.spec.policy.restarts(reason) {
            self.streak = 0;
            return Decision::LeaveDown;
        }

        if now.saturating_sub(self.window_start) > self.spec.window_ticks {
            self.window_start = now;
            self.restarts_in_window = 0;
            self.streak = 0;
        }

        // Same order as `init`: the budget is checked *before* the increment, so a
        // default intensity of 5 allows five restarts and gives up on the sixth
        // abnormal exit inside the window.
        if self.restarts_in_window >= self.spec.intensity {
            self.gave_up = true;
            return Decision::GiveUp;
        }
        self.restarts_in_window += 1;

        self.streak += 1;
        let delay_ticks = self.spec.backoff.delay_ticks(self.streak);
        if delay_ticks > 0 {
            self.restart_at = Some(now.saturating_add(delay_ticks));
        }
        Decision::Restart { delay_ticks }
    }

    /// Whether a pending backoff has elapsed. Clears the pending timer.
    pub fn take_due(&mut self, now: u64) -> bool {
        match self.restart_at {
            Some(at) if at <= now => {
                self.restart_at = None;
                true
            }
            _ => false,
        }
    }

    /// True while a backoff timer is outstanding.
    pub fn is_pending(&self) -> bool {
        self.restart_at.is_some()
    }

    /// Flag a death the supervisor caused itself (strategy expansion).
    pub fn mark_scope_terminated(&mut self) {
        self.terminated_by_scope = true;
    }

    /// Clear a scope-termination flag, returning whether it was set.
    pub fn take_scope_terminated(&mut self) -> bool {
        let was = self.terminated_by_scope;
        self.terminated_by_scope = false;
        was
    }

    fn set_tid(&mut self, tid: usize) {
        self.tid = Some(tid);
        self.restart_at = None;
        self.terminated_by_scope = false;
    }
}

/// A supervisor tree: one strategy over a dynamic list of children.
pub struct Tree {
    strategy: Strategy,
    children: Vec<Child>,
}

impl Tree {
    /// Build a tree from declarations in **declaration order** (the order
    /// `one_for_all`/`rest_for_one` use).
    pub fn new(strategy: Strategy, specs: impl IntoIterator<Item = ChildSpec>) -> Self {
        Self {
            strategy,
            children: specs.into_iter().map(Child::new).collect(),
        }
    }

    pub fn strategy(&self) -> Strategy {
        self.strategy
    }

    pub fn children(&self) -> &[Child] {
        &self.children
    }

    pub fn child(&self, index: usize) -> Option<&Child> {
        self.children.get(index)
    }

    /// Index of the child currently owning `tid`.
    pub fn index_of_tid(&self, tid: usize) -> Option<usize> {
        self.children.iter().position(|c| c.tid == Some(tid))
    }

    /// Index of the child declared with `name`.
    pub fn index_of_name(&self, name: &str) -> Option<usize> {
        self.children.iter().position(|c| c.name() == name)
    }

    pub fn tid_of(&self, name: &str) -> Option<usize> {
        self.index_of_name(name)
            .and_then(|index| self.children[index].tid())
    }

    /// Children a failure at `index` restarts, in declaration order.
    ///
    /// Pure: the strategy expansion is unit-tested on the host.
    pub fn scope(&self, index: usize) -> Vec<usize> {
        if index >= self.children.len() {
            return Vec::new();
        }
        match self.strategy {
            Strategy::OneForOne => vec![index],
            Strategy::OneForAll => (0..self.children.len()).collect(),
            Strategy::RestForOne => (index..self.children.len()).collect(),
        }
    }

    /// Spawn and watch every declared child that has no live process yet.
    ///
    /// Called once at supervisor start and again after a scope expansion.
    pub fn start_all(&mut self, ctx: &mut ActorCtx) {
        for index in 0..self.children.len() {
            if !self.children[index].is_live() {
                self.respawn(ctx, index, "start");
            }
        }
    }

    /// Spawn + watch the child at `index`, logging what it became.
    pub fn respawn(&mut self, ctx: &mut ActorCtx, index: usize, why: &str) {
        let path = self.children[index].path();
        let name = self.children[index].name();
        match ctx.spawn(path) {
            Ok(tid) => match ctx.watch(tid) {
                Ok(()) => {
                    self.children[index].set_tid(tid);
                    log!("[supervisor] {why}: child {name} up tid={tid} policy={:?}", self.children[index].spec().policy);
                }
                Err(e) => {
                    log!("[supervisor] child {name} spawned tid={tid} but NotifyOnExit failed: {e:?}");
                    self.children[index].set_tid(tid);
                }
            },
            Err(e) => {
                log!("[supervisor] child {name} spawn of {path} FAILED: {e:?}");
            }
        }
    }

    /// Handle a possible exit notification.
    ///
    /// Returns `true` when `sender_tid` belongs to this tree (so the caller
    /// knows the message was a child death, not application traffic).
    pub fn handle_exit(&mut self, ctx: &mut ActorCtx, sender_tid: usize, reason: u64) -> bool {
        let Some(index) = self.index_of_tid(sender_tid) else {
            return false;
        };

        // A death this tree caused itself (strategy expansion) is not a failure.
        if self.children[index].take_scope_terminated() {
            log!(
                "[supervisor] child {} terminated by scope, respawning",
                self.children[index].name()
            );
            self.respawn(ctx, index, "scope-restart");
            return true;
        }

        let now = ctx.now_ticks();
        let name = self.children[index].name();
        let policy = self.children[index].spec().policy;
        let decision = self.children[index].record_exit(reason, now);

        match decision {
            Decision::LeaveDown => {
                log!("[supervisor] child {name} exited reason=0x{reason:x} policy={policy:?} — left down");
            }
            Decision::GiveUp => {
                log!(
                    "[supervisor] restart storm on child {name}: {} restarts in window — GIVING UP on {name} (other children keep running)",
                    self.children[index].restarts_in_window()
                );
            }
            Decision::Restart { delay_ticks } => {
                log!(
                    "[supervisor] child {name} exited reason=0x{reason:x} policy={policy:?} → restart in {delay_ticks} ticks"
                );
                let scope = self.scope(index);
                for member in scope {
                    if member == index {
                        if delay_ticks == 0 {
                            self.respawn(ctx, member, "restart");
                        }
                        continue;
                    }
                    // Sibling inside the scope: terminate it (if live) so it comes
                    // back with the failed child, in declaration order.
                    if let Some(tid) = self.children[member].tid() {
                        self.children[member].mark_scope_terminated();
                        if let Err(e) = ctx.force_exit(tid) {
                            log!(
                                "[supervisor] scope: ForceExit of child {} (tid={tid}) failed: {e:?}",
                                self.children[member].name()
                            );
                            self.children[member].take_scope_terminated();
                        }
                    } else {
                        self.respawn(ctx, member, "scope-restart");
                    }
                }
            }
        }
        true
    }

    /// Fire any backoff timers that have elapsed.
    pub fn handle_tick(&mut self, ctx: &mut ActorCtx, now: u64) {
        for index in 0..self.children.len() {
            if self.children[index].take_due(now) {
                self.respawn(ctx, index, "backoff-elapsed");
            }
        }
    }

    /// Whether any child has an outstanding backoff timer.
    pub fn has_pending(&self) -> bool {
        self.children.iter().any(Child::is_pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &'static str) -> ChildSpec {
        ChildSpec::new(name, "/bin/backend-worker")
    }

    fn tree(strategy: Strategy, names: &[&'static str]) -> Tree {
        Tree::new(strategy, names.iter().map(|n| spec(n)))
    }

    #[test]
    fn policy_matrix_follows_spec_12_4_3() {
        assert!(Policy::Permanent.restarts(0));
        assert!(Policy::Permanent.restarts(u64::MAX));
        assert!(!Policy::Transient.restarts(0), "clean exit stays down");
        assert!(Policy::Transient.restarts(1), "abnormal exit restarts");
        assert!(Policy::Transient.restarts(u64::MAX), "fault/ForceExit restarts");
        assert!(!Policy::Temporary.restarts(u64::MAX));
    }

    #[test]
    fn abnormal_exit_restarts_transient_child() {
        let mut child = Child::new(spec("w").with_policy(Policy::Transient));
        child.set_tid(11);
        assert_eq!(
            child.record_exit(u64::MAX, 100),
            Decision::Restart { delay_ticks: 0 }
        );
        assert!(!child.is_live(), "a dead child has no tid until it is respawned");
    }

    #[test]
    fn clean_exit_leaves_transient_child_down() {
        let mut child = Child::new(spec("w").with_policy(Policy::Transient));
        child.set_tid(11);
        assert_eq!(child.record_exit(0, 100), Decision::LeaveDown);
        assert_eq!(child.restarts_in_window(), 0);
    }

    #[test]
    fn intensity_gives_up_on_the_sixth_abnormal_exit_inside_one_window() {
        let mut child = Child::new(spec("w"));
        // Same window for all six: the window is 1_000 ticks and `now` stays inside it.
        let decisions: Vec<Decision> = (0..6).map(|_| child.record_exit(u64::MAX, 10)).collect();
        for (i, decision) in decisions.iter().enumerate().take(5) {
            assert_eq!(*decision, Decision::Restart { delay_ticks: 0 }, "restart #{i}");
        }
        assert_eq!(decisions[5], Decision::GiveUp);
        assert!(child.gave_up());
    }

    #[test]
    fn window_rollover_resets_the_restart_budget() {
        let mut child = Child::new(spec("w"));
        for _ in 0..5 {
            assert!(matches!(
                child.record_exit(u64::MAX, 10),
                Decision::Restart { .. }
            ));
        }
        assert_eq!(child.record_exit(u64::MAX, 10), Decision::GiveUp);
        // A fresh window clears the counter *and* the give-up is not re-armed
        // silently: the child was already declared given-up.
        let mut fresh = Child::new(spec("w"));
        for _ in 0..5 {
            fresh.record_exit(u64::MAX, 10);
        }
        assert_eq!(
            fresh.record_exit(u64::MAX, 10 + DEFAULT_WINDOW_TICKS + 1),
            Decision::Restart { delay_ticks: 0 },
            "exits outside the window start a new budget"
        );
    }

    #[test]
    fn backoff_grows_exponentially_and_saturates_at_the_cap() {
        let backoff = Backoff {
            base_ticks: 10,
            cap_ticks: 40,
        };
        assert_eq!(backoff.delay_ticks(0), 10);
        assert_eq!(backoff.delay_ticks(1), 10);
        assert_eq!(backoff.delay_ticks(2), 20);
        assert_eq!(backoff.delay_ticks(3), 40);
        assert_eq!(backoff.delay_ticks(9), 40, "capped");
        assert_eq!(Backoff::NONE.delay_ticks(7), 0);
    }

    #[test]
    fn backoff_defers_the_respawn_until_the_timer_elapses() {
        let backoff = Backoff {
            base_ticks: 100,
            cap_ticks: 100,
        };
        let mut child = Child::new(spec("w").with_backoff(backoff));
        child.set_tid(3);
        assert_eq!(
            child.record_exit(u64::MAX, 1_000),
            Decision::Restart { delay_ticks: 100 }
        );
        assert!(child.is_pending());
        assert!(!child.take_due(1_099), "not yet due");
        assert!(child.take_due(1_100), "due at now + delay");
        assert!(!child.is_pending());
    }

    #[test]
    fn strategy_scope_is_declaration_ordered() {
        let one_for_one = tree(Strategy::OneForOne, &["a", "b", "c"]);
        assert_eq!(one_for_one.scope(1), vec![1]);

        let one_for_all = tree(Strategy::OneForAll, &["a", "b", "c"]);
        assert_eq!(one_for_all.scope(1), vec![0, 1, 2]);

        let rest_for_one = tree(Strategy::RestForOne, &["a", "b", "c"]);
        assert_eq!(rest_for_one.scope(1), vec![1, 2]);
        assert_eq!(rest_for_one.scope(2), vec![2]);
        assert!(rest_for_one.scope(9).is_empty(), "out of range has no scope");
    }
}
