---
phase: 4
title: "File Handle Bounded Reads"
status: completed
priority: P1
effort: "2d"
dependencies: [2, 3]
tier: thinking
---

# Phase 04: File Handle Bounded Reads

## Overview

Append the minimal VFS file-handle ABI and service-local table needed for owner-scoped, bounded inline reads. This phase creates the endpoint only; Phase 05 migrates non-shell callers and disables `GetFile`/`DataPtr`.

## Requirements

- Functional: `OpenFileAt -> FileHandle -> ReadFileHandle -> CloseFile`, with every read owner/generation checked and re-authorized.
- Non-functional: append-only `libs/api` wire; no variant renumbering; no syscall number, manifest bit, fast-IPC reachability, Tier 2, DMA, reactor, or `RecvScatter`.
- Compatibility: existing request discriminants remain `GetFile=0`, `ReadFileGrant=13`, `OpenRootDir=14`, `OpenDir=15`, `ReadAt=16`, `SealPaths=22`; existing response `DirHandle=8` stays put.

## Architecture

**OBSERVED:** current request enum ends at `SealPaths` (`libs/api/src/services/ipc.rs:27`, `libs/api/src/services/ipc.rs:152`), response enum ends at `DirHandle` (`libs/api/src/services/ipc.rs:197`, `libs/api/src/services/ipc.rs:220`), and tests freeze `GetFile=0`, `ReadFileGrant=13`, `ReadAt=16` (`libs/api/src/services/dir_name_tests.rs:252`). Add only:

- Create `libs/api/src/services/vfs_file_handles.rs`; export from `libs/api/src/services.rs` after existing module list (`libs/api/src/services.rs:8`).
- Type: `#[repr(transparent)] pub struct ViVfsFileHandle(pub u64)` deriving `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize`.
- Request variant 23: `OpenFileAt { dir: crate::dir_handles::ViDirHandle, name: &'a str }`.
- Request variant 24: `ReadFileHandle { file: crate::vfs_file_handles::ViVfsFileHandle, offset: u64, max: u32 }`.
- Request variant 25: `CloseFile { file: crate::vfs_file_handles::ViVfsFileHandle }`.
- Response variant 9: `FileHandle(crate::vfs_file_handles::ViVfsFileHandle)`.
- `is_path_addressed`: all three new request variants are `false`; they carry service-issued handles, not caller path strings.

Data flow:
`Caller { cell, generation }` attested by recv (`cells/services/vfs/src/dispatch.rs:22`) -> existing `dir_admission::admit` and seal gate (`cells/services/vfs/src/dispatch.rs:36`, `cells/services/vfs/src/dispatch.rs:50`) -> `OpenFileAt` validates name through `DirTable::resolve` (`cells/services/vfs/src/dirs.rs:112`) -> checks `access.can_read` and `stat(path) == file` -> inserts file entry -> `ReadFileHandle` owner-checks, rechecks `access.can_read`, snapshots data from `VfsManager::read_to_vec` (`cells/services/vfs/src/manager.rs:111`) -> returns `Data(&resp_buf[..n])` where `n <= min(max, available, MAX_INLINE_FILE_READ)`.

Inline max: define `MAX_INLINE_FILE_READ: usize = api::ipc::IPC_BUF_SIZE - 96`, matching the existing handle-addressed reply margin (`cells/services/vfs/src/dispatch_dirs.rs:27`). `max=0` and `offset>=len` return `Data(&[])` as EOF, not an error. Oversized `max` is clamped, never denied. `offset` conversion overflow or backend absence returns `Err(ERR_IO=1)`; denied policy returns `Err(ERR_DENIED=3)`; unknown/wrong-owner/tombstoned handle returns `Err(ERR_HANDLE=4)`; per-owner table quota or global id exhaustion returns `Err(ERR_QUOTA=2)` (`cells/services/vfs/src/paths.rs:11`).

Dedicated table:

- Create `cells/services/vfs/src/file_handles.rs`; add `mod file_handles;` in `cells/services/vfs/src/main.rs` near current service modules (`cells/services/vfs/src/main.rs:26`).
- Add `pub files: FileHandleTable` to `VfsManager`; include it in `new`, `purge_owned_state`, and `response_creates_owned_state` (`cells/services/vfs/src/manager.rs:28`, `cells/services/vfs/src/manager.rs:191`, `cells/services/vfs/src/manager.rs:198`).
- `FileEntry { owner: Caller, path: String, parent_dir: u64, state: FileState }`; no raw `VAddr`, no `CapId`, no grant pointer.
- `FileState = Open | InFlightSyncRead | Tombstoned | Closed`. Because VFS serializes normal requests through `GLOBAL_VFS` and drops that lock before `sys_send` (`cells/services/vfs/src/main.rs:190`, `cells/services/vfs/src/main.rs:210`), no competing request can observe `InFlightSyncRead`; it is a checked service-local transition, not an async cancellation API.
- Table quota: `MAX_FILE_HANDLES_PER_CALLER = 32`. Handle `0` is never issued. `next: u64` advances with `checked_add(1)` only; no wrap, no saturating reuse. Exhaustion makes all future opens `ERR_QUOTA` until service restart; do not scan for freed ids.
- Wrong-owner `read`/`close` does not consume the real entry; unknown and wrong-owner are indistinguishable like pending reads and current handle table (`cells/services/vfs/src/pending.rs:85`, `cells/services/vfs/src/handle_table.rs:99`).

