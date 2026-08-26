## Current live read surfaces are split three ways
**Verdict:** Cellos does not have one VFS read path; it has three: raw-pointer `GetFile`, grant copy-out `ReadFileGrant`, and kernel file-cap syscalls.
- `VfsRequest` still exposes `GetFile`, `ReadGrant`, `ReadFileGrant`, and the appended dir-cap variants in one ABI surface.
- Message-path VFS serves `GetFile`, `ReadGrant`, and `ReadFileGrant` in `dispatch.rs`; dir-cap requests are delegated to `dispatch_dirs.rs`.
- Kernel syscalls `OpenCap`/`ReadCap`/`CloseCap` are a separate file-I/O stack backed by `CAP_TABLE`, not by VFS IPC.
- Law 1 still applies to any public `libs/api` or `libs/types` change before reshaping these surfaces.
**Source:** [libs/api/src/services/ipc.rs](/home/dmin/cellos/libs/api/src/services/ipc.rs:27), [cells/services/vfs/src/dispatch.rs](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:54), [kernel/src/task/syscall.rs](/home/dmin/cellos/kernel/src/task/syscall.rs:2835), [docs/code-standards.md](/home/dmin/cellos/docs/code-standards.md:12)

## `GetFile` is live, same-SAS only, and still the widest authority leak
**Verdict:** `GetFile` is the only live producer of `DataPtr`, and every real consumer must dereference it inside the shared SAS; this cannot cross Tier 2.
- VFS message-path authorizes `GetFile` with `can_read` and returns `DataPtr { ptr, len }`; the fast path serves the same response shape.
- Real in-tree consumers are shell `read_file_vfs`, Lua `vfs_get_file{,_vec}`, and the WASM loader; all copy from the returned raw pointer.
- Spec 18 says `DataPtr`-style raw pointers are unrepresentable across Tier-2 boundaries, and Spec 17 says `GetFile`/`DataPtr` must be removed or translated before Layer B.
- Shell already documents the direct fast-IPC probe as inactive for separately linked cells, so current production reads are the governed message path, not the direct path.
**Source:** [cells/services/vfs/src/dispatch.rs](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:55), [cells/services/vfs/src/main.rs](/home/dmin/cellos/cells/services/vfs/src/main.rs:97), [cells/tools/shell/src/cmd_fs.rs](/home/dmin/cellos/cells/tools/shell/src/cmd_fs.rs:336), [cells/runtimes/lua/src/bindings_vfs.rs](/home/dmin/cellos/cells/runtimes/lua/src/bindings_vfs.rs:48), [cells/tools/wasm/src/main.rs](/home/dmin/cellos/cells/tools/wasm/src/main.rs:95), [docs/specs/18-cell-trust-tiers.md](/home/dmin/cellos/docs/specs/18-cell-trust-tiers.md:155), [docs/specs/17-ipc-wire-contract.md](/home/dmin/cellos/docs/specs/17-ipc-wire-contract.md:248)

## Fast-IPC `GetFile` has authorization parity on paper, but runtime reachability is still effectively dead
**Verdict:** The fast path is no longer an auth bypass, but it is still not the current runtime proof or migration vehicle.
- Kernel `call_vfs` derives caller identity from live scheduler state, not from caller-controlled arguments, matching Spec 17 attestation intent.
- VFS fast handler refuses unknown callers, checks `has_met`, `is_sealed`, and `can_read`, then serves only `GetFile`; all other ops are forced back to the ecall path.
- Kernel fast-IPC notes that separately linked cells still read their own null handler table and fall back; the shell comment confirms current reads use the governed message round trip.
- The old `kernel::task::spawn_from_file` raw-opcode `GetFile` stub is dead scaffolding, not part of the attested VFS path.
**Source:** [kernel/src/fast_ipc.rs](/home/dmin/cellos/kernel/src/fast_ipc.rs:126), [cells/services/vfs/src/main.rs](/home/dmin/cellos/cells/services/vfs/src/main.rs:107), [cells/tools/shell/src/cmd_fs.rs](/home/dmin/cellos/cells/tools/shell/src/cmd_fs.rs:338), [kernel/src/task.rs](/home/dmin/cellos/kernel/src/task.rs:1125)

