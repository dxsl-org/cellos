---
phase: 02
title: Detection — Deadline Enforcement + Watchdog Tick
priority: P0
status: planned
depends_on: ["01"]
risk: high
---

# Phase 02 — Detection (Deadline + Watchdog)

> ⚠️ **Red-team revisions (authoritative — override conflicting text below):**
> - **Canonical clock = `system_ticks()`** (the 10ms software counter, [task.rs:76-77](../../kernel/src/task.rs)).
>   The timer hook is `vi_timer_tick()` (takes NO arg — [trap.rs:80](../../hal/arch/riscv/src/rv64/trap.rs));
>   read "now" via `system_ticks()` inside it, not a passed `mtime`.
> - **RecvTimeout deadline is currently a *relative mtime* timeout stored verbatim**
>   ([syscall.rs:440-447](../../kernel/src/task/syscall.rs)) while `Sleeping{until}` is *absolute
>   software ticks*. A naive sweep compares incomparable values. **Fix at the syscall boundary
>   (Law 1):** convert to `deadline_abs = system_ticks() + timeout` in the canonical clock so the
>   sweep is correct. The `Sleeping` sweep at [scheduler.rs:281-297] already exists — extend it to
>   also sweep `Recv{deadline}`.
> - **`run_ticks` reset is scattered** (≥5 block sites: RecvTimeout/Wait/Sleeping/FutexWait/Sending).
>   Reset at the **single schedule-in chokepoint** `pick_next` ([scheduler.rs:357]) and treat
>   **`Polling`/async-await as NON-accruing** (a cell parked on a slow kernel future must not accrue
>   watchdog ticks → else false-positive RT kill). Base the budget on cell-code S-mode execution,
>   not wall ticks while descheduled.

## Context Links
- Spec: [12-reliability.md](../../docs/specs/12-reliability.md) §4.2
- Code: [kernel/src/task/tcb.rs](../../kernel/src/task/tcb.rs) (`TaskState::Recv{deadline}` @31-36, `Sleeping{until}`)
- Code: [kernel/src/task/scheduler.rs](../../kernel/src/task/scheduler.rs) (`pick_next`, ready queues)
- Code: [kernel/src/task/syscall.rs](../../kernel/src/task/syscall.rs) (`RecvTimeout` @440-466)
- Code: [hal/arch/riscv/src/rv64/trap.rs](../../hal/arch/riscv/src/rv64/trap.rs) (timer IRQ, code 5)

## Overview
- **Priority:** P0
- **Status:** planned
- **Description:** Today a task can block forever. `RecvTimeout` stores a `deadline` that the
  scheduler never checks, and there is no detection of a cell that monopolizes the CPU
  (livelock — "alive but paralyzed"). Add timer-driven (1) deadline expiry and (2) a runtime
  budget watchdog. These make hangs *observable and reapable* — the foundation the supervisor
  (Phase 03/04) acts on.

## Key Insights
- A timer interrupt path already exists (priority preemption, trap.rs code 5). Both features
  hook the **same per-tick callback** — no new interrupt wiring.
- `Sleeping { until }` already implies a "wake at tick T" mechanism; deadline expiry should
  **reuse the same monotonic-tick comparison** rather than invent a parallel timer.
- Watchdog must distinguish *legitimately busy RealTime cell* from *runaway loop*. Use a
  per-task **runtime budget** (ticks of continuous CPU without yielding/blocking); exceeding
  it escalates (warn → preempt → mark Faulted). Budgets are per-priority (RealTime gets more).

## Requirements
**Functional**
- On each timer tick: any task in `Recv{deadline:Some(d)}` with `d <= now` is woken with a
  timeout error return (consistent with existing `usize::MAX`/error convention).
- A task that runs `budget` consecutive ticks without yielding/blocking is force-preempted;
  repeated offenders (configurable threshold) are terminated via the fault path.
- Watchdog action emits an audit event (new `CellWatchdog` or reuse `CellFault` with a code).

**Non-functional**
- Per-tick scan is O(blocked tasks); acceptable for current cell counts. Document the bound.
- No starvation of legitimate RealTime work — budget tuned so normal control loops never trip.