Parent lineage and cleanup:

- `OpenFileAt` records the exact parent dir id used to open the file.
- Change service-local `DirTable::revoke`/`revoke_ids` to expose the revoked dir ids as well as the count; `CloseDir` must purge `files.revoke_by_parent_dirs(&ids)` before replying `Ok`.
- Transitive parent revoke already walks dir descendants (`cells/services/vfs/src/dirs/lifecycle.rs:145`); purging by the full revoked-id set makes files opened below a revoked child disappear too.
- Apply that same outcome in `DirTable::on_contact`/`purge_cell`: `dir_admission::admit` must purge files anchored to every directory removed when a higher caller generation replaces its predecessor. Owner mismatch alone is not cleanup.
- Owner death must purge `dirs`, legacy `handles`, `pending`, and new `files` for exactly the recorded generation. Phase 03 already wires owner watch after owned-state responses and purges on death (`cells/services/vfs/src/main.rs:212`, `cells/services/vfs/src/manager.rs:178`); extend `response_creates_owned_state` to include `FileHandle(_)`.
- VFS restart starts empty for handle tables; hot-swap must not serialize file handles, matching current handle-table non-serialization (`cells/services/vfs/src/manager.rs:206`).

Lock and state order:

1. Decode/admit/seal under `GLOBAL_VFS`.
2. `OpenFileAt`: resolve dir -> validate file/stat/access -> insert file -> encode `FileHandle` while still locked.
3. After lock release, subscribe owner death through existing `NotifyOnExit`; on subscription failure, rollback owner watch and return `Err(3)` as current code does (`cells/services/vfs/src/main.rs:212`).
4. `ReadFileHandle`: own-check -> mark `InFlightSyncRead` -> clone/read bytes into service buffer -> mark `Open` unless tombstoned -> encode `Data`.
5. `CloseFile`, `CloseDir`, owner death, and service revoke tombstone/remove only while holding VFS state and make no kernel syscall while holding it.
6. Fast IPC remains excluded: `vfs_fast_handler` accepts only `GetFile` and returns `0xFE` for other requests (`cells/services/vfs/src/main.rs:117`, `cells/services/vfs/src/main.rs:147`); do not add file-handle arms there.

Cancellation has no public transition in Phase 04. If the caller dies or reply delivery fails after an inline read, only service-owned response bytes are discarded; no caller frame, grant, pointer, DMA operation, or delayed write survives. The handle remains `Open` until exact owner-death cleanup, or is reaped in that cleanup; any requirement for explicit cancellation or caller-memory survival is a hard stop back to the Phase 03 pin/lease checkpoint.

## Assumptions

- **Claim:** every Phase 05 caller can self-bootstrap a directory before sealing; inheritance is optional only where an authorized producer is separately proven.
  **Confidence:** medium
  **How to verify:** extend the Phase 02 caller matrix before migration; default to sender-masked `OpenRootDir` before `SealPaths`. Shell has `spawn=false`, so never assume its children receive `ViSpawnDirHandles`. Stop on any row that needs a new absolute opener or syscall authority.
- **Claim:** `MAX_INLINE_FILE_READ = IPC_BUF_SIZE - 96` always leaves postcard envelope room.
  **Confidence:** high
  **How to verify:** add API round-trip test encoding `VfsResponse::Data(&[0xAA; MAX_INLINE_FILE_READ])` into `IPC_BUF_SIZE`.

## Related Files

- Create: `libs/api/src/services/vfs_file_handles.rs`.
- Modify: `libs/api/src/services.rs`, `libs/api/src/services/ipc.rs`, `libs/api/src/services/dir_name_tests.rs`.
- Modify docs after the same Law 1 checkpoint: `docs/specs/17-ipc-wire-contract.md`, `docs/specs/09-vfs.md`.
- Create: `cells/services/vfs/src/file_handles.rs`.
- Modify: `cells/services/vfs/src/main.rs`, `cells/services/vfs/src/manager.rs`, `cells/services/vfs/src/dispatch.rs`, `cells/services/vfs/src/dispatch_dirs.rs`, `cells/services/vfs/src/dir_admission.rs`, `cells/services/vfs/src/dirs/lifecycle.rs`.
- Modify tests: `cells/tests/vfs-test/src/main.rs`, `cells/tests/vfs-test/src/dircap.rs`, `tests/integration/tests/vfs-quota.rs`.

