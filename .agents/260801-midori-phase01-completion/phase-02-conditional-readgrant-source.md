---
phase: 2
title: "ReadGrant Source Verification"
status: blocked
priority: P1
effort: "6h"
dependencies: [1]
tier: thinking
---

# Phase 2: ReadGrant Source Verification

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs.

## Overview

Immediately verify whether `ReadGrant` can be covered honestly. Record it blocked unless a real existing `HandleTable::insert_ro` caller exists; do not design around the absence inside this plan.

## Requirements

- Functional: enumerate `HandleTable::insert_ro` callers and either prove a real source or mark `ReadGrant` blocked by missing handle source.
- Non-functional: no magic cap sentinel, no fake path, no `libs/api`, no public syscall contract change, no overloading a production request with test-only semantics.
- Backward compatibility: existing `ReadGrant` behavior for unknown or wrong-owner cap remains `GrantDone { bytes: 0 }` from `cells/services/vfs/src/dispatch.rs:249`.

## Architecture

Observed flow:

1. `libs/ostd/src/fs.rs:358` sends `ReadGrant { cap, offset, size, grant }` using a cap from `sys_open_cap`.
2. VFS dispatch re-authorizes by looking up the cap path in `vfs.handles.path_of` at `cells/services/vfs/src/dispatch.rs:221`.
3. VFS copies bytes only when `vfs.handles.get_mut` finds a caller-owned entry at `cells/services/vfs/src/dispatch.rs:236`.
4. The only observed seeding method is `HandleTable::insert_ro` at `cells/services/vfs/src/handle_table.rs:56`; observed calls are unit-test fixtures at `cells/services/vfs/src/handle_table.rs:136`.

Required real-source bar:

- A pre-existing production-internal or test-internal path must call `HandleTable::insert_ro` with real path, data pointer/length from `VfsManager::get_file_ptr` at `cells/services/vfs/src/manager.rs:85`, real caller identity, and a real cap id.
- If the only callers are unit-test fixtures, `ReadGrant` runtime coverage is blocked.

## Assumptions

None — no unverified ReadGrant source may be assumed.

## Related Files

- Modify: `.agents/260801-parallel-midori-closure/reports/midori-p02-runtime-verify-report.md`
- Modify: `.agents/260801-parallel-midori-closure/phase-01-p02-runtime-verify.md`
- Conditional Modify only if real source already exists: `cells/tests/vfs-test/src/grant_io.rs`

## Implementation Steps

1. Run source verification before any source edit: `rg -n "insert_ro|ReadGrant|sys_open_cap|OpenCap" cells/services/vfs libs/ostd kernel/src`.
2. List every `insert_ro` caller in the Phase 01 report with file:line citations.
3. If all callers are tests/fixtures, do not edit VFS behavior and record: `ReadGrant runtime coverage blocked: VFS HandleTable has no real source feeding insert_ro`.
4. If a real source already exists, add only the matching QEMU test: obtain that real handle, allocate/share grant, request `ReadGrant`, assert `GrantDone { bytes > 0 }`, and assert bytes match.
5. Also assert unknown or not-owned cap returns `GrantDone { bytes: 0 }` without copying, preserving the indistinguishable failure contract.

## Success Criteria

- [ ] `.agents/260801-parallel-midori-closure/reports/midori-p02-runtime-verify-report.md` records the `insert_ro` caller list.
- [ ] Either QEMU serial contains `[PASS] grant: ReadGrant copies nonzero bytes from a real VFS handle`, or the report records the explicit blocker.
- [ ] No magic cap constant or hidden test sentinel is introduced.
- [ ] `git diff -- libs/api libs/types` is empty.

## Security Considerations

- Risk High x High: fake handle population would claim coverage while bypassing the actual authority model. Mitigation: hard stop unless source is file/line-verifiable.
- Risk Medium x High: leaking another caller's cap existence. Mitigation: keep unknown and wrong-owner behavior indistinguishable as zero bytes.
- Risk Medium x Medium: re-authorizing stale handles omitted. Mitigation: retain existing `path_of` check before copying.

## Test Matrix

- Source verification: `rg -n "insert_ro|ReadGrant|sys_open_cap|OpenCap" cells/services/vfs libs/ostd kernel/src`
- If unblocked by an existing real source: same QEMU gate as Phase 1 plus marker wait.
- If blocked: no runtime test added; evidence/status update is the deliverable.

## Risk Notes

- Rollback: revert any conditional VFS/test edits; blocker report can be superseded by a future source-design plan.
- Cannot undo: none, if stopped before fake implementation.
- Stop gate: any design needing new VFS handle-source wiring, new `VfsRequest` variants, new public syscalls, or `libs/api` changes is outside this plan and must become a separate approved feature.

## Deviation Log

- 2026-08-01 — Source verification found no real `HandleTable::insert_ro` source in scope. The only caller remains the unit-test fixture at `cells/services/vfs/src/handle_table.rs:136`, so this phase records `ReadGrant` blocked instead of designing a synthetic seeding path.