## Architecture
```
timer_tick(now):                      ← single callback from trap.rs code 5
  ├─ deadline sweep:
  │     for t in tasks where Recv{deadline:Some(d)} && d <= now:
  │         wake(t, TIMEOUT_ERR); push_ready(t)
  ├─ budget accounting:
  │     cur = current_task
  │     cur.run_ticks += 1
  │     if cur.run_ticks > budget(cur.priority):
  │         audit(CellWatchdog, cur)
  │         cur.strikes += 1
  │         if cur.strikes >= MAX_STRIKES: terminate_current_cell_on_fault(WATCHDOG, pc)
  │         else: force preempt (pend SSIP), reset run_ticks
  └─ existing preemption logic
on context switch IN:  task.run_ticks = 0   ← reset budget when (re)scheduled
on block/yield:        task.run_ticks = 0
```

## Related Code Files
**Modify**
- `kernel/src/task/tcb.rs` — add `run_ticks: u32`, `strikes: u8` to Task; constants for budgets.
- `kernel/src/task/scheduler.rs` — `timer_tick(now)` doing deadline sweep + budget accounting;
  reset `run_ticks` on schedule-in and on state→blocked.
- `hal/arch/riscv/src/rv64/trap.rs` — call `scheduler::timer_tick(now)` in the timer arm.
- `kernel/src/task/syscall.rs` — ensure `RecvTimeout` sets `deadline` consistently; define the
  timeout error return value used by the sweep.
- `kernel/src/audit.rs` — add `CellWatchdog` event (or document reuse of `CellFault` + code).

## Implementation Steps
1. Add `run_ticks`, `strikes` to `Task`; add `WATCHDOG_BUDGET_TICKS[priority]` and
   `MAX_STRIKES` constants with rationale comments (why these values won't trip normal RT).
2. Implement `Scheduler::timer_tick(now)`: deadline sweep first (wake timed-out receivers),
   then budget accounting for the current task.
3. Reset `run_ticks` to 0 wherever a task is scheduled in (`pick_next`) and wherever it
   transitions to a blocked state (Recv/Sending/Sleeping/FutexWait).
4. Wire `timer_tick(now)` into the trap timer arm; pass the monotonic tick/`mtime`.
5. Define `TIMEOUT_ERR` and apply it in the sweep so `RecvTimeout` callers get a clean,
   documented error (align with `libs/api` — **Law 1: confirm if a new error variant is added**).
6. Add audit event; log watchdog escalations.
7. Build per-arch; boot QEMU.

## Todo List
- [ ] TCB fields + budget constants
- [ ] `timer_tick` deadline sweep
- [ ] `timer_tick` budget accounting + escalation
- [ ] `run_ticks` reset points (schedule-in, block)
- [ ] Trap timer wiring
- [ ] Timeout error return for RecvTimeout (Law 1 check)
- [ ] Audit `CellWatchdog`
- [ ] Test: cell blocks on dead peer with RecvTimeout → wakes with error
- [ ] Test: cell in `loop{}` → preempted, then terminated after MAX_STRIKES; shell alive

## Success Criteria
- A test cell calling `RecvTimeout` against a peer that never replies returns an error within
  ~deadline, instead of hanging forever.
- A test cell running `loop{}` at Normal priority is terminated; system stays responsive.
- A normal RealTime control cell running its periodic loop is **never** falsely terminated
  (tune budgets; document the headroom).

## Risk Assessment
- **False-positive watchdog kill of legitimate RT work (High).** A mis-tuned budget kills a
  real control cell → robot loses control. *Mitigation:* conservative budgets with wide
  headroom; warn-before-kill (strikes); make budget per-priority and a named constant; add a
  test that a busy-but-cooperative RT cell survives.
- **Per-tick scan cost grows with blocked-task count (Med).** *Mitigation:* current counts are
  small; if needed later, keep a min-heap of deadlines. Document the O(n) bound now.
- **Law 1:** new error variant / RecvTimeout semantics in `libs/api` → **2× user confirm.**

## Security Considerations
- Watchdog is a DoS mitigation (a cell can't wedge the CPU). Ensure budget can't be raised by
  a cell itself (kernel-owned constant; no syscall to extend own budget).

## Next Steps
- Phase 03 consumes these detection signals: watchdog/timeout terminations become supervisor
  restart triggers via death-notification.
