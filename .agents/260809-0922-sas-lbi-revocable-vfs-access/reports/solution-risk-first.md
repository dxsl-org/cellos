# Solution Design — Risk-First Revocable VFS Boundary

## Verdict

Choose a staged **bounded-copy-out first, handle-addressed second, revocable grant-read last** design. The top risk is not throughput; it is stale SAS authority surviving close/revoke/death. `GetFile/DataPtr` must be treated as the hard stop because the current spec calls it permanent, unrevocable SAS read authority that cannot cross Tier 2 (`docs/specs/17-ipc-wire-contract.md:449`, `docs/specs/18-cell-trust-tiers.md:155`).

## Approach Scores

| Approach | Blast Radius | Reversibility | Complexity | Fit | Security | Performance | Verdict |
|---|---:|---:|---:|---:|---:|---:|---|
| Bounded copy-out (`ReadFileGrant`) | 4 | 4 | 4 | 5 | 3 | 3 | First migration step |
| File handle + bounded read | 3 | 4 | 3 | 4 | 4 | 4 | Endpoint for VFS semantics |
| Revocable `Grant`/`ReadGrant` | 2 | 2 | 2 | 3 | 5 | 5 | Only after live open/pin/revoke substrate |

Scoring: 5 is best. Risk-first weighting favors low blast radius and rollback over peak performance.

## Comparison

- **Bounded copy-out wins first:** `ReadFileGrant` already exists in `VfsRequest` and copies `min(file_len, max, grant_len)` (`libs/api/src/services/ipc.rs:71`, `cells/services/vfs/src/dispatch.rs:292`). It is used by spawn's VFS route (`libs/ostd/src/fs.rs:266`) and has QEMU-era proof hooks for nonzero copy, clamp, and deny-after-seal (`cells/tests/vfs-test/src/grant_io.rs:62`). Failure mode: path-addressed ambient naming remains until sealed; mitigation: use only as `GetFile` replacement, not as capability endpoint.
- **File handle + bounded read is the narrow endpoint:** current VFS durable state already stores `Caller { cell, generation }` and refuses unknown/wrong-owner handles (`cells/services/vfs/src/caller.rs:15`, `cells/services/vfs/src/handle_table.rs:77`, `cells/services/vfs/src/pending.rs:85`). Failure mode: open-time policy staleness; mitigation: re-authorize current path on every `Poll`/`ReadGrant` as current code does (`cells/services/vfs/src/dispatch.rs:181`, `cells/services/vfs/src/dispatch.rs:215`).
- **Revocable `ReadGrant` is not first:** current `ReadGrant` is synchronous and depends on an already-open VFS handle (`cells/services/vfs/src/dispatch.rs:209`), while prior research found no production `HandleTable::insert_ro` producer. Failure mode: false completion claim; mitigation: do not make `ReadGrant` the migration source until a real VFS open/close/revoke producer exists.

## Selected Design

1. **Deprecation boundary:** stop adding new `GetFile` consumers immediately. Convert existing whole-file reads to bounded copy-out where whole-file behavior is intended.
2. **VFS file handle:** add/finish a Law-1 VFS `OpenFileAt`/`ReadFileAt`/`CloseFile` family only after the exact ABI is confirmed 2x because `VfsRequest` lives in `libs/api/src/services/ipc.rs:25` and Law 1 governs `libs/api` (`docs/code-standards.md:12`).
3. **Owner model:** every durable VFS file handle is owned by `Caller { cell, generation }`; no sender-tid or path-hint identity. Spec 17 requires absent identity to deny and generation comparison for service-held state (`docs/specs/17-ipc-wire-contract.md:421`, `docs/specs/17-ipc-wire-contract.md:430`).
4. **Scope model:** directory handles remain the namespace authority; file handles are derived from a directory handle plus relative name. A sealed/handle-only cell must be unable to express path-addressed reads because `is_path_addressed()` refuses those arms (`libs/api/src/services/ipc.rs:155`, `cells/services/vfs/src/dispatch.rs:46`).
5. **Lifetime model:** open creates `Open`; bounded read borrows only for the synchronous reply; async or cancellable read pins target memory first; close/revoke moves to terminal state; cell death reaps all owned state.

