# Scout Report — SAS/LBI Revocable VFS Access

## Verified Baseline

- Standards source: `docs/code-standards.md:1` exists; `docs/coding.md` is absent in this checkout.
- Law 1 remains the ABI gate for `libs/api/` and `libs/types/` (`docs/code-standards.md:12`).
- The approved freeze still holds: migration tactic = bounded copy-out, endpoint = file handle + bounded read, `ReadGrant` deferred.

## Frozen Contract Surface

- Public VFS request surface is still `GetFile`, `ReadAsync`, `Poll`, `ReadGrant`, `ReadFileGrant`, and dir-cap `ReadAt` in one append-only enum (`libs/api/src/services/ipc.rs:27`, `libs/api/src/services/ipc.rs:48`, `libs/api/src/services/ipc.rs:56`, `libs/api/src/services/ipc.rs:79`, `libs/api/src/services/ipc.rs:111`).
- `GetFile` still returns raw `DataPtr` (`libs/api/src/services/ipc.rs:202`).
- Spec 17 names `DataPtr` as permanent SAS authority and requires `GetFile`/`DataPtr` removal or translation before any Tier 2 boundary (`docs/specs/17-ipc-wire-contract.md:451`, `docs/specs/17-ipc-wire-contract.md:454`).
- Spec 18 says `DataPtr`-style raw pointers are unrepresentable across the tier boundary (`docs/specs/18-cell-trust-tiers.md:156`).

## Producer / Consumer Inventory

### Live producers

- Message-path `GetFile -> DataPtr` producer: `cells/services/vfs/src/dispatch.rs:55`, `cells/services/vfs/src/dispatch.rs:63`.
- Fast-path `GetFile -> DataPtr` producer: `cells/services/vfs/src/main.rs:97`, `cells/services/vfs/src/main.rs:125`, `cells/services/vfs/src/main.rs:136`, `cells/services/vfs/src/main.rs:164`.
- Path copy-out producer: `ReadAsync -> PendingHandle -> Poll -> Data` (`cells/services/vfs/src/dispatch.rs:161`, `cells/services/vfs/src/dispatch.rs:175`, `cells/services/vfs/src/dispatch.rs:178`).
- Grant copy producers: `ReadGrant` and `ReadFileGrant` (`cells/services/vfs/src/dispatch.rs:209`, `cells/services/vfs/src/dispatch.rs:292`).
- Dir-handle bounded reader: `ReadAt` in dir dispatch (`cells/services/vfs/src/dispatch.rs:324`, `cells/services/vfs/src/dispatch_dirs.rs:42`).

### Backend / manager reachability

- Pointer producers still exist behind `get_file_ptr` in the manager and backends (`cells/services/vfs/src/manager.rs:85`, `cells/services/vfs/src/manager.rs:86`).
- Exemplar pointer-capable backends remain `backend_ramfs` and overlay composition; disk-backed backends already refuse pointer export and force copy paths (`cells/services/vfs/src/backend_ramfs.rs:136`, `cells/services/vfs/src/backend_bin_overlay.rs:34`, `cells/services/vfs/src/backend_bootfs.rs:49`, `cells/services/vfs/src/backend_littlefs.rs:63`, `cells/services/vfs/src/backend_fat.rs:162`).

### Raw-pointer consumers

- Shell: fast probe + message fallback decode `DataPtr`, then fallback to `ReadAsync`/`Poll` for disk-backed paths (`cells/tools/shell/src/cmd_fs.rs:341`, `cells/tools/shell/src/cmd_fs.rs:347`, `cells/tools/shell/src/cmd_fs.rs:368`, `cells/tools/shell/src/cmd_fs.rs:381`, `cells/tools/shell/src/cmd_fs.rs:385`).
- Lua: fixed-buffer and `Vec` helpers both issue `GetFile` and decode `DataPtr` (`cells/runtimes/lua/src/bindings_vfs.rs:48`, `cells/runtimes/lua/src/bindings_vfs.rs:57`, `cells/runtimes/lua/src/bindings_vfs.rs:68`, `cells/runtimes/lua/src/bindings_vfs.rs:89`, `cells/runtimes/lua/src/bindings_vfs.rs:95`, `cells/runtimes/lua/src/bindings_vfs.rs:106`).
- WASM loader: `GetFile` + `DataPtr` only (`cells/tools/wasm/src/main.rs:97`, `cells/tools/wasm/src/main.rs:100`, `cells/tools/wasm/src/main.rs:113`).

