---
phase: 10
title: "Integrated signed-image qualification and measurement"
status: pending
priority: P1
tier: thinking
dependencies: [1, 2, 3, 4, 5, 6, 7, 8, 9]
---
# Phase 10: Qualification and delivery

## Overview and requirements

Close CBX-26..28 and audit CBX-01..25 plus CBX-29 on a matched signed x86 image/policy. All selected functionality remains required; earlier individual smoke proofs do not replace integrated user paths or lift physical/production gates.

## Architecture and related files

Update the affected image/package/signing recipes, command/app-development docs and existing integration harness. Extend `tests/integration/tests/desktop-x86.rs` or add a focused x86 CLI target only if needed for real behavioral proof; register any new target explicitly. Existing boot/pointer coverage is not a full terminal/search/editor/SSH acceptance run. Preserve unrelated user changes and independent service/app artifacts.

## Implementation steps

1. Audit every concrete delta's CP1 digest, exact implementation diff, consumer migration census, evidence and explicit CP2 approval; pending/blocked CP2 stops final cutover/rollout, not the disposable evidence work needed for review. Rebuild changed artifacts with correct Cell/kernel codegen and pinned notices, assemble a private matched signed image/policy and verify actual bytes; no stale artifact fallback.
2. Run headless and GUI-terminal consumer paths against the same suite: script/state transitions, typed and raw/external pipelines, top record/TUI, multi-chunk search, editor save/reopen and SSH remote command/PTY. GUI SSH proof joins P05's emulator with P09's headless client; P09 does not wait for Ratatui/GUI completion.
3. Observe negative cases on the integrated image: restricted file/lifecycle access, forged mode/alias, cross-session input, canceled streams, unavailable/truncated telemetry, search partial errors, editor interrupted/conflicting saves, SSH host/auth failure and unknown completion. Verify ordinary one-shot/non-terminal commands never unexpectedly become TUI or read keys, and unsupported explicit TUI requests fail truthfully.
4. Measure the same selected command/feature baseline and integrated build: suite executable count/bytes, image footprint, startup, heap/stack, idle redraw and long-stream bounds. Separate linker/artifact measurements from runtime resident-memory claims; report regressions as well as improvements.
5. Audit every command catalog caller, Cargo target, image/sign entry and docs example for a complete clean cutover. No obsolete suite binaries/wrappers, fake GUI interpreter or duplicate authority path may remain; retain unrelated independent consumers.
6. Update each acceptance row only with exact commit/version/profile/image/server provenance and actual evidence. Keep unavailable physical/VMX/remote/production claims outside scope; a missing required guest SSH proof remains blocked rather than marked done.
7. Update current docs, help/command catalog, changelog and portfolio to the exercised state; retain historical evidence at its original ceiling. Remove temporary consumers/fixtures/private secrets and record safe rollback/recovery limitations.

## Success criteria

- [ ] CBX-01..29 have actual evidence with no silent exclusions or placeholders.
- [ ] Real keyboard/screen/state and network/file effects prove selected behavior, not source assertions or fixed mock output.
- [ ] One suite artifact and all retained independent apps/services package correctly; no broad ambient authority or hidden legacy fallback.
- [ ] Measurements compare equivalent feature sets and distinguish host/QEMU/physical ceilings.
- [ ] Public docs, notices, command help, caller routes and portfolio match the delivered implementation; no IDE/build environment is scheduled or implied.

## Verification

Follow [verification.md](verification.md); run affected checks once after implementation edits, then actual consumer smoke and integrated qualification. No broad test reruns to reconfirm a user-reported failure. Keep meaningful behavior regressions, not incidental wording/forwarding tests.

## Assumptions

Exact required endpoint/image fixtures and approvals are available at entry. Verify paths/features and private-image ownership before running. Lack of hardware or external fixture does not authorize fabricated evidence or scope reduction.

## Security and risk assessment

Do not use an unattended runner that kills another VM or opens inbound SSH. Observe artifact/policy provenance and protect captured secret data. Rollback uses a matched safe image/policy and cold boot; file saves, trust changes and remote effects require their own recovery and are not reversed by code rollback.

## Deviation log

None; implementation pending. Record findings/decisions with reason/impact/revert and do not mark blocked acceptance as complete.
