# Solution Design — Simplicity-First

Verdict: choose **bounded copy-out first**, with a file-handle endpoint only after live callers stop depending on `GetFile/DataPtr`; defer revocable `ReadGrant` until a real VFS handle producer and service-side pin submit/complete exist.

## Evidence Baseline

- Law 1 blocks casual ABI edits in `libs/api/` and `libs/types/`; any VFS request/response change needs 2x user confirmation (`docs/code-standards.md:12`).
- `VfsRequest` already exposes `GetFile`, `ReadGrant`, `ReadFileGrant`, and directory-cap ops in one public enum (`libs/api/src/services/ipc.rs:27`).
- `GetFile` returns a raw `DataPtr` after authorization; the code states the pointer is permanent read authority in SAS (`cells/services/vfs/src/dispatch.rs:55`).
- Current `ReadFileGrant` is bounded by `min(file_len, max, grant_len)` and already backs spawn-time full-file reads (`cells/services/vfs/src/dispatch.rs:292`, `libs/ostd/src/fs.rs:266`).
- `ReadGrant` rechecks handle owner/path but still has no non-test `HandleTable::insert_ro` producer in the VFS tree; the only observed `insert_ro` hits are tests (`cells/services/vfs/src/handle_table.rs:55`).
- VFS durable state is already generation-scoped through `Caller { cell, generation }` (`cells/services/vfs/src/caller.rs:15`), and Spec 17 requires generation comparison for service-held handles/pending reads (`docs/specs/17-ipc-wire-contract.md:421`).
- Fast `GetFile` now derives caller identity from scheduler state and requires the VFS fast handler to authorize identically (`kernel/src/fast_ipc.rs:126`, `cells/services/vfs/src/main.rs:97`).
- Async Pinning Registry exists for grant/DMA lifetimes: pinned regions refuse free/unregister and dead-owner frames quarantine until acknowledgement (`kernel/src/memory/pin.rs:1`, `kernel/src/task/syscall.rs:201`, `kernel/src/task/syscall.rs:4167`).
- `DataPtr` is explicitly unrepresentable across Tier 2 and must be removed or replaced before Layer B (`docs/specs/17-ipc-wire-contract.md:449`, `docs/specs/18-cell-trust-tiers.md:155`, `docs/specs/19-hardware-isolation-layers.md:62`).
- `docs/coding.md` was requested but is absent in this checkout; this report applies `docs/code-standards.md` Law 1 instead.

## Option Scores

Scale: 5 is best. Simplicity-first weights Complexity, Fit, and Blast Radius highest.

| Option | Blast Radius | Reversibility | Complexity | Fit | Security | Performance | Total |
|---|---:|---:|---:|---:|---:|---:|---:|
| A. Bounded copy-out (`ReadFileGrant` / inline `ReadAsync+Poll`) | 5 | 5 | 5 | 5 | 3 | 3 | 26 |
| B. File handle + bounded read (`OpenAt`/`ReadAt`/`Close`) | 3 | 4 | 3 | 4 | 5 | 4 | 23 |
| C. Revocable `Grant`/`ReadGrant` first | 2 | 3 | 2 | 2 | 4 | 5 | 18 |

## Chosen Approach

Implement the narrow boundary as **no new raw-pointer reads, migrate live `GetFile` consumers to bounded copy-out, then introduce one minimal file-handle open/read/close path only where caller-side streaming needs it**. Do not make `ReadGrant` the first vehicle; it is currently a protocol arm without a real production VFS handle source.

This is the smallest safe diff because it reuses proven surfaces before adding state:

1. Consumer migration: shell, Lua, and WASM stop consuming `DataPtr` directly and use bounded copy-out wrappers (`cells/tools/shell/src/cmd_fs.rs:336`, `cells/runtimes/lua/src/bindings_vfs.rs:48`, `cells/tools/wasm/src/main.rs:95`).
2. Existing server path: `ReadFileGrant` stays synchronous and bounded for known-size whole-file reads (`cells/services/vfs/src/dispatch.rs:292`).
3. Existing generation owner model: new durable state, if any, uses `Caller` exactly like pending and dir handles (`cells/services/vfs/src/pending.rs:26`, `cells/services/vfs/src/dirs/lifecycle.rs:51`).
4. Endpoint: a later Law-1 handle read path can wrap directory-cap resolution or a VFS-local file table, but only after the consumer migration proves no broad raw-pointer dependency remains.

