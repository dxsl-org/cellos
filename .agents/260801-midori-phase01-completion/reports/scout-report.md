---
title: "Midori Phase 01 Completion Scout"
description: "Verified codebase anchors for closing the Midori Phase 01 runtime-evidence gaps."
status: complete
created: 2026-08-01
---

# Scout Report

## Red-Team Revision

- Synthetic kernel fast-IPC handler/selftest evidence is rejected as Phase 01 completion evidence. It can prove a unit-shaped kernel mechanism, but not the shipped fast-IPC `GetFile` path.
- Actual fast-IPC `GetFile` runtime proof remains blocked by D1: direct dispatch is not reachable today and must not be restored by loader import bridge work in this plan.
- `ReadGrant` remains conditional on a real `HandleTable::insert_ro` source; absent one, the honest outcome is a recorded blocker.

## Relevant Files

- `.agents/260801-parallel-midori-closure/phase-01-p02-runtime-verify.md:54` leaves the Phase 01 criterion open: evidence must name all read-gated operations and fast-IPC `GetFile`.
- `.agents/debug/debug-260801-midori-phase01-verification.md:1` classifies Phase 01 as incomplete: QEMU proved ordinary IPC gates, not fast-IPC, `ReadGrant`, or `ReadFileGrant`.
- `docs/specs/17-ipc-wire-contract.md:441` states direct dispatch is an accepted Tier-1 rewrite but is not reachable today.
- `docs/specs/17-ipc-wire-contract.md:449` requires any future fast handler to authorize exactly like its `ecall` counterpart.
- `docs/specs/17-ipc-wire-contract.md:450` says `GetFile` is not a valid target for the future rewrite because `DataPtr` is permanent raw authority.
- `kernel/src/fast_ipc.rs:1` is the canonical kernel-owned fast-IPC table; `kernel/src/fast_ipc.rs:141` is the canonical `call_vfs` entry, but it is not reachable from separately linked cells today.
- `kernel/src/fast_ipc.rs:152` derives caller identity from `task::current_task_id`, but proving that with a synthetic current-task selftest is not accepted as Phase 01 completion evidence.
- `cells/services/vfs/src/main.rs:117` defines `vfs_fast_handler`; `cells/services/vfs/src/main.rs:122` denies unattributed callers with `Err(3)`.
- `cells/services/vfs/src/main.rs:125` serves only `GetFile`; `cells/services/vfs/src/main.rs:133` denies sealed or unauthorized callers with `Err(3)`.
- `libs/ostd/src/fast_ipc.rs:155` is the cell-local `call_vfs`; `libs/ostd/src/fast_ipc.rs:160` returns 0 when the private table is null.
- `cells/tests/vfs-test/src/main.rs:44` sends VFS requests through typed ordinary IPC; `cells/tests/vfs-test/src/main.rs:58` uses `sys_send`.
- `cells/tests/vfs-test/src/main.rs:24` currently allows only `Send`, `Recv`, `Log`, and `LookupService`, so grant tests need grant syscalls added.
- `cells/tests/vfs-test/src/dircap.rs:227` seals paths; `cells/tests/vfs-test/src/dircap.rs:254` verifies sealed denial for existing path-addressed operations.
- `libs/api/src/services/ipc.rs:78` defines `ReadFileGrant`; `libs/api/src/services/ipc.rs:173` marks it path-addressed.
- `cells/services/vfs/src/dispatch.rs:283` implements `ReadFileGrant` with read authorization before `GrantSlice`.
- `libs/api/src/services/ipc.rs:55` defines `ReadGrant`; `cells/services/vfs/src/dispatch.rs:209` implements it.
- `cells/services/vfs/src/handle_table.rs:56` is the only non-test method that seeds read handles, but observed callers are only in `#[cfg(test)]` unit tests at `cells/services/vfs/src/handle_table.rs:136`.
- `libs/ostd/src/fs.rs:358` constructs `VfsRequest::ReadGrant` from a kernel `OpenCap` id, but that id is not observed feeding VFS `HandleTable::insert_ro`.
- `tests/integration/tests/vfs-quota.rs:67` boots the test-hooks kernel; `tests/integration/tests/vfs-quota.rs:71` currently waits only for `[vfs-test] ALL TESTS PASSED`.

## Patterns

- Test-hook kernel selftests exist, but they are not a valid substitute for fast-IPC Phase 01 runtime evidence.
- Test-hooks are non-shipping and explicit: `kernel/src/main.rs:43` documents `qemu_exit` as `test-hooks` only; `kernel/src/layer2_selftest.rs:7` warns not to ship `test-hooks`.
- CI/QEMU lanes must fail loud instead of silently skipping: `tests/integration/src/lib.rs:69` implements `ci_guard`.
- Public API is sacred: `docs/code-standards.md` Law 1 forbids `libs/api/` and `libs/types/` changes without explicit confirmation.

## Precedents

- `7a525538` gated VFS reads on kernel-attested identity and touched VFS access, caller, dispatch, handle ownership, kernel fast IPC, syscall attestation, and API caller identity.
- `72f01d0d` fixed VFS destructive-op authorization and added runtime `vfs-test` coverage in `cells/tests/vfs-test/src/main.rs`.
- `8f9e3a16` established the broader security-test pattern: runtime gates in CI, QEMU evidence, selftests, test cells, and docs status updates.

## Prior Failures

- `.agents/failure-history.jsonl` was absent; no ledger entries were available.
- `.agents/incidents/` was absent; no incident read-back entries were available.
- Known prior false-green class: `.agents/260801-parallel-midori-closure/reports/midori-p02-runtime-verify-report.md:22` states fast-IPC remained unproven even though QEMU passed.

## Blast Radius

- VFS grant test only: `cells/tests/vfs-test/src/main.rs`, optional new `cells/tests/vfs-test/src/grant_io.rs`, `cells/tests/vfs-test/src/dircap.rs`.
- Conditional ReadGrant design: VFS internals only if a real handle source is found; never `libs/api/`, never `libs/types/`.
- Evidence and gates: `tests/integration/tests/vfs-quota.rs`, `.agents/260801-parallel-midori-closure/*`, docs status files if phase state changes.

## Inconsistencies And Debt

- `ReadGrant` is wired as a request/dispatch arm but appears to lack a production VFS handle source; testing it with a magic cap would be fake coverage.
- `ReadFileGrant` is a production request and path-addressed, so QEMU can honestly prove both nonzero grant copy and `Err(3)` after seal.
- Fast-IPC production reachability is intentionally absent after D1; Phase 01 remains partial until separate approved Tier-1 rewrite evidence exists or success criteria are explicitly rescoped.
- The required `node .claude/scripts/set-active-plan.cjs` sync script is absent in this repo; plan state could not be synced by that hook.
