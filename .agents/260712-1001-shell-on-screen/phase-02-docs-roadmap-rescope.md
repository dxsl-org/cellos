# Phase 02 — Docs: rewrite roadmap §B stale claims

## Context Links
- Plan: [plan.md](plan.md)
- Target: `docs/project-roadmap.md` §B "Shell-on-screen: 3 tiers" (~lines 202-218)
- Cross-refs: `docs/specs/15-kernel-boundary.md` (fb_console deletion), `docs/project-changelog.md`

## Overview
- **Priority:** P2 (mandatory per task; documents shipped state)
- **Status:** pending
- **Description:** Correct the stale tier structure in roadmap §B. Tier A's premise ("fb_console ✅"
  as a kernel module) is false — the kernel module was deleted; the cheap path now ships as the
  userspace `fb-console` cell, superseded by the Phase 01 terminal.

## Key Insights (what is stale)
- §B line 206-207: "Mức A — fb_console keyboard relay … fb_console ✅" — kernel `fb_console.rs`
  **deleted** in `6036f2dd` (Boundary Law P08). No kernel text rendering; `GpuFlush=300` forwards to
  the GPU Driver Cell.
- §B line 210-213: "Mức B … IPC pipe output shell qua relay syscall" — the transport is actually the
  **existing** `ReadLog=237` log-ring drain; no relay syscall required for the MVP.
- §B does not state the primary arch (riscv64) or that fb-console already ships.

## Requirements (edits)
1. Rewrite Tier A: kernel `fb_console` deleted; cheap boot-text-on-screen **already ships** as the
   `cells/apps/fb-console` cell (log ring → HDMI); folded into Tier B (terminal supersedes it).
2. Rewrite Tier B: Terminal Emulator Cell = evolve fb-console; transport = `ReadLog=237` (no new
   syscall for MVP); reuses ViUI `FONT8X8` + compositor Grant surfaces; primary arch riscv64;
   minimal ANSI subset (clear/home/BS/CR/LF/tab; colors deferred). Link `.agents/260712-1001-shell-on-screen/`.
3. Keep Tier C (SSH via Tier 3b) unchanged but note "config-only, gated on Tier 3b — no plan".
4. Mark status once Phase 01 lands (📋 → 🔨/✅).

## Related Code Files
**Modify**
- `docs/project-roadmap.md` (§B, ~202-218)
- `docs/project-changelog.md` (add entry: terminal cell ships; fb_console kernel deletion clarified)

## Implementation Steps
1. Read current §B; rewrite tiers per Requirements with file:line evidence for the fb_console deletion.
2. Add changelog entry referencing the plan folder.
3. Verify cross-refs/dates; confirm no other doc repeats the "kernel fb_console" claim
   (grep: `docs/codebase-summary.md` mentions `fb_console` — reconcile).

## Todo List
- [ ] Rewrite roadmap §B tiers A/B/C
- [ ] Changelog entry
- [ ] Reconcile `docs/codebase-summary.md` fb_console mention

## Success Criteria
- §B contains no claim that a kernel `fb_console` exists.
- Tier A described as shipped (fb-console cell) + superseded; Tier B links this plan; primary arch stated.
- `grep -rn "fb_console" docs/` yields only accurate (userspace-cell / historical-deletion) references.

## Risk Assessment
| Risk | L×I | Mitigation |
|---|---|---|
| Docs drift again vs code | Low×Low | Cite file:line + commit hashes in the doc edit |
| Over-claiming before Phase 01 merges | Low×Med | Gate ✅ status on Phase 01 completion |

## Next Steps
- Deferred Phase 03 remains a roadmap "future" bullet under Tier B.
