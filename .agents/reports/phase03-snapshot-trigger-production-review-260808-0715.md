**VERDICT:** PASS - the scoped Phase 03 snapshot trigger diff preserves syscall 420/bit32 compatibility while moving mutation authority behind SupervisorCap and supervisor-mediated shell IPC.

[POSITIVE] kernel/src/task/syscall.rs:4083 - Snapshot now checks `caller_has_supervisor(caller_id)` before calling `crate::snapshot::serialize_snapshot()` at line 4090, so an allowlisted non-supervisor caller cannot mutate snapshot state.
[POSITIVE] libs/api/src/abi/syscall.rs:651 - Snapshot still maps to allowlist bit 32 with an explicit comment that the kernel SupervisorCap gate is the authority boundary; syscall number 420 remains unchanged at `libs/api/src/abi/syscall.rs:159`.
[POSITIVE] cells/tools/shell/src/main.rs:22 - the shell allowlist no longer declares `Snapshot`, and the builtin dispatch at `cells/tools/shell/src/executor.rs:612` routes through `snapshot_client::run()` instead of calling `sys_snapshot` directly.
[POSITIVE] cells/tools/shell/src/snapshot_client.rs:71 - the shell sends a full zeroed 4096-byte App envelope to `service::SUPERVISOR`, preventing stale AppContext tail bytes from turning an opcode-only request into a malformed payload.
[POSITIVE] cells/services/supervisor/src/main.rs:78 - the supervisor checks exact sender task name `"shell"` before parsing or running the snapshot request, rejects malformed zero-tail framing at line 83, and sends only bounded 3-byte status replies.
[POSITIVE] cells/services/supervisor/src/snapshot.rs:11 - supervisor maps any kernel `sys_snapshot` error to `STATUS_UNAVAILABLE`, so a kernel `Unknown`/NullBlock path cannot become a wrapped frame-count success.
[POSITIVE] tests/integration/tests/launch-profile.rs:93 - QEMU assertions cover supervisor-routed shell unavailability, direct allowlisted bench denial without SupervisorCap, and absence of the false success marker `[snapshot] wrote`.
[POSITIVE] cells/tests/bench/src/scenarios/snapshot_authority.rs:8 - the runtime witness calls `sys_snapshot` from an allowlisted bench cell and only passes on `PermissionDenied`, matching the direct non-supervisor denial requirement.

Verification:
- Read the scoped 12-file uncommitted diff plus the App SDK IPC decoder, kernel IPC copy path, launch-profile authorization, supervisor sender identity helper, syscall mapping, and existing hotswap supervisor client for blast-radius context.
- Applied the base/API/observability pre-landing checklist; no blocking or informational findings found in scope.
- Ran `git diff --check` for the scoped files: clean.
- Did not rerun the QEMU suite in this review turn; relied on the provided tester evidence for 17/17, launch-profile, hotswap-smoke, release-kernel builds, and fresh-disk pass.
