---
phase: 1
title: "ReadFileGrant QEMU Evidence"
status: completed
priority: P1
effort: "6h"
dependencies: []
tier: medium
---

# Phase 1: ReadFileGrant QEMU Evidence

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs.

## Overview

Add honest QEMU-visible proof that production `ReadFileGrant` copies nonzero bytes before seal and denies with `Err(3)` after seal.

## Requirements

- Functional: print markers for `ReadFileGrant` nonzero copy and `ReadFileGrant` `Err(3)` after seal.
- Non-functional: no fast-IPC proof, no kernel selftest, no `libs/api`, no `libs/types`, no public syscall, no loader import bridge.
- Compatibility: production builds remain unchanged; grant tests are limited to the `test-hooks` VFS test cell lane.

## Architecture

Data flow:

1. `cells/tests/vfs-test` adds grant syscalls to its test-hooks allowlist from `cells/tests/vfs-test/src/main.rs:24`.
2. Before `dircap::run`, the test allocates and shares a grant to the VFS tid resolved by `vfs_tid()` at `cells/tests/vfs-test/src/main.rs:29`.
3. It sends `VfsRequest::ReadFileGrant` from `libs/api/src/services/ipc.rs:78`.
4. VFS authorizes and copies in `cells/services/vfs/src/dispatch.rs:283`.
5. After `SealPaths` in `cells/tests/vfs-test/src/dircap.rs:227`, it sends another `ReadFileGrant`; because `is_path_addressed` includes it at `libs/api/src/services/ipc.rs:173`, VFS must return `Err(3)`.

## Assumptions

- Claim: `test-hooks` vfs-test may add grant syscalls without public contract change.
  Confidence: medium
  How to verify: compile `app-vfs-test --features test-hooks`.
- Claim: grant helper code can be kept under `cells/tests/vfs-test` without touching `libs/api`.
  Confidence: high
  How to verify: `git diff -- libs/api libs/types` stays empty.

## Related Files

- Modify: `cells/tests/vfs-test/src/main.rs`
- Create: `cells/tests/vfs-test/src/grant_io.rs`
- Modify: `cells/tests/vfs-test/src/dircap.rs`

## Implementation Steps

1. Add `GrantAlloc`, `GrantShare`, `GrantSlice`, and `GrantFree` to the VFS test cell allowlist only.
2. Extract grant helpers into `grant_io.rs`: allocate, share to VFS, send typed request, inspect buffer, free on every return path.
3. Add a pre-seal `ReadFileGrant` scenario that writes a known `/tmp` file, reads it through a grant, checks `GrantDone { bytes > 0 }`, and checks copied bytes match.
4. Extend the final dircap sealed section to assert `ReadFileGrant` returns `Err(3)` after `SealPaths`; preserve the current "runs last" invariant from `cells/tests/vfs-test/src/dircap.rs:11`.
5. Ensure denied-after-seal leaves the grant buffer unchanged.

## Success Criteria

- [ ] QEMU serial contains `[PASS] grant: ReadFileGrant copies nonzero bytes`.
- [ ] QEMU serial contains `[PASS] grant: ReadFileGrant is refused after sealing`.
- [ ] QEMU serial contains no `[FAIL] grant:` marker.
- [ ] `git diff -- libs/api libs/types kernel/src` is empty for this phase.

## Security Considerations

- Risk Medium x High: grant buffer freed before VFS reply. Mitigation: helper frees only after `GrantDone` or after immediate error response.
- Risk Medium x Medium: sealed denial only checks response, not side effects. Mitigation: verify denied grant buffer remains unchanged.
- Risk Low x Medium: adding grant syscalls to the test cell broadens its authority. Mitigation: only the test-hooks test cell receives them.

## Test Matrix

- Unit/build: `cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -p app-vfs-test --features test-hooks`
- Kernel build: `bash scripts/build-test-hooks-ci.sh`
- QEMU integration: `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`

## Risk Notes

- Rollback: revert the VFS test-cell files; no data migration.
- Cannot undo: QEMU evidence logs already written to reports, but they can be superseded.
- Stop gate: if `ReadFileGrant` needs API or syscall changes to test, stop; that contradicts this phase.

## Deviation Log

- 2026-08-01 — Baseline commands required `wsl bash -lc` from the Windows host. `bash scripts/build-test-hooks-ci.sh` failed from the PowerShell host path with `cargo: command not found`, and the Linux-target integration command failed there with missing host-target std; both commands run correctly inside WSL against `/home/dmin/cellos/.worktrees/midori-phase01-evidence`.
- 2026-08-01 — Kept the test cell manifest minimal after follow-up direction: the pre-seal proof uses `GrantDone { bytes == Stat.size && bytes > 0 }`, and the post-seal proof uses `ReadFileGrant` with invalid grant `0` to show `SealPaths` denies before any grant lookup or copy. No spawn-cap widening and no new unsafe helper were introduced.
