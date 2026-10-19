---
phase: 1
title: "Source baseline, exact contracts and executable image path"
status: completed
priority: P1
tier: thinking
dependencies: []
---
# Phase 01 — Baseline and contract gates

## Overview
Freeze one truthful source/image baseline, write the concrete desktop session/theme/launch/stream contracts and obtain exact interface approvals before implementation. Planning approval is not ABI approval.

## Requirements
AC-03/12/13/19; cover all acceptance IDs through the ledger. Intel x86-64 only. Preserve user-owned changes, existing capability semantics and frozen opcodes.

## Architecture
Compositor owns frames, surfaces, focus, geometry and a bounded view inventory. Desktop owns shell UI, app catalog/launch intent and UI preference policy. Apps own session contents and close decisions; init/existing lifecycle authority owns spawning and system power. SDK carries correlation, owner-generation and bounded asynchronous operations, not synchronous GUI cycles. Reuse actual lifecycle provenance rather than inventing a second Cell registry.

## Related Files
- Existing: `docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md`, `docs/code-standards.md`, `.agents/cell-contract-0024-locks.md`, controlling Cell plan and approvals.
- Existing consumers: `libs/api/src/services/{ipc,display}.rs` (confirm exact display location in recon), `libs/ostd/src/{display,input,ipc,fs,io}.rs`, `libs/viui/src/managed_surface.rs`, compositor, init, shell.
- Existing build: `scripts/build-cloud-images.sh`, `scripts/package-ocel-profile.py`, `scripts/generate-boot-config.py`, `scripts/sign-policy.py`, `run-desktop.sh`, x86 image/test runners.
- Create here: `contracts.md`, `abi-delta-ledger.md`, source/image manifest and execution-evidence index. Final module placements follow verified source map, not guesses.

## Implementation Steps
1. Capture current revision plus dirty-file/content hashes, compiler/target/features, firmware, guest artifacts and prepared image inventory without reverting user edits. Read retained incident/evidence records first. Build/capture only after implementation admission; planning itself edits no runtime files.
2. Reconcile stale `current-focus.md` documentation-only statements against controlling plan completion labels and exact approval records. Name any affected file-owner and Law-1 CP1/CP2 gates. New desktop request permits this plan, not blanket authority to rewrite frozen VFS/stdio/VM ABI or release G2-J.
3. Specify proposed typed contracts and owner table: AppId, view/frame/session references, owner incarnation, new-session request, authenticated launch target, selection/snapshot revision, configure-ack, dirty/close/refusal/modal, theme revision, stream endpoints and guest host input. Bound counts/strings/queues/timeouts; name owner-death/restart/exhaustion behavior. Field names are proposed until ratified.
4. Allocate only approved new operations/versions; never renumber postcard variants or reinterpret fixed layouts. Public symbol edits require LSP references first when available, whole-workspace consumer census and clean caller migration. Obtain design approval before `libs/api`/`libs/types` edits, implementation approval against actual delta/evidence afterwards.
5. Create a reproducible CLI/desktop image selection path using existing builders, signing and boot-config source-of-truth; isolate new evidence images, preserve unrelated embedded cells and keep safe CLI recovery. Package a real consumer per phase, not optional build-only stubs.
6. Bind tests to actual x86 runner/profile; RV64 `desktop-shell.rs` cannot silently stand for Intel evidence. Establish reusable QMP keyboard/pointer/screendump capture and matching VMware interaction procedure. Baseline smoke after admission; no false skip on missing artifacts.

## Assumptions
- Existing actual-module Cell/stream/VM approvals may cover reusable consumers; verify exact ledger, not headline status. Confidence medium.
- QEMU/KVM and VMware image/guest inputs can be pinned; verify host availability and image manifest. Missing VMware blocks that evidence only, never produces a pass or stops reachable implementation.

## Success Criteria
- [x] `contracts.md` has finite states, authority and cancellation/failure behavior for each changed interface; ABI ledger records CP1 and CP2 separately with complete generational qualifiers, open-file launch binding, close consent revision vectors, and full filename pagination.
- [x] Every AC maps to a phase and live scenario; no approved feature disappears behind an unresolved gate.
- [x] Actual x86 image boots recovery CLI and existing desktop entry after admitted changes; evidence pins image/source/features and materializes `tests/integration/tests/desktop-x86.rs`. No new kernel authority for GUI policy.
## Security Considerations
Do not trust caller-supplied TID/AppId/frame ID. Launch association is not authorization. System shutdown, force-stop and theme mutation have explicit trusted caller paths. No arbitrary `/apps` path grants or broad capability relaxation to make a GUI launch pass.

## Risk Assessment
Main risk: stale/mutually inconsistent approval documents and hidden shared consumers. Roll back only this phase's implementation patch or boot the previously pinned image; do not revert user work. No data migration intended; any actual persistent/config writes and distributed/protected effects are excluded until explicitly approved.

## Deviation Log
- Decision: Materialized Law-1 CP1 design gates for D-UI-01 through D-UI-10 in `abi-delta-ledger.md` (marked PENDING CANDIDATE APPROVAL, keeping planning approval separate from ABI approval) and recorded complete untracked user artifact census in `baseline-manifest.md`.
- Decision: Materialized the planned x86 integration test target `desktop-x86` in `tests/integration/tests/desktop-x86.rs` and wired it into `tests/integration/Cargo.toml`.
- Surprise / Retained Evidence: Baseline QEMU KVM boot of `build/cellos-x86_64-desktop.qcow2` observed pre-existing `COMPLETION-QUEUE: FAIL` self-test, but cleanly reached `Cellos >` interactive shell, VMX root operation, and compositor/fb-console/sshd startup in 12 seconds.