### Copy-path callers

- Service HTTPD reads through `ReadAsync`/`Poll` and returns empty on all failures (`cells/services/httpd/src/net_ipc.rs:126`, `cells/services/httpd/src/net_ipc.rs:131`, `cells/services/httpd/src/net_ipc.rs:144`, `cells/services/httpd/src/net_ipc.rs:151`).
- Net-tools HTTPD does the same with a caller buffer and silent `0` fallback; it documents the 480-byte reply ceiling (`cells/tools/net-tools/src/bin/httpd.rs:68`, `cells/tools/net-tools/src/bin/httpd.rs:73`, `cells/tools/net-tools/src/bin/httpd.rs:77`, `cells/tools/net-tools/src/bin/httpd.rs:90`, `cells/tools/net-tools/src/bin/httpd.rs:97`).

### Facade / downstream mismatch

- `VfsClient::read_file()` still sends `GetFile` but only accepts `VfsResponse::Data`, so the facade contract disagrees with the service (`libs/ostd/src/clients/vfs.rs:20`, `libs/ostd/src/clients/vfs.rs:37`, `libs/ostd/src/clients/vfs.rs:38`, `libs/ostd/src/clients/vfs.rs:44`).
- Downstreams that inherit this mismatch: Hypha FS tool (`cells/apps/hypha/tools/fs/src/main.rs:58`, `cells/apps/hypha/tools/fs/src/main.rs:61`), net-broker config loader (`cells/services/net-broker/src/identity.rs:59`, `cells/services/net-broker/src/identity.rs:60`), and net-broker key loader (`cells/services/net-broker/src/transport.rs:64`, `cells/services/net-broker/src/transport.rs:66`).

### Grant / handle paths already present

- Spawn-side bounded copy path is already wired through `libs/ostd/src/fs.rs:295`.
- Non-production `ReadGrant` client helper still exists in `libs/ostd/src/fs.rs:358`.
- VFS file-handle table exists and is generation-scoped, but no production file-open path inserts into it yet (`cells/services/vfs/src/handle_table.rs:44`, `cells/services/vfs/src/handle_table.rs:48`).
- Pending async reads are owner/generation scoped and re-authorize on `Poll` (`cells/services/vfs/src/pending.rs:28`, `cells/services/vfs/src/pending.rs:32`, `cells/services/vfs/src/pending.rs:44`, `cells/services/vfs/src/pending.rs:76`).
- Dir handles already purge on higher-generation contact and support explicit cell purge (`cells/services/vfs/src/dirs/lifecycle.rs:61`, `cells/services/vfs/src/dirs/lifecycle.rs:62`, `cells/services/vfs/src/dirs/lifecycle.rs:175`).

### Kernel capability / lifecycle surfaces

- Kernel file capabilities live in a separate `CAP_TABLE`; `ReadCap` parks the file outside the lock, and `revoke_all_for` is cell-only, not generation-aware (`kernel/src/cell/cap_registry.rs:23`, `kernel/src/cell/cap_registry.rs:27`, `kernel/src/cell/cap_registry.rs:204`, `kernel/src/cell/cap_registry.rs:257`).
- Exit and force-exit both revoke kernel caps today (`kernel/src/task/syscall.rs:2058`, `kernel/src/task/syscall.rs:2152`).
- `NotifyOnExit` exists but is SpawnCap-gated and remains checkpoint material, not Phase 01 authority (`kernel/src/task/syscall.rs:880`, `kernel/src/task/syscall.rs:2281`).

### Fast-path reachability