## State Machine

`Unbound -> Open -> InFlightRead -> Open -> Closing -> Closed`

Terminal side paths:

- `Open/InFlightRead + Revoke(owner)` -> `Revoking`: mark handle not usable, allow only completion/ack drain.
- `Open/InFlightRead + Close(owner)` -> `Closing`: refuse new reads, drain or cancel existing read.
- `Any + CellDeath(cell,generation)` -> `Reaped`: delete VFS handles/pending reads; revoke kernel caps; quarantine pinned frames.
- `InFlightRead + Cancel` -> either no memory escaped the sync reply, or frames stay pinned until service/driver acknowledgement. No third mode.

## Owner, Scope, Lifetime

- **Owner:** `Caller { cell, generation }` from kernel attestation only (`cells/services/vfs/src/main.rs:191`, `cells/services/vfs/src/caller.rs:31`).
- **Scope:** directory handle root plus relative name; reject absolute, empty, slash-containing, and `..` names at VFS resolve. Do not normalize first.
- **Lifetime:** file handles live until explicit close, owner revoke, service policy revoke, or cell-generation death. Child handles are new entries, not aliases, so parent/child revoke semantics can be explicit.
- **Same-SAS migration:** within Tier 1, replace raw pointers with copy-out first. Before Layer B/Tier 2, no `DataPtr` may remain on a cross-domain path (`docs/specs/17-ipc-wire-contract.md:454`, `docs/specs/19-hardware-isolation-layers.md:62`).

## Close, Revoke, Death

- `CloseFile(handle)`: owner-only; unknown and wrong-owner are indistinguishable, matching existing handle-table behavior (`cells/services/vfs/src/handle_table.rs:80`).
- `Revoke(cell,generation or handle)`: service-side administrative path marks handles dead before freeing backing state.
- `Cell death`: kernel already revokes `CAP_TABLE` entries on `Exit`/`ForceExit` (`kernel/src/task/syscall.rs:2055`, `kernel/src/task/syscall.rs:2151`) and grant reaping quarantines pins before reuse (`kernel/src/task/syscall.rs:2066`, `kernel/src/memory/pin.rs:223`).
- `Service death`: clients must receive explicit error/timeout, never silent fallback to `GetFile`. Peer-death CQ remains outside this narrow VFS boundary unless the read path becomes async.

## Fast Path Identity

Fast VFS must not accept caller-provided identity. Current `kernel::fast_ipc::call_vfs` derives identity from live scheduler state before invoking the handler (`kernel/src/fast_ipc.rs:126`, `kernel/src/fast_ipc.rs:150`), and the VFS handler refuses `None` plus unseen callers (`cells/services/vfs/src/main.rs:117`, `cells/services/vfs/src/main.rs:127`). Keep this only for same-SAS Tier-1 fallback work; do not use fast `GetFile` as a Layer-B migration target.

## Async Pinning and Stale Frames

The Async Pinning Registry is a substrate, not VFS policy. Use it only when VFS or a device may touch caller memory after the synchronous reply window. Its contract is fail-closed: owner cannot free pinned memory, owner death quarantines frames, and release needs explicit acknowledgement (`kernel/src/memory/pin.rs:4`, `kernel/src/memory/pin.rs:23`). `GrantFree`/`GrantUnregister` already refuse pinned regions (`kernel/src/task/syscall.rs:4167`, `kernel/src/task/syscall.rs:4223`). Do not attempt to pin `DataPtr`; it has no revoke point.

## Lock Ordering

- VFS request path: `GLOBAL_VFS` lock may be held for decode/dispatch/encode, but is released before `sys_send` to avoid scheduler/VFS deadlock (`cells/services/vfs/src/main.rs:185`, `cells/services/vfs/src/main.rs:198`).
- Pin/grant teardown: `PAGE_GRANT_TABLE` or `REG_GRANT_TABLE` -> `PIN_TABLE` leaf -> release -> `FRAME_ALLOCATOR` -> `KERNEL_ROOT`, matching `pin.rs` (`kernel/src/memory/pin.rs:28`).
- Kernel cap read: park file under `CAP_TABLE`, perform I/O outside the lock, then unpark (`kernel/src/cell/cap_registry.rs:23`, `kernel/src/task/syscall.rs:2901`).
- New VFS handle code must not call kernel syscalls while holding VFS internal locks except current bounded service-call reply rules.