## `ReadFileGrant` is the only live bounded copy-out path already used by production code
**Verdict:** `ReadFileGrant` is the real migration foothold today because it is live, bounded by grant length, and already backs spawn.
- VFS authorizes `ReadFileGrant` before resolving the grant and copies `min(file_len, max, grant_len)` bytes into the caller grant.
- `ostd::fs::read_full_via_grant` is live and is the first path used by `sys_spawn_from_path` once VFS is up; bootstrap fallback only runs if that path fails.
- The QEMU acceptance tests prove clamp, nonzero copy, and post-`SealPaths` refusal; Phase 02 explicitly closes on those markers.
- Trade-off: it is still path-addressed, so it preserves ambient naming until the caller is sealed; it fixes pointer revocability, not namespace authority, by itself.
**Source:** [cells/services/vfs/src/dispatch.rs](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:292), [libs/ostd/src/fs.rs](/home/dmin/cellos/libs/ostd/src/fs.rs:266), [libs/ostd/src/syscall.rs](/home/dmin/cellos/libs/ostd/src/syscall.rs:312), [cells/tests/vfs-test/src/grant_io.rs](/home/dmin/cellos/cells/tests/vfs-test/src/grant_io.rs:62), [cells/tests/vfs-test/src/dircap.rs](/home/dmin/cellos/cells/tests/vfs-test/src/dircap.rs:237), [docs/project-roadmap.md](/home/dmin/cellos/docs/project-roadmap.md:26)

## `ReadGrant` still has no real producer; it is fail-closed and test-only today
**Verdict:** `ReadGrant` exists in the ABI and dispatch, but the only confirmed producer for its VFS-side handle table is a unit test.
- `ReadGrant` re-authorizes through `handles.path_of` and `handles.get_mut`, but that only matters if a real `HandleEntry` exists.
- `HandleTable::insert_ro` has no non-test callers in the VFS tree; the only observed caller is the test fixture inside `handle_table.rs`.
- Phase 02 already records this: the current runtime claim is unknown-cap zero-byte/fail-closed behavior, not a real file-backed producer.
- Result: revocable grant reads are not migration-ready yet; they need the future Law-1 `OpenAt`/file-handle/close design to create real handles first.
**Source:** [cells/services/vfs/src/dispatch.rs](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:209), [cells/services/vfs/src/handle_table.rs](/home/dmin/cellos/cells/services/vfs/src/handle_table.rs:55), [cells/tests/vfs-test/src/grant_io.rs](/home/dmin/cellos/cells/tests/vfs-test/src/grant_io.rs:120), [.agents/260727-2101-midori-lessons-cellos/phase-02-vfs-read-gating.md](/home/dmin/cellos/.agents/260727-2101-midori-lessons-cellos/phase-02-vfs-read-gating.md:198), [.agents/260805-1833-midori-closure-execution/plan.md](/home/dmin/cellos/.agents/260805-1833-midori-closure-execution/plan.md:37)

## Kernel `OpenCap`/`ReadCap`/`CloseCap` is the other live file-handle path, but it bypasses VFS semantics
**Verdict:** The kernel cap syscalls already provide bounded handle reads, but they are not a drop-in replacement for VFS namespace or `/bin` overlay behavior.
- `OpenCap` opens through kernel `VIFS1`, allocates a `CAP_TABLE` entry owned by the caller cell, and `ReadCap`/`CloseCap` enforce ownership there.
- Real live users are the hypervisor loaders and the `ostd::fs::File` wrappers; `backend_bootfs` also uses this path internally for synchronous BootFS reads.
- This path is streaming-friendly and Tier-2-shaped because it returns copied bytes, not raw pointers; the cost is architectural mismatch with VFS mount/ACL/overlay logic.
- Specifically, `read_full_via_grant` exists because spawn needs the VFS mount table to reach `/bin` cell-store overlay plus bootstrap cells; raw kernel `OpenCap` alone does not solve that.
**Source:** [kernel/src/task/syscall.rs](/home/dmin/cellos/kernel/src/task/syscall.rs:2836), [kernel/src/cell/cap_registry.rs](/home/dmin/cellos/kernel/src/cell/cap_registry.rs:19), [cells/services/hypervisor/src/main.rs](/home/dmin/cellos/cells/services/hypervisor/src/main.rs:30), [cells/services/hypervisor/src/loader_image.rs](/home/dmin/cellos/cells/services/hypervisor/src/loader_image.rs:126), [cells/services/vfs/src/backend_bootfs.rs](/home/dmin/cellos/cells/services/vfs/src/backend_bootfs.rs:109), [libs/ostd/src/fs.rs](/home/dmin/cellos/libs/ostd/src/fs.rs:232)

