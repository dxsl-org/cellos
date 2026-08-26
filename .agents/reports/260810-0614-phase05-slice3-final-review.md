**VERDICT:** PASS — the shared VFS facade now owns VFS-local error-code mapping, shell no longer needs the missing-path compatibility shim, and no remaining actionable Slice3 blocker was observed.

[POSITIVE] libs/ostd/src/clients/vfs.rs:13 — VFS facade methods now import the centralized VFS-local wire mapper instead of the generic `ViError` discriminant mapper.
[POSITIVE] libs/ostd/src/clients/vfs.rs:94 — `stat` maps `ERR_IO=1` to `ViError::IO`, so missing VFS paths preserve the shell-visible missing-path contract without caller-specific compensation.
[POSITIVE] cells/tools/shell/src/cmd_fs.rs:329 — shell read sizing now propagates facade errors directly; the prior `InvalidArgument -> IO` special-case is gone.
[POSITIVE] libs/ostd/src/clients/vfs/read_file/wire.rs:9 — the shared VFS-local mapper covers IO, quota, denied, and handle errors in one place for both facade operations and handle reads.
[POSITIVE] cells/tools/shell/src/cmd_fs.rs:347 — migrated shell reads remain handle-bounded and exact-size checked, with no grant/raw/async/fast fallback.

Verification: accepted rerun evidence from the task prompt: fmt/diff, RV64 app-shell/ostd/netbroker/hypha check, API 78, and QEMU shell-utils 1/1 pass. Local scoped `git diff --check` passed.