## Data Flow

Bounded copy-out flow:

`caller path request -> kernel-attested Caller -> VFS can_read + seal check -> resolve VFS mount/overlay -> copy into reply buffer or caller-owned grant -> typed byte count -> caller copies from its own buffer/grant -> caller frees grant`.

File-handle flow:

`OpenAt/OpenFile -> VFS resolves under dir handle/path policy -> HandleEntry { owner: Caller, path, position } -> ReadHandle rechecks owner + current path policy -> bounded copy into reply/grant -> Close removes only owner-held handle`.

Revocation flow:

`Close/Revoke/Seal/CellDeath -> owned handle lookup by Caller -> remove root and derived entries -> pending reads for that owner denied or purged on next generation contact -> grants pinned only while memory may outlive synchronous reply -> dead-owner frames quarantine until ack`.

## State Machine

`Unseen -> Admitted -> PathAllowed | Sealed -> Open -> InFlightRead -> Open -> Closed`

- `Unseen`: fast path must decline and force ecall admission (`cells/services/vfs/src/main.rs:107`).
- `Admitted`: `DirTable::on_contact` records generation and purges predecessor state on higher generation (`cells/services/vfs/src/dirs/lifecycle.rs:51`).
- `Sealed`: all path-addressed requests are refused before dispatch (`cells/services/vfs/src/dispatch.rs:50`).
- `Open`: durable state requires `caller.may_own_state()` (`cells/services/vfs/src/caller.rs:42`).
- `InFlightRead`: synchronous copy-out relies on caller blocked until reply; async/cancellable variants must pin first.
- `Closed/Reaped`: close/revoke owner-only; successor generation reaches no predecessor state (`cells/services/vfs/src/dirs/lifecycle.rs:110`).

## Owner, Scope, Lifetime

- Owner: always kernel-attested `Caller`, never sender tid or request payload (`docs/specs/17-ipc-wire-contract.md:421`).
- Scope: VFS path or directory handle; no global namespace widening. Inherited dir sets are all-or-nothing and seal the child on bind/refusal (`cells/services/vfs/src/dir_admission.rs:22`, `cells/services/vfs/src/dirs/bind.rs:23`).
- Lifetime: synchronous copy-out authority ends at reply; grants end when caller frees after `GrantDone`; handles end on close/revoke/cell generation advance.
- Close/revoke: non-owner sees the same failure as unknown handle; sweeping handle IDs must not be an oracle (`cells/services/vfs/src/handle_table.rs:77`, `cells/services/vfs/src/pending.rs:85`).
- Cell death: kernel caps already revoke on exit/force-exit by cell id; VFS generation prevents successor inheritance, but a direct VFS death hook for pruning open/pending state is a later hardening step, not required for the first copy-out migration.

## Fast Path Identity

Any retained fast VFS path must keep the current rule: identity is derived from live scheduler state in `kernel::fast_ipc::call_vfs`, not passed by the caller (`kernel/src/fast_ipc.rs:126`). Fast path must never be the migration vehicle for revocable access while serving `DataPtr`; Spec 17 says `GetFile` is invalid as a Tier-2 rewrite target (`docs/specs/17-ipc-wire-contract.md:449`).

## Async Pinning Boundary

- Use the Async Pinning Registry only when target memory can outlive the synchronous reply.
- Do not pin for ordinary bounded copy-out that completes before reply; that would add state without buying revocation.
- If `ReadGrant` becomes cancellable/async, VFS must submit a pin before copying, clear it only after completion/ack, and route owner death through quarantine. Otherwise a cancelled caller can free a grant while VFS still writes into it (`cells/services/vfs/src/dispatch.rs:305`, `kernel/src/memory/pin.rs:6`).
- Stale-frame defense is pin refusal + quarantine + explicit acknowledge, never a timer (`kernel/src/memory/pin.rs:23`).

## Lock Ordering

