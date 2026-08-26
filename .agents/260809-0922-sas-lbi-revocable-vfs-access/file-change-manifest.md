# Exact File Change Manifest

This is future implementation ownership, not an authorization to edit now. A file moves from read-only to modify only after its named checkpoint.

## Phase 01 — Plan/Characterization Only

- Modify product files: none.
- Read/scan: `libs/api/src/services/ipc.rs`, `libs/api/src/services/dir_name.rs`, `libs/ostd/src/fs.rs`, `libs/ostd/src/clients/vfs.rs`, `libs/ostd/src/fast_ipc.rs`.
- Read/scan: `cells/services/vfs/src/{main.rs,dispatch.rs,dispatch_dirs.rs,handle_table.rs,pending.rs,manager.rs,caller.rs,dir_admission.rs}` and `cells/services/vfs/src/dirs/lifecycle.rs`.
- Read/scan: `kernel/src/{fast_ipc.rs,task.rs}`, `kernel/src/task/{scheduler.rs,syscall.rs}`, `kernel/src/cell/{cap_registry.rs,hotswap.rs}`.

## Phase 02 — ABI-Stable Pioneer

- Modify: `libs/ostd/src/clients/vfs.rs` (characterization/typed error contract only).
- Modify: `cells/tools/shell/src/cmd_fs.rs`.
- Modify: `cells/tests/vfs-test/src/grant_io.rs`, `tests/integration/tests/vfs-quota.rs`, `tests/integration/tests/shell-utils.rs`.
- Characterize, no authority widening: `cells/services/httpd/src/{handlers.rs,net_ipc.rs}`, `cells/tools/net-tools/src/bin/httpd.rs`, `tests/integration/tests/http-smoke.rs`.
- Read-only allowlists: `cells/runtimes/lua/src/main.rs`, `cells/tools/wasm/src/main.rs`, `cells/apps/hypha/tools/fs/src/main.rs`, `cells/services/net-broker/src/main.rs`, `cells/services/httpd/src/main.rs`.

## Phase 03 — Lifecycle Gate (Separate Approval)

- Modify only after lifecycle/authority approval: `kernel/src/task.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/syscall.rs`, `kernel/src/cell/hotswap.rs`, `kernel/src/cell/cap_registry.rs`.
- Modify only after the same approval: `cells/services/vfs/src/main.rs`, `cells/services/vfs/src/dirs/lifecycle.rs`, `cells/services/vfs/src/pending.rs`.
- No `libs/api`, syscall-number, manifest, or wire edit in this phase; if required, stop for a separate checkpoint.

## Phase 04 — Handle Endpoint (Law 1 Checkpoint A)

- Modify: `libs/api/src/services.rs`, `libs/api/src/services/ipc.rs`, `libs/api/src/services/dir_name_tests.rs`; create `libs/api/src/services/vfs_file_handles.rs`.
- Create: `cells/services/vfs/src/file_handles.rs`, `cells/services/vfs/src/file_handles/{table.rs,owner_counts.rs,tests.rs,selftest.rs}`, `cells/services/vfs/src/dispatch_file_handles.rs`, `cells/services/vfs/src/dirs/lifecycle/revoke.rs`, `cells/services/vfs/src/manager/{owned_state.rs,state_transfer.rs,tests.rs}`.
- Modify: `cells/services/vfs/src/main.rs`, `cells/services/vfs/src/manager.rs`, `cells/services/vfs/src/dispatch.rs`, `cells/services/vfs/src/dispatch_dirs.rs`, `cells/services/vfs/src/dir_admission.rs`, `cells/services/vfs/src/dirs.rs`, `cells/services/vfs/src/dirs/bind.rs`, `cells/services/vfs/src/dirs/lifecycle.rs`.
- Modify tests: `cells/tests/vfs-test/src/dircap.rs`, `tests/integration/tests/vfs-quota.rs`.
- Modify docs: `docs/specs/17-ipc-wire-contract.md`, `docs/specs/09-vfs.md`, `docs/TODO.md`, `docs/project-roadmap.md`, `docs/project-changelog.md`.
- Read/test existing validator without duplicating it: `libs/api/src/services/dir_name.rs`.
- Hygiene note: `dispatch.rs`, `main.rs`, and `cells/tests/vfs-test/src/dircap.rs` were already over 200 lines at `HEAD`; Phase 04 split every new module and every touched file that this phase itself pushed over the limit, and did not widen into a legacy dispatch/test refactor.

## Phase 05 — Caller Migration and Disablement (Checkpoint B)

- Modify facade/downstreams: `libs/ostd/src/clients/vfs.rs`, `cells/apps/hypha/tools/fs/src/main.rs`, `cells/services/net-broker/src/identity.rs`, `cells/services/net-broker/src/transport.rs`.
- Modify direct callers: `cells/tools/shell/src/cmd_fs.rs`, `cells/runtimes/lua/src/bindings_vfs.rs`, `cells/tools/wasm/src/main.rs`.
- Modify HTTPD paths: `cells/services/httpd/src/handlers.rs`, `cells/services/httpd/src/net_ipc.rs`, `cells/tools/net-tools/src/bin/httpd.rs`.
- Preserve until last parity proof: `libs/ostd/src/fs.rs` spawn `ReadFileGrant` path.
- Disable old serving: `cells/services/vfs/src/dispatch.rs`, `cells/services/vfs/src/main.rs`, `kernel/src/fast_ipc.rs`, `libs/ostd/src/fast_ipc.rs`.
- Update contract after approval: `docs/specs/17-ipc-wire-contract.md`; keep `libs/api/src/services/ipc.rs` discriminants reserved.

## Phase 06 — Evidence and Documentation

- Modify tests: `cells/tests/vfs-test/src/{main.rs,dircap.rs,grant_io.rs}`, `tests/integration/tests/{vfs-quota.rs,http-smoke.rs,shell-utils.rs,hypha-boot.rs}`.
- Create if the existing harness cannot isolate the matrix: `tests/integration/tests/vfs-revocable-access.rs`, with a matching `[[test]]` entry in `tests/integration/Cargo.toml`.
- Update only after implementation evidence and by merging pre-existing user edits: `docs/project-roadmap.md`, `docs/project-changelog.md`, `docs/specs/09-vfs.md`, `docs/specs/17-ipc-wire-contract.md`.
- Merge without overwriting unrelated content: user explicitly authorized handling the pre-existing dirty `docs/TODO.md`, `docs/project-roadmap.md`, and `docs/project-changelog.md` on 2026-08-09.

## Explicit Non-Touches

- No Tier-2/per-domain page-table, DMA driver, `RecvScatter`, reactor, SMP, syscall-number, or manifest file unless a separate approved plan/checkpoint explicitly changes scope.
- No physical deletion/renumbering of `GetFile` or `DataPtr` variants in this plan.
