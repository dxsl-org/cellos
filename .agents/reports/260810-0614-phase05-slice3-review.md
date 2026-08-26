**VERDICT:** PASS_WITH_RISK — shell Slice3 removes the grant/raw-pointer read dependency and preserves shell behavior; one shared `stat` error-mapping shim remains brittle but non-blocking for this slice.

[LOW]      libs/ostd/src/clients/vfs.rs:94 — `VfsClient::stat` still maps VFS-local `ERR_IO=1` through the generic `ViError` discriminant mapper, so missing/failed VFS stat becomes `InvalidArgument` and shell has to compensate at `cells/tools/shell/src/cmd_fs.rs:335`. Move all VFS-local error mapping into the VFS facade, then remove the shell-only special-case.
[POSITIVE] libs/ostd/src/clients/vfs.rs:35 — the shared facade read path is documented and implemented as `OpenRootDir`/`OpenDir`/`OpenFileAt`/`ReadFileHandle`/`CloseFile`, with no `GetFile`, `DataPtr`, grant, async, or fast fallback in the migrated shell path.
[POSITIVE] libs/ostd/src/clients/vfs/read_file/session.rs:53 — cleanup always evaluates file close and directory close before returning, so a file-close failure no longer skips directory handle release.
[POSITIVE] libs/ostd/src/clients/vfs/read_file/session.rs:106 — the read loop caps each request at `MAX_READ_CHUNK`, probes exact-limit files with a one-byte follow-up, and returns `OutOfMemory` if the file grows past the caller bound.
[POSITIVE] cells/tools/shell/src/cmd_fs.rs:348 — shell exact-size reads reject too-small caller buffers before VFS I/O, then require the handle-read byte count to equal the prior `Stat` snapshot.
[POSITIVE] cells/tools/shell/src/shell_test.rs:329 — shell-test covers exact 700-byte content, over-480-byte reads, too-small destination rejection, directory error preservation, missing-file compatibility, and post-error cleanup/reuse.
[POSITIVE] libs/ostd/src/service.rs:117 — `ServiceRef::call` now routes through `service_call_typed`, so facade calls receive replies masked to the resolved service tid instead of wildcard reply traffic.

Verification: reviewed tester evidence from the task prompt: fmt/diff, RV64 compile, `build-shell-test`, and QEMU `shell-utils` 1/1 pass; `ostd` unit harness remains known unavailable. Locally ran `git diff --check` for the scoped files: clean.
