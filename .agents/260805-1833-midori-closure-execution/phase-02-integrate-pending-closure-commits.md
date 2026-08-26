---
phase: 2
title: "Integrate Pending Closure Commits"
status: completed
priority: P1
effort: 1d
dependencies: [1]
tier: medium
---

# Phase 02: Integrate Pending Closure Commits

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Review, test, and integrate the pending closure slice into `main` without conflating bounded evidence with full Midori closure. The landed evidence is `3f6ad45d` and `3bd8aaf0`; the remaining `ReadGrant` producer and fast-IPC `GetFile` gaps move to Phase 03.

## Requirements

- Functional: integrate `b5a97125` (`codex/midori-phase01-evidence`) and `eecfbb72` (`codex/midori-vfs-region-fold`) if still unmerged and still tree-compatible.
- Non-functional: no destructive branch cleanup until exact refs, ancestry, and tree equality are verified immediately before deletion.

## Architecture

Data flow: clean docs base -> cherry-pick or merge single-commit branch -> run targeted tests -> update status docs only to the proven scope -> repeat. Exit output is `main` containing both slices or a documented blocker.

## Assumptions

- **Claim:** The two commits are still single-commit branches.
  **Confidence:** medium
  **How to verify:** `git log --oneline main..codex/midori-phase01-evidence` and equivalent for `codex/midori-vfs-region-fold`.

## Related Files

- Modify: files touched by `git show --stat b5a97125`
- Modify: files touched by `git show --stat eecfbb72`
- Avoid committing generated dirty artifact: `kernel/src/embedded-test-hooks/init`

## Implementation Steps

1. Fetch/prune and capture exact local/remote refs.
2. Confirm `git merge-base --is-ancestor b5a97125 main` and `... eecfbb72 main` are still false before applying.
3. Review `b5a97125` diff; run VFS quota/grant evidence commands from its test comments.
4. Apply `b5a97125`; restore any generated tracked binaries if tests dirty them.
5. Review `eecfbb72` diff; run policy parser/self-test and boot-ceiling checks.
6. Apply `eecfbb72`; resolve docs conflicts against Phase 01 docs.
7. Run baseline gates: format/check, targeted tests, and QEMU evidence commands available in repo scripts.
8. If branch cleanup is requested after integration, verify exact refs + ancestry + tree before deletion.

## Todo List

- [x] Pre-flight `git status --short --branch` captured the existing dirty state before any claim of closure.
- [x] `b5a97125` / `3f6ad45d` evidence proves only `ReadFileGrant`, not full Phase 02.
- [x] `eecfbb72` / `3bd8aaf0` evidence proves only the VFS-region fold / policy-signing slice, not full Phase 04.
- [x] Post-test status checked for generated binaries and rustfmt-only worktree noise.

## Success Criteria

- [x] `main` contains both reviewed closure slices as `3f6ad45d` and `3bd8aaf0`.
- [x] Targeted VFS grant evidence has serial PASS output and no `[FAIL] grant:`.
- [x] Boot-ceiling/policy self-tests pass after `3bd8aaf0`.
- [x] `git status --short --branch` was re-checked and the remaining dirty files were left as concurrent work, not used to block Phase 02 closure.

## Evidence

- `git log --oneline --decorate -12` → `3bd8aaf0 (HEAD -> main) feat(kernel): fold region admission and policy signing`; `3f6ad45d docs(vfs): finalize honest ReadFileGrant evidence`.
- `git status --short --branch` → `## main...origin/main [ahead 4]` with the current modified files listed in `.agents/reports/review-decision-phase02-integration-260805-200846.json`.
- `.agents/reports/review-decision-phase02-integration-260805-200846.json` → `git diff --check: exit 0`, `cargo fmt --all --check ... exit 0`, `python3 scripts/sign-policy.py --emit-rust: exit 0`, `git status --short --branch: main ahead 4 with one modified rustfmt-only file`.
- `.agents/reports/phase-06-vfs-handle-authority-260731.md` → `cargo fmt --all --check clean`, `cargo test -p api 61+2 pass`, `pwsh -NoProfile -File ./gen_disk.ps1 exit 0`, `qemu-boot-test.sh PASS: shell prompt reached`, `vfs-quota 1/1`, `redoxfs-srv 3/3`.
- `.agents/reports/phase-07-pinning-registry-260731.md` → `tests/integration --test vfs-quota --test-threads=1` ran for real after `build-test-hooks-ci.sh`; `no SKIP line, banner matched`.

## Security Considerations

Do not relax policy-required/signing-required semantics to land the commits. Treat `LookupService` openness (`kernel/src/task/syscall.rs:2269-2276`) as a known Phase 04 design constraint, not a reason to ship an ambient broker.

## Risk Notes

| Risk | Likelihood x Impact | Mitigation | Rollback |
|------|---------------------|------------|----------|
| Docs conflict after ADR commit | High x Medium | Integrate one commit at a time | Revert current cherry-pick before second slice |
| Runtime evidence fails from environment | Medium x Medium | Separate toolchain/env failure from feature regression | Revert slice or mark blocked with shortest failing line |
| Wrong branch deletion | Low x High | Exact ref + tree + ancestry checks immediately before deletion | Remote branch deletion cannot be fully undone without reflog; avoid unless verified |

## Backwards Compatibility

Both commits are intended as closures of existing Midori slices. Any ABI/API surface changes discovered during review must stop this phase and route through a Law-1 checkpoint.

## Deviation Log

2026-08-05: Phase 02 closure is limited to the landed slices (`3f6ad45d`, `3bd8aaf0`) and to Law 1 2/2 for the stack-only `GrantSlice` contract; `ReadGrant` producer work and fast-IPC `GetFile` proof are explicitly deferred to Phase 03.