## Directory-capability VFS ops are implemented, but the only runtime caller is still the test pioneer
**Verdict:** The dir-cap surface is real in VFS, but not yet adopted by any production cell beyond `/bin/vfs-test`.
- `dispatch_dirs.rs` implements `OpenRootDir`, `OpenDir`, `ReadAt`, `WriteAt`, `StatAt`, `ListAt`, `UnlinkAt`, `CloseDir`, and `SealPaths`.
- Repo-wide callers for those request variants are only the VFS service itself plus `cells/tests/vfs-test/src/dircap.rs`; no shell, Lua, WASM, init, or app runtime consumes them yet.
- Spec 09 names `/bin/vfs-test` as the pioneer and states the guarantee only holds for sealed cells until path-string variants are removed.
- Practical consequence: handle-addressed VFS is architecturally ahead of adoption; replacing `GetFile` callers still requires client migration work, not just server support.
**Source:** [cells/services/vfs/src/dispatch_dirs.rs](/home/dmin/cellos/cells/services/vfs/src/dispatch_dirs.rs:32), [cells/tests/vfs-test/src/dircap.rs](/home/dmin/cellos/cells/tests/vfs-test/src/dircap.rs:261), [docs/specs/09-vfs.md](/home/dmin/cellos/docs/specs/09-vfs.md:74), [.agents/260727-2101-midori-lessons-cellos/phase-06-directory-capabilities.md](/home/dmin/cellos/.agents/260727-2101-midori-lessons-cellos/phase-06-directory-capabilities.md:156)

## Spawn-time inherited dir handles are half-wired: consumer exists, producer does not
**Verdict:** `QueryDirHandles` is live enough for VFS admission, but `SpawnSetDirs` has no in-tree caller, so inherited handle sets are currently a dead production path.
- Kernel supports `SpawnSetDirs`, carries the staged set into the child via `install_on_child`, and exposes it back through `QueryDirHandles`.
- VFS calls `sys_query_dir_handles` on first contact and seals on bound or refused inherited sets, preserving the all-or-nothing rule.
- There is an `ostd` wrapper for `sys_query_dir_handles`, but no wrapper or call site for `SpawnSetDirs`; repo search finds no non-kernel producer.
- Result: the consumer side is ready, but no live cell can currently stage inherited dir handles before spawn.
**Source:** [kernel/src/task/syscall.rs](/home/dmin/cellos/kernel/src/task/syscall.rs:2597), [kernel/src/task/dir_inherit.rs](/home/dmin/cellos/kernel/src/task/dir_inherit.rs:24), [cells/services/vfs/src/dir_admission.rs](/home/dmin/cellos/cells/services/vfs/src/dir_admission.rs:22), [libs/ostd/src/syscall.rs](/home/dmin/cellos/libs/ostd/src/syscall.rs:712), [libs/api/src/abi/syscall.rs](/home/dmin/cellos/libs/api/src/abi/syscall.rs:431)

## Ranked migration order
**Verdict:** Best fit is `GetFile/DataPtr` -> bounded copy-out first, then real handle-addressed reads, then revocable grant reads; do not start from `ReadGrant`.
- **1. Winner: replace live `GetFile` callers with bounded copy-out first.** Use existing `ReadFileGrant` where whole-file reads are natural and `ReadAt` plus grant/copy where the dir-cap pioneer can be introduced. Lowest blast radius: live producers already exist, tests already prove clamp/deny, and it removes Tier-2-blocking raw pointers first.
- **2. Runner-up: expand kernel/file-handle streaming only where VIFS1 semantics are sufficient.** `OpenCap`/`ReadCap`/`CloseCap` already give owner-checked bounded reads, but they bypass VFS mount/overlay/ACL semantics; good for hypervisor-style boot assets, wrong as the general `/bin` migration story.
- **3. Avoid for now: making `ReadGrant` the first migration target.** No live producer, no spawn-time dir-set producer, and Phase 02 already defers the real handle source to a future Law-1 `OpenAt`/file-handle/close design.
- Revoke/perf trade-off: `DataPtr` is fastest and worst for revocation; `ReadFileGrant` copies once and keeps authority bounded to a grant lifetime; real handle + bounded read is the right Tier-2 architectural endpoint; revocable `ReadGrant` only becomes credible after a live handle producer and reaper/revoke integration from the async plan.
**Source:** [docs/specs/18-cell-trust-tiers.md](/home/dmin/cellos/docs/specs/18-cell-trust-tiers.md:155), [docs/specs/19-hardware-isolation-layers.md](/home/dmin/cellos/docs/specs/19-hardware-isolation-layers.md:62), [cells/services/vfs/src/dispatch.rs](/home/dmin/cellos/cells/services/vfs/src/dispatch.rs:55), [libs/ostd/src/fs.rs](/home/dmin/cellos/libs/ostd/src/fs.rs:266), [.agents/260727-2101-midori-lessons-cellos/phase-02-vfs-read-gating.md](/home/dmin/cellos/.agents/260727-2101-midori-lessons-cellos/phase-02-vfs-read-gating.md:198)