## Implementation Steps

1. Law 1 gate: existing 2026-08-09 confirmation pair is sufficient only for the exact append-only delta above. Stop for a new pair if any name, field, discriminant, syscall, manifest, fast path, or wire shape changes.
2. Add `ViVfsFileHandle`, request variants 23-25, response variant 9, `is_path_addressed=false`, and discriminant tests.
3. Add `FileHandleTable` with owner/generation, parent dir id, path, state, per-owner quota, monotonic nonreuse, `purge_owner`, and `revoke_by_parent_dirs`.
4. Wire `OpenFileAt`, `ReadFileHandle`, `CloseFile` into normal dispatch only; route them beside directory capability arms, not through `ReadGrant` or fast IPC.
5. Extend `DirTable` revoke results so `CloseDir`, owner purge, and lazy higher-generation replacement expose every transitively removed dir id; purge all file handles anchored to those ids before continuing.
6. Extend Phase 03 owned-state watch to include `FileHandle(_)`; add tests that owner death purges files without purging another generation.
7. Exercise open/read/close directly through API and VFS service tests; client wrappers and caller migration remain Phase 05 work.
8. Run gates below; record exact pass/fail evidence in a Phase 04 execution report before Phase 05 starts.

## Success Criteria

- [x] API tests prove old request/response discriminants unchanged and new indices are `OpenFileAt=23`, `ReadFileHandle=24`, `CloseFile=25`, `FileHandle=9`.
- [x] `OpenFileAt` refuses bad names, directories, denied paths, unauthenticated or generation-zero callers, and per-owner quota exhaustion.
- [x] `ReadFileHandle` rechecks owner/generation and current access policy on every read; cross-cell guessing cannot read or close another handle.
- [x] Parent `CloseDir` revokes files opened from that dir and all derived dirs.
- [x] Owner death purges new file handles, old `HandleTable`, dirs, and pending reads for exactly one generation.
- [x] A higher caller generation lazily reaps predecessor file handles and any cross-cell file handles anchored below predecessor-owned directories.
- [x] Sealed cells can read through a valid file handle but `GetFile`, `ReadFileGrant`, `OpenRootDir`, and other path-addressed requests still return `Err(3)`.
- [x] No fast-IPC `ReadFileHandle` route exists; normal message fallback remains the only file-handle transport.

## Validation Matrix

- Unit/API: `cargo test -p api --target x86_64-unknown-linux-gnu`.
- Unit/VFS service: `cargo test -p service-vfs --target x86_64-unknown-linux-gnu` or the closest package name discovered by `cargo metadata`; include file-table quota/reuse/owner/revoke tests.
- Formatting: `cargo fmt --all --check`; `git diff --check`.
- Harness rebuild: `bash scripts/build-test-hooks-ci.sh`.
- Runtime: `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`.
- Production compile: RV64, AArch64, and x86_64 kernel release build commands used in Phase 03 execution evidence.

## Security Considerations

Refuse absent identity before durable state (`cells/services/vfs/src/caller.rs:42`). New handle values are non-secret; confidentiality rests on owner/generation comparison. Do not expose raw pointers, `CapId`, grant ids, or path strings in the new response. Do not serialize file handles across hot-swap. Do not add VFS broad `SpawnCap`; use only the Phase 03 current-caller owner-watch bridge.

## Risk Notes

- Risk Medium x Critical: ABI drift. Mitigation: append-only discriminant tests and Law 1 hard stop.
- Risk Medium x Critical: stale handles survive parent revoke or death. Mitigation: parent-dir id list from dir revoke plus exact-generation `purge_owner`.
- Risk Medium x High: id reuse resurrects authority after close. Mitigation: monotonic `checked_add`, no free-list, exhaustion fails closed.
- Risk Low x High: deadlock. Mitigation: no kernel syscalls or sends while holding `GLOBAL_VFS`; preserve Phase 03 notify-after-unlock order.
- Rollback: before external callers depend on it, revert Phase 04 files and docs as one slice; Phase 02 `ReadFileGrant` remains the working migration path. Irreversible part: once a released ABI documents variants 23-25/response 9, they can be reserved/disabled but not silently removed or renumbered.
- Hard stops: new syscall number, manifest bit, `libs/types` edit, fast-IPC arm, grant/DMA async lifetime, `RecvScatter`, broader SpawnCap, caller without directory bootstrap, or inability to prove owner/parent cleanup.

## Deviation Log

None.
