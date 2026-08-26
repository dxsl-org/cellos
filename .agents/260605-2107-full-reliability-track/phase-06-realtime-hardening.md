---
phase: 06
title: Realtime Hardening — CPU Budget + WCET + EDF Eval
priority: P1-P2
status: planned
depends_on: ["02"]
risk: low
gated: data (EDF only)
---

# Phase 06 — Realtime Hardening (committed; EDF data-gated)

> ✅ **Validated decision 2026-06-05: Phase 06 stays IN the track** (not deferred). Stages A
> (WCET measurement) + B (CPU budget) are committed. Stage C (EDF) remains **data-gated** — only
> built if measured jitter misses the target. Runs after Phase 02; does not block 03/05/04.

## Context Links
- Spec: [12-reliability.md](../../docs/specs/12-reliability.md) §4.5
- Code: [kernel/src/task/scheduler.rs](../../kernel/src/task/scheduler.rs) (3-level priority + SSIP preempt)
- Builds on Phase 02 (timer tick + budgets already exist by then).

## Overview
- **Priority:** P1-P2 (do NOT block the never-die spine 01→04)
- **Status:** planned, research-gated — may be deferred to a later milestone.
- **Description:** "Never-die" axis 4 = not dying *by deadline*. The current scheduler is
  priority-preemptive but offers no temporal guarantee: no CPU budget per cell, no measured
  WCET, no deadline-aware scheduling. This phase adds *measurable* RT behavior. Scope is
  deliberately staged: measure first, then decide whether EDF is warranted (YAGNI).

## Key Insights
- Phase 02 already added per-task `run_ticks` accounting — extend it into a **CPU budget /
  time-slice guarantee** rather than inventing new machinery.
- You cannot claim realtime without **numbers**. WCET of the syscall + IPC + context-switch
  paths must be measured (cycle counters) before any deadline math is meaningful.
- EDF is a big change to the scheduler core. Justify it with data: if priority + budget already
  meets target jitter for the robot-control use case, EDF is unnecessary complexity. Decide
  after measurement.

## Requirements
**Functional (staged)**
- Stage A (measure): instrument syscall entry/exit, IPC send/recv, context switch with cycle
  counters; report min/avg/max (WCET) over a benchmark workload.
- Stage B (budget): per-cell CPU budget over a window; a cell exceeding budget is throttled
  (not killed — distinct from the watchdog runaway case).
- Stage C (eval, gated): prototype deadline-aware (EDF or rate-monotonic) scheduling ONLY if
  Stage A/B data shows priority+budget misses the jitter target.

**Non-functional**
- Measurement harness must be reproducible (QEMU + ideally one real board later).
- Any scheduler change preserves existing priority preemption semantics.

## Architecture
```
Stage A: cycle-counter probes (rdcycle) at:
   - ecall enter / sret exit
   - ipc send / recv boundaries
   - context-switch in/out
   → accumulate histograms; expose via a debug syscall or audit drain

Stage B: per-cell budget window (extends run_ticks):
   budget_used[cell] over period; if exceeded → demote/throttle until window resets

Stage C (only if needed):
   add deadline field to RT tasks; pick_next prefers earliest-deadline among RT
   (replaces/augments fixed RealTime priority bucket)
```

## Related Code Files
**Modify**
- `hal/arch/riscv/src/rv64/*` — `rdcycle` probe helpers (`// SAFETY:` on CSR reads).
- `kernel/src/task/scheduler.rs` — budget window; (Stage C) deadline-aware pick.
- `kernel/src/task/syscall.rs` — debug syscall to dump WCET histograms (or via audit).
- `kernel/src/task/tcb.rs` — budget window fields; (Stage C) deadline field for RT tasks.

## Implementation Steps
1. Add `rdcycle`-based probes; build a histogram accumulator (fixed buckets, no_std-friendly).
2. Define a benchmark workload (existing integration tests + a tight IPC ping-pong).
3. Measure & document WCET of syscall/IPC/context-switch in a report under `.agents/reports/`.
4. Implement per-cell CPU budget window + throttle; verify a greedy-but-cooperative cell is
   throttled, not killed.
5. **Decision gate:** compare measured jitter vs target. If met → STOP (skip EDF). If not →
   proceed to Stage C prototype and re-measure.
6. (Stage C) Prototype deadline-aware RT selection behind a feature flag; measure improvement.

## Todo List
- [ ] `rdcycle` probes + histogram
- [ ] Benchmark workload defined
- [ ] WCET measurement report (.agents/reports/)
- [ ] Per-cell CPU budget window + throttle
- [ ] Decision gate: priority+budget vs target jitter
- [ ] (Conditional) EDF prototype behind feature flag + re-measure

## Success Criteria
- A documented WCET report exists for syscall/IPC/context-switch paths.
- A CPU-budget test shows a greedy cooperative cell throttled without affecting a higher-prio
  RT cell's timing.
- A reasoned, data-backed decision recorded on whether EDF is needed (and if so, a measured
  improvement; if not, an explicit "priority+budget meets target" conclusion).

## Risk Assessment
- **Scope creep into a scheduler rewrite (Med).** EDF can balloon. *Mitigation:* hard decision
  gate after measurement; EDF behind a feature flag; default off.
- **Measurement under QEMU misleads (Low-Med).** TCG timing ≠ silicon. *Mitigation:* treat
  QEMU numbers as relative; validate on a real board before any RT guarantee is published.

## Security Considerations
- Debug WCET syscall must not leak cross-cell timing usable as a side channel to untrusted
  cells — gate it to a debug build or a privileged capability.

## Next Steps
- Feeds a future "RT guarantees" claim in docs only once backed by real-board numbers.
