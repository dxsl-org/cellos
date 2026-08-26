# Phase 04 Exact Semantic/ABI Checkpoint

## Verdict

Ready for the user's Phase 04 semantic-checkpoint approval; implementation remains stopped until that approval names the exact append-only delta in `phase-04-file-handle-bounded-reads.md`.

Existing Law 1 confirmations #1/#2 from 2026-08-09 are sufficient for this exact checkpoint because it stays within the already recorded "append-only handle delta" and does not touch syscall numbers, manifests, `libs/types`, or fast IPC. A new confirmation pair is required if any request/response name, field, discriminant, syscall, manifest, wire shape, fast-path reachability, or lifecycle authority expands.

## OBSERVED Baseline

- Law 1 covers `libs/api/` and `libs/types/`, and requires two explicit confirmations (`docs/code-standards.md:12`).
- Current `VfsRequest` begins at `GetFile` and ends at `SealPaths` (`libs/api/src/services/ipc.rs:27`, `libs/api/src/services/ipc.rs:152`).
- Current `VfsResponse` ends at `DirHandle` (`libs/api/src/services/ipc.rs:197`, `libs/api/src/services/ipc.rs:220`).
- Current tests freeze `GetFile=0`, `ReadFileGrant=13`, and `ReadAt=16` (`libs/api/src/services/dir_name_tests.rs:252`).
- Directory handle type is in frozen ABI because kernel carries it across spawn (`libs/api/src/abi/dir_handles.rs:38`); file handles are service-local and must not be kernel-carried.
- VFS refuses unattested requests before dispatch (`cells/services/vfs/src/dispatch.rs:28`), admits inherited dir state before serving (`cells/services/vfs/src/dispatch.rs:36`), and refuses path-addressed requests after sealing (`cells/services/vfs/src/dispatch.rs:50`).
- Existing directory resolution validates one component and joins it under an owned dir (`cells/services/vfs/src/dirs.rs:112`).
- Current owner-death bridge watches only after owned-state responses and purges owned state on unattributed death (`cells/services/vfs/src/main.rs:212`, `cells/services/vfs/src/manager.rs:178`).
- Fast IPC currently serves only `GetFile`; every other request returns `0xFE` (`cells/services/vfs/src/main.rs:125`, `cells/services/vfs/src/main.rs:147`).

## Exact Delta

- New API module: `libs/api/src/services/vfs_file_handles.rs`; exported from `libs/api/src/services.rs`.
- New type: `ViVfsFileHandle(u64)`, transparent serde newtype, service-local, never `CapId`.
- Requests appended: `OpenFileAt=23`, `ReadFileHandle=24`, `CloseFile=25`.
- Response appended: `FileHandle=9`.
- Reads return existing `VfsResponse::Data`, bounded to `IPC_BUF_SIZE - 96`.
- File table owns id allocation; id `0` is invalid; ids are monotonic nonreused; exhaustion fails `ERR_QUOTA`.
- File entries are owned by `Caller { cell, generation }`, record `parent_dir`, and re-authorize path policy on every read.
- Parent `CloseDir` must return the full transitive revoked-dir id set to purge files opened from any revoked ancestor/descendant.
- Lazy generation replacement in `DirTable::on_contact` must expose the same revoked-dir id set, so `dir_admission` also purges file handles anchored anywhere in the predecessor's transitive directory graph.
- Owner death cleanup must include new file handles in addition to dirs, legacy handles, and pending reads.
- Fast IPC remains excluded.

## Bootstrap Matrix

Every Phase 05 caller must prove one of these before migration:

| Caller class | Allowed bootstrap | Stop if |
|---|---|---|
| Already path-capable before seal | `OpenRootDir` before `SealPaths` | needs a post-seal absolute opener |
| Spawned child with an already-proven authorized producer | inherited `ViSpawnDirHandles`, bound all-or-nothing by VFS | the producer cannot call `SpawnSetDirs` |
| Shell-spawned or legacy caller | self-bootstrap with `OpenRootDir` before `SealPaths` over sender-masked message IPC | needs post-seal absolute open or new syscall authority |
| Fast/direct caller | message path fallback only | needs fast-IPC file handle arm |

## Hard Stops

Stop before implementation or mid-build on any syscall number, manifest bit, `libs/types` edit, fast-IPC expansion, `RecvScatter`, async grant/DMA lifetime, broad VFS `SpawnCap`, missing caller directory bootstrap, reusable id allocation, or inability to prove parent/death/generation-replacement cleanup.

## Commands

- `cargo test -p api --target x86_64-unknown-linux-gnu`
- `cargo test -p service-vfs --target x86_64-unknown-linux-gnu` or resolved package name from `cargo metadata`
- `cargo fmt --all --check`
- `bash scripts/build-test-hooks-ci.sh`
- `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`
- RV64/AArch64/x86_64 production kernel release builds used by Phase 03
- `git diff --check`

## Rollback

Before any caller ships against the new ABI, revert Phase 04 docs/API/VFS/client-test files as one slice and continue using Phase 02 `ReadFileGrant`. After publication, variants 23-25 and response 9 can only be reserved/disabled, not removed or renumbered.