- Kernel fast-IPC VFS hook is still registered and callable (`kernel/src/fast_ipc.rs:50`, `kernel/src/fast_ipc.rs:58`, `kernel/src/fast_ipc.rs:141`).
- Userspace fast-IPC scaffolding is still linked in `libs/ostd/src/fast_ipc.rs:53`, `libs/ostd/src/fast_ipc.rs:85`, `libs/ostd/src/fast_ipc.rs:155`.
- VFS comments already admit separately linked Cells currently miss the direct path, so shell fast probing is characterization-only, not proof of a production fast lane (`cells/tools/shell/src/cmd_fs.rs:344`, `cells/tools/shell/src/cmd_fs.rs:353`).

## Tests And Runtime Oracles

- The current seal/runtime oracle still proves: `GetFile` works before sealing, `ReadFileGrant` clamps and copies nonzero bytes, then path-addressed reads are denied after sealing (`cells/tests/vfs-test/src/dircap.rs:261`, `cells/tests/vfs-test/src/dircap.rs:263`, `cells/tests/vfs-test/src/dircap.rs:317`, `cells/tests/vfs-test/src/dircap.rs:334`, `cells/tests/vfs-test/src/grant_io.rs:67`, `tests/integration/tests/vfs-quota.rs:95`).
- Service HTTPD runtime coverage exists in integration boot tests (`tests/integration/tests/boot.rs:646`, `tests/integration/tests/boot.rs:696`).

## Generated / Embedded Hit Classification

- Product/runtime hits: all files listed above under `cells/`, `libs/`, `kernel/`, and `docs/specs/`.
- Test-only hits: `cells/tests/vfs-test/src/dircap.rs`, `cells/tests/vfs-test/src/grant_io.rs`, `tests/integration/tests/vfs-quota.rs`, bench `cells/tests/bench/src/scenarios/vfs_getfile_breakdown.rs`.
- Generated/build artifacts: `build/*` binary matches are non-source and excluded from migration authority.
- Embedded/tooling/third-party noise: `limine/**`, `scripts/write-vfs-main.py`, and `scripts/unsafe-allowlist.toml` mention `DataPtr`/HTTPD context but are not runtime call sites.

## Frozen Migration Order

1. Characterize and fix the facade mismatch first: `libs/ostd/src/clients/vfs.rs` before downstreams.
2. Preserve spawn `ReadFileGrant` as the bounded copy-out reference path (`libs/ostd/src/fs.rs:295`).
3. Phase 02 pioneer: shell bounded copy-out only; keep Lua, WASM, Hypha, net-broker, and both HTTPDs on characterized current transport until handle reads exist.
4. Phase 03: settle lifecycle authority and cleanup before any durable file-handle producer.
5. Phase 04: append file-handle ABI, reuse dir authority, and add owner/generation-scoped file handles.
6. Phase 05 caller order: VfsClient downstreams -> shell -> Lua -> WASM -> service HTTPD -> net-tools HTTPD -> disable `GetFile`/fast `GetFile`.
7. Preserve `ReadFileGrant` spawn parity until `/bin` overlay and boot/runtime parity are proven on handle reads.

## Ownership Reconciliation

- Phase 01 stays plan-only; no product files are authorized for modification (`file-change-manifest.md`).
- Phase 02 ownership is intentionally narrow: shell + facade + tests; HTTPD is characterization-only.
- Phase 03 owns lifecycle files only after separate approval; no `libs/api/`, syscall number, manifest bit, or wire edit.
- Phase 04 is the first Law 1 checkpoint and owns the append-only ABI plus VFS handle implementation.
- Phase 05 owns all remaining callers plus fast-path disablement, but keeps ABI discriminants reserved.
- Phase 06 owns evidence/docs updates only after implementation lands.

## Precedent Commits

- `d34b23dd` — bounded grant-copy hardening precedent.
- `72f01d0d` — fail-closed destructive-op authorization precedent.
- `7a525538` — kernel-attested VFS read identity precedent.
