# Phase 04 Execution Evidence

## Result

Phase 04 is complete. The append-only file-handle ABI delta is closed, the service-local handle path is message-only, and the phase holds the expected rollback boundary.

This sync pass only updated `.agents/260809-0922-sas-lbi-revocable-vfs-access/plan.md`, `.agents/260809-0922-sas-lbi-revocable-vfs-access/phase-04-file-handle-bounded-reads.md`, and this report. No product code or docs were edited in this sync pass.

## Exact ABI

- Requests appended: `OpenFileAt=23`, `ReadFileHandle=24`, `CloseFile=25`.
- Response appended: `FileHandle=9`.
- Type: `ViVfsFileHandle(u64)` is service-local and never kernel-carried.
- Reads stay bounded to `IPC_BUF_SIZE - 96`.
- Fast IPC remains excluded; the file-handle path stays on normal message dispatch only.

## Phase-Owned Files

- API: `libs/api/src/services/ipc.rs`, `libs/api/src/services.rs`, `libs/api/src/services/vfs_file_handles.rs`, `libs/api/src/services/dir_name_tests.rs`.
- VFS service: `cells/services/vfs/src/file_handles.rs`, `cells/services/vfs/src/file_handles/{table.rs,owner_counts.rs,tests.rs,selftest.rs}`, `cells/services/vfs/src/dispatch_file_handles.rs`, `cells/services/vfs/src/main.rs`, `cells/services/vfs/src/manager.rs`, `cells/services/vfs/src/manager/{owned_state.rs,state_transfer.rs,tests.rs}`, `cells/services/vfs/src/dispatch.rs`, `cells/services/vfs/src/dispatch_dirs.rs`, `cells/services/vfs/src/dir_admission.rs`, `cells/services/vfs/src/dirs.rs`, `cells/services/vfs/src/dirs/bind.rs`, `cells/services/vfs/src/dirs/lifecycle.rs`, `cells/services/vfs/src/dirs/lifecycle/revoke.rs`.
- Tests: `cells/tests/vfs-test/src/dircap.rs`, `tests/integration/tests/vfs-quota.rs`.
- Docs: `docs/specs/17-ipc-wire-contract.md`, `docs/specs/09-vfs.md`, `docs/TODO.md`, `docs/project-roadmap.md`, `docs/project-changelog.md`.

## Delivered Semantics

- `OpenFileAt` validates the caller, directory bootstrap, access policy, and per-owner quota before issuing a handle.
- `ReadFileHandle` rechecks owner/generation and access policy on each read.
- `CloseDir`, owner death, and higher-generation replacement purge file handles anchored below revoked directory state.
- Handle ids are monotonic and nonreused; `0` stays invalid.
- Cancellation has no public phase-04 transition; inline read failure discards only service-owned response bytes.

## Final Verification

- `cargo test -p types -p api --target x86_64-unknown-linux-gnu`: pass, with 78 API tests, 2 contract tests, and 10 types tests.
- `cargo check -p service-vfs --target x86_64-unknown-linux-gnu --no-default-features`: pass.
- `bash scripts/build-test-hooks-ci.sh`: pass.
- `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`: pass; the RV64 QEMU quota lane observed the 7 table markers, parent revoke, a valid file-handle read after sealing, and path-addressed denial markers.
- `cargo fmt --all --check`: pass.
- `git diff --check`: pass.
- `cargo +nightly-2026-05-01 build --release -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc`: pass.
- `cargo +nightly-2026-05-01 build --release -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc`: pass.
- `cargo +nightly-2026-05-01 build --release -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc`: pass.
- `cargo llvm-cov --target x86_64-unknown-linux-gnu -p types -p api --summary-only`: pass; coverage is 36.84% line / 35.85% branch, and that remains pre-existing debt below the threshold.

## Host / Runtime Note

- Host `cargo test -p service-vfs` stayed unavailable on the `no_std` / unwind-gated host path, so the QEMU `vfs-quota` lane was the runtime substitute.
- No hardware runtime claim is made.
- `dispatch.rs`, `main.rs`, and `cells/tests/vfs-test/src/dircap.rs` were already over the 200-line guideline at `HEAD`; Phase 04 split its new functionality into focused submodules and did not broaden into an unrelated legacy dispatch/test rewrite.

## Review

- Standard production review: PASS.
- Final domain-risk review: `CLEAR`; the earlier Phase 02/03 scope findings were reclassified as predecessor-state false positives.

## Rollback

- Revert the Phase 04 ABI, service handle table, dispatch, and test slice as one unit.
- Continue using Phase 02 `ReadFileGrant` if Phase 04 needs to be backed out before downstream callers depend on it.
- After publication, variants `23-25` and response `9` can only be reserved or disabled, not silently removed or renumbered.