## Precise File Touch Set

- `libs/api/src/services/ipc.rs`: append VFS file-handle variants only after Law-1 2x confirmation.
- `docs/specs/17-ipc-wire-contract.md`: document new wire contract, direct fast-path restriction, and no-`DataPtr` Layer-B gate.
- `docs/specs/09-vfs.md`: document file handle lifetime/revoke semantics.
- `cells/services/vfs/src/dispatch.rs`: route open/read/close/revoke; keep path-string refusal boundary.
- `cells/services/vfs/src/handle_table.rs`: extend entries for file handles; preserve `Caller` owner checks.
- `cells/services/vfs/src/dir_admission.rs` and `dispatch_dirs.rs`: derive file scope from existing dir-cap authority.
- `libs/ostd/src/fs.rs`: migrate clients from `GetFile` to copy-out or handle reads.
- `kernel/src/memory/pin.rs` and `kernel/src/task/syscall.rs`: touch only if reads become cancellable/async across caller memory.

## Hard Scope Stops

- No Tier-2/Layer-B page-table work in this plan.
- No loader import bridge or resurrection of direct fast dispatch as proof.
- No removal of old path variants until migrated clients and Law-1 removal confirmation.
- No async VFS read unless grant pin/quarantine and explicit completion semantics are in place.
- No claim that kernel `OpenCap` replaces VFS; it bypasses VFS mount/overlay semantics even though it is owner-checked (`kernel/src/task/syscall.rs:2836`, `libs/ostd/src/fs.rs:266`).

## Law 1 Checkpoints

1. Before appending any `VfsRequest`/`VfsResponse` variant.
2. Before removing or renumbering any path-string variant.
3. Before changing syscall ABI for VFS lifecycle, completion, or revocation.
4. Before changing Spec 17 caller attestation or fast-path constraints.

## Failure Modes and Mitigations

- **Permanent SAS leak via leftover `GetFile`: High x Critical.** Mitigate with grep gate: no new consumers, converted consumers prefer `ReadFileGrant`/handle reads, and Layer-B gate fails while `DataPtr` remains reachable.
- **Wrong-owner/stale-generation handle reuse: Medium x Critical.** Mitigate with `Caller` owner on every table and death tests for same `CellId`, different generation.
- **Grant frame reuse after cancel/death: Medium x Critical.** Mitigate with pin-before-escape, quarantine-on-death, explicit ack release.
- **Path-string migration window gives false guarantee: High x High.** Mitigate with per-cell sealed/handle-only refusal and tests that sealed cells get `Err(3)` for path reads.
- **Law-1 ABI drift: Medium x High.** Mitigate by append-only variants, wire tests, 2x confirmation, and rollback via client fallback to old path API until final removal.
- **Deadlock from lock order regression: Low x High.** Mitigate with lock-order comments/tests and no send/syscall while holding VFS locks.

## Rollback

- Bounded copy-out migration: revert client wrappers to old path API; no persisted state.
- File-handle phase: leave appended variants unused and keep path-string fallback; remove handle-only flag for affected cells.
- Revocable grant-read phase: disable async/cancellable read path and return to synchronous bounded copy-out. Quarantined frames already withheld cannot be un-quarantined without the ack path; leak is the safe rollback residue.

## Success Criteria

- No new `GetFile/DataPtr` consumer; converted paths pass existing VFS grant tests.
- Sealed cell cannot issue path-addressed read, including fast path.
- File read handles are owner/generation checked, closeable, revocable, and reaped on cell death.
- Async/cancellable grant read either pins/quarantines correctly or is not enabled.
- Layer-B planning can cite zero remaining `DataPtr` dependency on any cross-domain path.
