---
phase: 3
title: "Amend Phase 02 Runtime Closure Criteria"
status: completed
priority: P1
effort: 1d
dependencies: [2]
tier: thinking
---

# Phase 03: Amend Phase 02 Runtime Closure Criteria

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Replace the invalid Phase 02 tail assumptions with an explicit success-criteria amendment. User approval arrived on 2026-08-05, so this phase also lands the missing governed message-path `GetFile` proof in the existing `vfs-test` lane and updates the original Midori status artifacts in the same change.

Verdict: **Phase 02 is runtime-closed under user-approved amended criteria.** The retained proof set is governed message-path `GetFile` before `SealPaths`, governed message-path `GetFile` denial after `SealPaths`, `ReadFileGrant` clamp/nonzero/deny evidence, and current fail-closed `ReadGrant` zero-byte behavior only.

## Requirements

- Functional: amend Phase 02 success criteria to retain only observable governed message-path behavior:
  - message-path `GetFile` positive proof before seal;
  - message-path `GetFile` denial after seal;
  - `ReadFileGrant` positive, clamp, and post-seal denial proof;
  - `ReadGrant` remains fail-closed/zero-byte only until a producer exists.
- Functional: formally rescope `ReadGrant` production into a future `OpenAt`/file-handle/close design. A real producer is not a small follow-up because kernel `CAP_TABLE` owns `OpenCap`/`ReadCap`/`CloseCap`, while VFS `HandleTable::insert_ro` is a separate service table with no production caller.
- Functional: formally rescope Tier-1 direct fast-IPC dispatch into a future design prerequisite. The current callable fast path is documented as unavailable/fallback when not in the kernel-owned bridge, not as a runtime authorization proof.
- Non-functional: do not claim `DataPtr` as Tier-2 safe; Spec 18 says raw `DataPtr`-style pointers are unrepresentable across Tier-2 boundaries (`docs/specs/18-cell-trust-tiers.md:151-156`).
- Scope: update only the owned test/status artifacts for this closure slice. No kernel, `libs/api`, or transport-surface edits in this phase.

## Architecture

### Data flows

- Message-path `GetFile`: caller sends `VfsRequest::GetFile` -> VFS `dispatch` checks seal/access before resolving bytes -> VFS returns `DataPtr` or denial (`cells/services/vfs/src/dispatch.rs:55-63`, `cells/tests/vfs-test/src/dircap.rs:290-295`).
- `ReadFileGrant`: caller registers a grant -> sends path + grant -> VFS copies bounded bytes before seal and denies path access after seal (`cells/services/vfs/src/dispatch.rs:292-306`, `cells/tests/vfs-test/src/dircap.rs:241-256`, `cells/tests/vfs-test/src/dircap.rs:327-335`).
- Current `ReadGrant`: caller sends `cap` + grant -> VFS asks `HandleTable::path_of`/`get_mut`; if cap is unknown or owned by another caller, it returns zero bytes (`cells/services/vfs/src/dispatch.rs:209-259`). `HandleTable::insert_ro` exists only as a service-local table operation (`cells/services/vfs/src/handle_table.rs:56-65`).
- Kernel cap path: `OpenCap` allocates in kernel `CAP_TABLE`; `ReadCap` parks/unparks kernel file state; `CloseCap` revokes the kernel cap (`kernel/src/task/syscall.rs:2764-2808`, `kernel/src/task/syscall.rs:2830-2845`, `kernel/src/task/syscall.rs:3057-3071`, `kernel/src/cell/cap_registry.rs:79-112`, `kernel/src/cell/cap_registry.rs:257`). This does not populate VFS `HandleTable`.
- Fast-IPC: the VFS cell registers an `ostd` handler copy (`cells/services/vfs/src/main.rs:161-164`), but kernel fast-IPC documents the rejected relocation gap: direct cell calls use their own copy of `VFS_HANDLER_PTR`, so `call_vfs` in shell reads zero and falls back unless a future Tier-1 kernel bridge is built (`kernel/src/fast_ipc.rs:121-135`).

## Assumptions

- **OBSERVED:** `ReadGrant` has no production producer in current VFS because `insert_ro` is not called outside tests (`cells/services/vfs/src/handle_table.rs:56-65`, `cells/services/vfs/src/handle_table.rs:134-136`) while production file caps are kernel `CAP_TABLE` entries (`kernel/src/task/syscall.rs:2800-2808`).
- **OBSERVED:** direct fast-IPC `GetFile` cannot be used as Phase 02 runtime proof under the current D1/Spec17 model because the kernel bridge is explicitly future-facing and the current cell-local copy falls back (`kernel/src/fast_ipc.rs:121-135`).
- **OBSERVED:** `DataPtr` cannot be called a Tier-2 target (`docs/specs/18-cell-trust-tiers.md:151-156`, `.agents/260727-2101-midori-lessons-cellos/phase-06-directory-capabilities.md:282`).

## Related Files

- Modify: `cells/tests/vfs-test/src/dircap.rs`
- Modify: `tests/integration/tests/vfs-quota.rs`
- Modify: `.agents/260805-1833-midori-closure-execution/phase-03-runtime-close-phase-02.md`
- Modify: `.agents/260805-1833-midori-closure-execution/plan.md`
- Modify: `.agents/260727-2101-midori-lessons-cellos/phase-02-vfs-read-gating.md`
- Modify: `.agents/260727-2101-midori-lessons-cellos/plan.md`
- Modify: `docs/project-roadmap.md`
- Modify: `docs/project-changelog.md`

