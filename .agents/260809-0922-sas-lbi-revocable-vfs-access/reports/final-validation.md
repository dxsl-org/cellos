# Final Validation — SAS/LBI Revocable VFS Access

Verdict: **CLEAR**. Re-validation was limited to the four applied corrections plus `file-change-manifest.md` path/exactness and current line caps. No plan/product/docs files were edited by this validation pass.

## Counts

- Corrections checked: 4/4 applied and source-backed.
- Remaining blockers: 0.
- Remaining corrections required: 0.
- Needs clarification from prior full pass: 2 intentionally remain as phase-gated proof items, not blockers (`NotifyOnExit` bridge approval; per-caller dir bootstrap proof).
- Line caps: `plan.md` 72 lines (<80); phases 71, 80, 72, 89, 84, 74 lines (<150 each); `file-change-manifest.md` 54 lines.
- Manifest exactness: `.agents/260809-0922-sas-lbi-revocable-vfs-access/file-change-manifest.md` exists; all listed existing product/test/doc paths sampled below resolve by `git ls-files --error-unmatch`. Planned create-only paths remain correctly labeled as create.

## Correction Recheck

1. **Exact RV64 QEMU command — CLEAR**
   - Phase 06 now requires `bash scripts/build-test-hooks-ci.sh`, then `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture` (`phase-06-closure-verification-and-rollback.md`, step 5).
   - Source supports the named test and required grant markers: `tests/integration/tests/vfs-quota.rs:67`, `tests/integration/tests/vfs-quota.rs:96-104`.

2. **`ReadFileGrant` line citations — CLEAR**
   - `plan.md` rationale cites `libs/api/src/services/ipc.rs:79` and `cells/services/vfs/src/dispatch.rs:292`.
   - Phase 02 architecture uses the same corrected citations.
   - Source confirms variant starts at `libs/api/src/services/ipc.rs:79`; dispatch arm starts at `cells/services/vfs/src/dispatch.rs:292` and clamps at `dispatch.rs:304`.

3. **Shell pioneer wording — CLEAR**
   - Phase 02 now says shell's syscall allowlist has grant operations, but `cmd_fs.rs` has no grant adapter yet; Phase 02 implements/proves it as pioneer.
   - Source confirms shell allowlist includes `GrantAlloc`, `GrantShare`, `GrantSlice`, `GrantFree` (`cells/tools/shell/src/main.rs:42-45`), while current read path still uses `GetFile`/`DataPtr` plus `ReadAsync/Poll` fallback (`cells/tools/shell/src/cmd_fs.rs:341-381`).

4. **Inventory wording — CLEAR**
   - Phase 05 step 1 now says “tables, and facade/downstream surfaces,” not “all listed handle tables.”
   - This matches current code: VFS `handle_table.rs` is the true service handle table; other hits are caller/facade/surface inventory.

## Manifest Check

- Plan links `file-change-manifest.md` from `plan.md` Dependencies and states product/docs files remain untouched.
- Manifest exists at `.agents/260809-0922-sas-lbi-revocable-vfs-access/file-change-manifest.md`.
- Existing paths verified include VFS core (`cells/services/vfs/src/{main.rs,dispatch.rs,dispatch_dirs.rs,handle_table.rs,pending.rs,manager.rs,caller.rs,dir_admission.rs}`, `cells/services/vfs/src/dirs/lifecycle.rs`), kernel lifecycle (`kernel/src/{fast_ipc.rs,task.rs}`, `kernel/src/task/{scheduler.rs,syscall.rs}`, `kernel/src/cell/{cap_registry.rs,hotswap.rs}`), callers (`cells/tools/shell/src/cmd_fs.rs`, `cells/runtimes/lua/src/bindings_vfs.rs`, `cells/tools/wasm/src/main.rs`, Hypha/net-broker/HTTPD paths), tests (`cells/tests/vfs-test/src/{main.rs,dircap.rs,grant_io.rs}`, `tests/integration/tests/{vfs-quota.rs,http-smoke.rs,shell-utils.rs,hypha-boot.rs}`), and docs/specs.
- Create-only paths are correctly not required to exist now: `cells/services/vfs/src/file_handles.rs` and optional `tests/integration/tests/vfs-revocable-access.rs`.
- `.agents/` plan artifacts are not tracked by Git; `git ls-files` rejects them as expected, not as a path defect.

## Final State

- Law 1 gates remain before API/spec changes and disablement.
- Caller migration ordering remains shell pioneer -> handle endpoint -> facade/downstreams/direct callers -> disable old serving.
- Rollback/stops remain explicit and no runtime fallback to `GetFile` is allowed.
- Host-gated lanes are still marked deferred rather than complete.
- Current worktree has pre-existing dirty docs (`docs/TODO.md`, `docs/project-roadmap.md`, `docs/project-changelog.md`); validation did not modify them.
