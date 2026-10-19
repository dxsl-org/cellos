---
title: "CellOS unified desktop, native sessions and shared Linear theme"
status: in-progress
priority: P1
tier: thinking
created: 2026-10-19
blockedBy: []
blocks: []
---
# Unified desktop implementation plan

## Scope and authority
Implement all approved [ADR-0027](../../docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md) outcomes on Intel x86-64; see [acceptance ledger](acceptance.md), [proposed contracts](contracts.md) and [source recon](scout-report.md). Phase 01 baseline and contract gates are complete; implementation phases (02–11) proceed under Law-1 ABI gates. Existing user changes remain untouched. Phase 01 resolves exact file-owner/ABI admission; this plan does not unlock G2-J, remote/protected/physical or broad legacy programs.

## Phases
| Phase | Deliverable | Depends on | State |
|---|---|---|---|
| 01 | [Baseline and exact contract gates](phase-01-contract-baseline.md) | — | completed |
| 02 | [Shared Linear theme and components](phase-02-system-theme.md) | 01 | pending |
| 03 | [Native views, frames, tabs and lifecycle](phase-03-native-sessions.md) | 01,02 | pending |
| 04 | [Bottom taskbar, Spotlight and launch registry](phase-04-desktop-launcher.md) | 01,02,03 | pending |
| 05 | [Six-zone snap and F11 fullscreen](phase-05-placement-fullscreen.md) | 03,04 | pending |
| 06 | [Ocel native session and reader cutover](phase-06-ocel-cutover.md) | 02,03,04,05 | pending |
| 07 | [Standalone Files and complete VFS operations](phase-07-files-vfs.md) | 01,02,03,04,05,06 | pending |
| 08 | [Shared preview and Quick Look](phase-08-quick-look.md) | 02,06,07 | pending |
| 09 | [GUI Terminal and shell-session streams](phase-09-terminal-streams.md) | 01,02,03,04,05 | pending |
| 10 | [Real Tier3 single-app guest integration](phase-10-tier3-app-session.md) | 01,03,04,05,07 | pending |
| 11 | [Complete image and user-path qualification](phase-11-qualification.md) | 02–10 | pending |

## Overall progress
- **Phase 01 — Baseline and exact contract gates**: **Completed** (2026-10-19).
  - Materialized `abi-delta-ledger.md` (D-UI-01 through D-UI-10 specification, candidate digest, wire layouts, Law-1 CP1 pending ratification).
  - Materialized `baseline-manifest.md` (source baseline, dirty tracked and untracked files inventory with full SHA-256 hashes, toolchain state, verification matrix).
  - Materialized and verified `tests/integration/tests/desktop-x86.rs` and `boot_x86_bios_with_accel` in `tests/integration/src/lib.rs` (tested TCG and KVM, QMP pointer routing + screendump diff, QMP keyboard injection).
  - Reconciled `docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md` scope; updated `contracts.md` and `phase-01-contract-baseline.md`.
- **Phase 02 — Shared Linear theme and components**: Pending (next phase).
- **Phases 03–11**: Pending.

## Execution policy
One integrating owner for API/SDK/kernel/shared lifecycle changes. Independent research and bounded app-specific slices may parallelize after interfaces freeze; do not concurrently edit shared source. Package and smoke a real consumer each phase; phase 11 is cross-feature qualification, not first integration. Every AC remains required for whole-plan completion, even when a prerequisite blocks one phase. Local captures do not qualify physical or production behavior.

## Key findings
- Existing surface resize/close and ViTheme are reusable; generation-bound session frames/Paper/live theme are missing.
- Files needs complete directory paging and safe operation results, not only a UI over a truncated listing.
- `fb-console` is not a GUI Terminal; atomic session I/O and explicit interrupt/resize need work.
- `vm-runner`/Chrome currently log and exit; real app VM/payload/display/resize must be implemented and evidenced.
- Conflicting plan-complete/freeze projections require exact provenance review, not blanket substrate assumptions.

## Assumptions and dependencies
[Contracts](contracts.md) name proposed owner/state semantics; exact wire/capacity constants are phase-01 design/approval work. Reuse current lifecycle/VFS/stream/VMX implementation only when source and exact approval demonstrate it. Guest/runtime artifacts, status providers and KVM/VMware availability remain explicit checks; unavailable hardware blocks only that evidence, never silently narrows delivery. No paid service, procurement or new platform assumed.

## Deferred by approved scope
Drag-to-group/detach, Files split-pane/full tree, desktop icons/widgets, generic app installer/store and third-party inner-tab extraction. Menu group/detach, all three Files views, four preview families, native sessions and actual Tier3 app remain in scope.

## Verification and rollback
Use actual-module behavior tests plus live UI/state captures on exact signed images; [verification commands and scenarios](verification.md) distinguish current commands from the planned desktop-x86 runner. Preserve prior reported failures without redundant confirmation. Measure performance rather than inheriting unsupported RAM/startup/preview targets. Each phase states reversible code/image scope and irreversible session/file/guest effects. [Acceptance ledger](acceptance.md) owns AC coverage and evidence requirements.

## Red Team Review
Four internal review lenses completed; [20 unique findings and corrections](review.md) recorded (17 Major, 3 Minor, no Critical). Fixed dependencies, Files new sessions, shortcut dispatch, snap selector, prepared-close/kill/open authority, bounded decoder termination, actual VMM dispatch/device topology, child stdio/single-reader stdin, atomic group configure and coherent file replacement. No external review egress or independent human approval claimed.

## Validation Log
Product choices are approved in conversation. Three read-only recon slices plus direct source checks ground prerequisites; artifact validation checks links, AC IDs and phase/index DAG consistency. Phase 01 execution is completed: baseline manifest frozen, D-UI ABI deltas specified, and real x86 integration test runner materialized and verified in `tests/integration/tests/desktop-x86.rs`. Exact ABI approval, uncertain storage/VMX provenance and unavailable platform evidence remain phase-specific gates, not reasons to reopen settled UI choices.

## Handoff
Phase 01 baseline is established and verified. Next step: Phase 02 (Shared Linear theme and components) implementation via `hc-cook /home/dmin/cellos/.agents/261019-1310-unified-desktop/plan.md`. Observe phase gates and ABI ratification checkpoints prior to modifying `libs/api` or `libs/types`.
