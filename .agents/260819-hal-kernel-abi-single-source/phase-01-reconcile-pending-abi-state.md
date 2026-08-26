---
phase: 1
title: "Reconcile Pending ABI State"
status: completed
priority: P1
effort: "2h"
dependencies: []
tier: medium
---

# Phase 1: Reconcile Pending ABI State

## Overview

Establish the exact post-merge baseline before implementation. The worktree currently has pending tracked edits in `docs/TODO.md` and `kernel/src/memory/paging.rs`; treat them as user/merge-state until verified, not as automatically accepted plan output.

## Requirements

- Functional: classify existing dirty edits, keep ABI scope narrow, and avoid overwriting user notes.
- Non-functional: no runtime behavior changes; no RPi3 hardware claims; preserve branch naming rule without `codex/` prefix.

## Architecture

Data flow: `git status/diff` enters this phase -> classify each changed hunk as ABI closure, docs closure, or unrelated -> only ABI-closure hunks flow into Phase 2; docs closure waits for Phase 3 evidence.

Dependency graph: this phase is the root. Phase 2 cannot edit `paging.rs` until this phase decides whether the existing `HandlePageFault` assertion is retained, revised, or reverted. Phase 3 cannot edit `docs/TODO.md` until compile/smoke evidence exists.

Observed baseline:
- Branch is `refactor/hal-kernel-rust-abi`; HEAD observed as `84cbf1b3`.
- `docs/TODO.md:12` currently has the ABI debt item, and `docs/TODO.md:23` records RV32 kernel failures as baseline debt.
- Pending diff marks TODO item closed and adds `const _: crate::hal::HandlePageFault = vi_handle_page_fault;` after `kernel/src/memory/paging.rs:935`.

## Assumptions

- **Claim:** dirty edits came from the user's merged RPi3/HAL work or prior ABI attempt.
  **Confidence:** medium
  **How to verify:** ask user only if the diff conflicts with implementation; otherwise preserve and work around.

## Related Files

- Modify: `kernel/src/memory/paging.rs`
- Modify: `docs/TODO.md`
- Read: `hal/traits/arch/src/kernel_abi.rs`
- Read: `hal/core/src/lib.rs`
- Read: `docs/code-standards.md`

## Implementation Steps

1. Run `git status --short --branch` and `git diff -- docs/TODO.md kernel/src/memory/paging.rs`.
2. If the pending `HandlePageFault` assertion compiles, keep it; if it fails due to safe/unsafe ABI mismatch, revise in Phase 2 rather than deleting the whole hunk.
3. Leave the `docs/TODO.md` closure hunk uncommitted until Phase 3 evidence exists.
4. Confirm no `__build.bat` or unrelated RPi3 files are dirty before code edits; if present, classify them outside this plan.

## Success Criteria

- [x] Dirty file ownership recorded before editing.
- [x] No unrelated RPi3, board, or build-script hunk is modified by this ABI plan.
- [x] `docs/TODO.md` is not marked closed until Phase 3 validation passes.

## Evidence

- `git status --short --branch` captured the merged-worktree baseline on `refactor/hal-kernel-rust-abi`.
- `git diff -- docs/TODO.md kernel/src/memory/paging.rs` confirmed the existing dirty hunks were ABI-related and preserved.
- The later phase-3 evidence closed the TODO only after compile/boundary/QEMU checks.

## Reviewer

CLEAR

## Security Considerations

N/A. This phase is classification only; it must not change ABI exposed under `libs/api/`.

## Risk Assessment

- Medium likelihood x high impact: overwriting user HAL/RPi3 fixes while resolving ABI hunks. Mitigation: inspect exact hunks and only edit the ABI assertion/doc lines named above.
- Rollback: revert only this phase's `.agents` notes or implementation hunks. Irreversible part: none.

## File Ownership

- Phase 1 owns `docs/TODO.md` classification only and `kernel/src/memory/paging.rs` classification only.
- No parallel phase may edit those files until Phase 1 is complete.

## Deviation Log

None.
