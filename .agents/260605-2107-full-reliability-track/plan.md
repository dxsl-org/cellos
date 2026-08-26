---
title: Full Reliability Track — ViCell "Never-Die"
slug: full-reliability-track
created: 2026-06-05
status: planned
owner: ViOS Team
spec: docs/specs/12-reliability.md
---

# Full Reliability Track — ViCell "Never-Die"

Raise ViCell's aggregate never-die score from ~25-30% toward ~55-60% by building the
**detection + recovery** layer that turns isolated cell crashes into recoverable events.
Source of truth: [docs/specs/12-reliability.md](../../docs/specs/12-reliability.md) §4.

**Decision baseline (do not revisit):** per-cell SATP is dropped; isolation is tiered
(Tier 1 LBI / Tier 2 WASM / Tier 3 Stage-2). This track is pure reliability, independent of
that decision.

## Guiding constraints
- **Law 1 (Interface is Sacred):** new syscalls / `libs/api` changes need 2× user confirm.
  Phase 02 & 03 add syscalls — flagged in each phase.
- **Law 4:** kernel `unsafe` only with `// SAFETY:`. Cells stay `#![forbid(unsafe_code)]`.
- **Law 8 (RAII):** reclamation work (Phase 05) must use `Drop` where possible.
- No fake/stub completion — every phase ends with a build + QEMU boot + observable behavior.

## Validated decisions (2026-06-05 — `/hc:plan validate`)
- **C1 stays inside Phase 00** (not a separate hotfix) — fixed as the foundation of the track.
- **State recovery = ON with a generation marker.** `state_stash` gains a generation/validity
  field; the supervisor skips/clears a slot after a post-restore fault (no restore-crash loop).
  Built in Phase 03 (stash primitive) + consumed in Phase 04 (policy).
- **Supervisor host = new dedicated cell** `cells/services/supervisor` (init spawns it first,
  grants caps). Not folded into init.
- **Phase 06 stays IN the track** (committed, not deferred). Still data-gated: EDF only if
  measured jitter misses target.

## Phases

> **Restructured after red-team** (`.agents/reports/red-team-260605-2126-full-reliability-track.md`).
> Phase 00 added (fault-path crash-safety is a hard prerequisite — a pre-existing lock-leak
> bug that auto-restart would amplify). Phase 05 moved BEFORE 04 (reclamation must land before
> high-frequency restart, else every restart leaks to OOM). SATP stays dropped (red-team F2
> reinforces it: single shared page table → no per-cell PT to reclaim).

| # | Phase | Priority | Depends on | Risk |
|---|-------|----------|-----------|------|
| 00 | [Fault-path crash-safety](phase-00-fault-path-crash-safety.md) | P0 (foundation) | — | High (panic/fault path, locks) |
| 01 | [Stop silent death — guard pages + reboot-on-panic](phase-01-stop-silent-death.md) | P0 | 00 | High (paging) |
| 02 | [Detection — deadline enforcement + watchdog tick](phase-02-detection-deadline-watchdog.md) | P0 | 01 | High (scheduler, Law 1) |
| 03 | [Supervisor kernel primitives](phase-03-supervisor-kernel-primitives.md) | P0 | 00, 02 | Med (TCB, syscall, Law 1) |
| 05 | [Stop slow death — frame reclaim + async-pin GC](phase-05-stop-slow-death.md) | P0 (was P1) | 03 | High (SAS memory safety) |
| 04 | [Root supervisor cell + restart policies](phase-04-root-supervisor-cell.md) | P0 | 03, 05 | Med (userspace logic) |
| 06 | [Realtime hardening — CPU budget + WCET + EDF eval](phase-06-realtime-hardening.md) | P1-P2 | 02 | Low (mostly measurement) |

## Critical path
`00 → 01 → 02 → 03 → 05 → 04` is the never-die spine. **05 now precedes 04** so auto-restart
never runs without reclamation (red-team H1). 06 is research-gated and may be deferred.

> **Phase 00 is non-negotiable first.** Without it, a kernel panic while servicing a cell
> syscall is mis-classified as a cell fault and resumes with global locks held → permanent
> deadlock. Both 04 (restart) and 05 (reclaim) increase alloc/free under those locks, so the
> rest of the track *amplifies* this bug until 00 fixes it.

## Success criteria (track-level)
- A crashed Tier-1 service cell (e.g. VFS) is **automatically restarted** with its
  well-known endpoint intact, verified live in QEMU.
- A cell stuck in `loop{}` or blocked on a dead peer is **detected and reaped** (no
  permanent hang).
- A true kernel panic **reboots** instead of `wfi`-halting.
- Stack overflow **traps** instead of silently corrupting memory.
- A long-running boot→crash→restart cycle shows **no monotonic frame leak** (Phase 05).
- Detection axis ~15%→~65%, Recovery axis ~10%→~70% (per spec §4 trajectory).

## Out of scope
- Per-cell SATP (dropped). Tier 3 Stage-2 hypervisor (separate track).
- Code-signing / secure-boot (Security track — load-bearing for trust model, not for reliability).
- ECC / hardware redundancy (axis 6 — needs target-board hardware).