- VFS tables stay service-local and single-threaded under the main loop unless fast path takes `GLOBAL_VFS` (`cells/services/vfs/src/main.rs:93`).
- Kernel grant teardown order remains `PAGE_GRANT_TABLE/REG_GRANT_TABLE -> PIN_TABLE leaf -> release -> FRAME_ALLOCATOR -> KERNEL_ROOT` (`kernel/src/memory/pin.rs:28`, `kernel/src/task/syscall.rs:242`).
- No design step may hold `SCHEDULER` across frame allocator or VFS service locks; reaping is already deferred outside scheduler lock (`kernel/src/task/scheduler.rs:670`).

## Hard Scope Stops

- Do not add Tier-2 page tables, ASID switching, grant mapping across domains, or installer tier choices.
- Do not add a new async VFS reactor or cancellable VFS grant reads in the first slice.
- Do not retrofit revocation onto `DataPtr`; remove or bypass it for migrated callers.
- Do not widen `VfsRequest` without Law 1 confirmation and Spec 17 amendment.
- Do not claim `ReadGrant` production readiness until `HandleTable::insert_ro` has a real non-test opener and close path.

## Phase Shape and Dependencies

1. **Copy-out migration**: own shell/Lua/WASM/ostd wrappers only; depends on existing `ReadFileGrant`, `ReadAsync`, and tests. Undo: restore callers to `GetFile`; cannot undo removal of unsafe consumer assumptions from docs/tests if already ratified.
2. **Server guardrails**: own VFS dispatch/tests only; assert `GetFile` remains gated, sealed path refusal stays first, bounded grant clamp stays tested. Undo: revert VFS dispatch/test changes.
3. **Minimal handle endpoint**: own `libs/api` VFS enum, `dispatch.rs`, `handle_table.rs`, ostd wrapper; depends on Law 1 confirmation and Phase 1 evidence. Undo: leave old bounded copy-out callers in place and revert new enum arms; public ABI amendment cannot be silently unshipped once external consumers depend on it.
4. **Async pin integration for revocable grants**: own kernel pin submit/complete hooks and VFS grant arms; depends on a real handle opener and cancellation semantics. Undo: disable async `ReadGrant` route and keep synchronous copy-out.

## Test Matrix

- Unit: `Caller` generation equality, `PendingTable` wrong-owner poll, `HandleTable` wrong-owner get/remove, dir revoke transitivity.
- Integration/QEMU: `ReadFileGrant` clamp/nonzero/deny after seal; `GetFile` denied after seal; migrated shell/Lua/WASM read fixture without `DataPtr`.
- Negative: same `CellId` higher generation cannot poll/read predecessor handle; unknown fast-path caller declines to ecall; `ReadGrant` unknown cap remains zero/fail-closed.
- Pinning: grant free/unregister refused while pinned; dead owner quarantines frames; acknowledge releases exactly then.
- Performance: compare small-file read latency before/after migration and ensure no extra round trip for known-size whole-file grant reads.

## Top Risks

- High x Medium: migrating shell/Lua/WASM may expose size assumptions hidden by raw pointers. Mitigation: use `Stat` + bounded grant for known-size reads; inline `ReadAsync+Poll` only for <=480 B.
- Medium x High: Law 1 handle endpoint renumbers or widens ABI incorrectly. Mitigation: append variants only, amend Spec 17, 2x confirmation, wire-level encode/decode tests.
- Medium x High: synchronous grant safety is accidentally generalized to cancellable async. Mitigation: hard gate async `ReadGrant` behind pin submit/ack tests.
- Low x High: same-SAS migration leaves `GetFile` reachable in one caller. Mitigation: grep for `DataPtr` consumers and fail CI while any production consumer remains outside VFS/tests/bench.

## Success Criteria

- No production caller outside VFS/tests/bench decodes `VfsResponse::DataPtr`.
- All migrated reads have an explicit byte bound from stat, grant length, reply limit, or caller buffer length.
- Existing seal/deny and `ReadFileGrant` clamp tests still pass.
- Any new durable VFS handle stores and compares `Caller { cell, generation }`.
- No new public ABI change lands without Law 1 checkpoint evidence.