## Implementation Steps

1. Record the invalidated assumptions:
   - `ReadGrant` producer requires a Law 1 file-handle API design, not a VFS-only patch.
   - Fast-IPC `GetFile` cross-cell runtime proof requires the rejected/future kernel bridge or a new transport.
   - `GetFile`/`DataPtr` cannot be promoted to Tier-2.
2. Amend Phase 02 criteria in this plan:
   - retained: governed message-path `GetFile` positive before seal and denied after seal;
   - retained: `ReadFileGrant` positive/clamp/deny evidence;
   - retained: `ReadGrant` fail-closed zero-byte behavior only;
   - removed from Phase 02 closure: real `ReadGrant` producer and Tier-1 direct fast dispatch.
3. Capture the user-approved runtime proof in the owned QEMU lane:
   - add a pre-seal `GetFile("/tmp/volatile.txt")` assertion with non-null `DataPtr` and the expected length; this is response-metadata proof only because the safe test crate must not dereference the raw pointer;
   - make the integration harness wait for the new PASS marker.
4. Update the original Midori Phase 02 status to "runtime-closed under amended criteria" and add the evidence/rescope map in the same change.

## Todo List

- [x] Criteria amendment is written in this closure plan.
- [x] `ReadGrant` producer is explicitly moved to future Law 1 `OpenAt`/file-handle/close design.
- [x] Tier-1 direct fast-IPC dispatch is explicitly moved to future bridge/transport design.
- [x] User approval checkpoint is explicit before touching original Midori Phase 02 status.
- [x] Owned QEMU evidence now proves governed message-path `GetFile` positive before `SealPaths`.

## Success Criteria

- [x] This plan no longer requires implementing `ReadGrant` producer or fast-IPC direct dispatch to close Phase 02.
- [x] The amended done definition is measurable from rerunnable QEMU evidence.
- [x] Original Midori Phase 02 status changed only after the user approved this amendment on 2026-08-05.
- [x] No `DataPtr` path is represented as Tier-2 safe.

## Security Considerations

Unknown identity must deny. Do not derive cell identity from tid; the Midori validation records kernel-attested identity as the required fix (`.agents/260727-2101-midori-lessons-cellos/plan.md:269`). Do not turn a zero-byte fast-path fallback into an authorization proof. Do not claim revocation semantics for a raw `DataPtr`.

## Test Matrix

| Evidence target | Validation |
|-----------------|------------|
| Message-path `GetFile` positive before seal | `cells/tests/vfs-test/src/dircap.rs` emits `dircap: GetFile returns a nonempty pointer before sealing`; `tests/integration/tests/vfs-quota.rs` waits for the nonzero-pointer + expected-length metadata proof |
| Message-path `GetFile` deny after seal | `cells/tests/vfs-test/src/dircap.rs` post-seal denial loop |
| `ReadFileGrant` positive and clamp | `cells/tests/vfs-test/src/dircap.rs` clamp + nonzero grant assertions |
| `ReadFileGrant` deny after seal | `cells/tests/vfs-test/src/dircap.rs` post-seal `ReadFileGrant` denial |
| `ReadGrant` current behavior | `cells/tests/vfs-test/src/grant_io.rs` now probes an intentionally unknown cap ID with a valid shared grant and proves `GrantDone { bytes: 0 }` / no fault only, not a real producer |
| Fast-IPC direct dispatch | Optional negative/fallback probe only; no Phase 02 positive proof claim |

## Risk Notes

| Risk | Likelihood x Impact | Mitigation | Rollback |
|------|---------------------|------------|----------|
| False-green Phase 02 | High x High | Require explicit user approval before original status update | Revert status text only; no product code changed |
| Shipping hidden ABI design under "closure" | High x High | Rescope `OpenAt`/file-handle/close to separate Law 1 plan | No irreversible change in this phase |
| Treating fast fallback as security proof | Medium x High | Fast probe may prove unavailable/fallback only | Remove fallback proof claim |
| `GetFile` raw pointer survives into Tier-2 path | Medium x High | Mark as same-SAS temporary and link to Spec 18 | No implementation in this phase; later code change must remove or time-bound it |

## Backwards Compatibility

No runtime behavior changes. Existing same-SAS VFS clients keep working because this phase changes only planning artifacts. Any later `libs/api` change for `OpenAt`/file handles requires Law 1 two confirmations (`docs/code-standards.md:12-18`).

## Approval Checkpoint

Approved 2026-08-05:

> Amend Midori Phase 02 runtime closure to exclude the real `ReadGrant` producer and Tier-1 direct fast-IPC proof, while retaining governed message-path `GetFile`, `ReadFileGrant`, and current fail-closed `ReadGrant` evidence.

## Deviation Log

2026-08-05: Replaced "implement missing producer/proof" with an amendment checkpoint after verifying `ReadGrant` production and fast-IPC direct dispatch are separate design prerequisites, not small Phase 02 closure tasks.
2026-08-05: User approved the amendment, so Phase 03 scope widened from docs-only to the owned `vfs-test` and integration harness files needed to add the missing governed message-path `GetFile` runtime marker.
