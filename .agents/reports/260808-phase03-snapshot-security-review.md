**VERDICT:** PASS — SupervisorCap dominates the snapshot mutation path, shell-to-supervisor IPC is bounded to exact sender/status checks, and the negative QEMU witness proves capability denial rather than allowlist denial.

[POSITIVE] kernel/src/task/syscall.rs:4083 — `Syscall::Snapshot` enters a dedicated arm whose first operation is the `caller_has_supervisor` denial before `serialize_snapshot()`, so allowlist bit 32 cannot authorize mutation by itself.
[POSITIVE] libs/api/src/abi/syscall.rs:651 — `Snapshot` still reuses allowlist bit 32 only as a dispatch allowlist bit, while the comment now correctly names the kernel `SupervisorCap` gate as the authority boundary.
[POSITIVE] libs/api/src/abi/syscall.rs:773 — raw syscall 420 still maps to `ViSyscall::Snapshot`; no semantic ABI renumbering observed.
[POSITIVE] cells/services/supervisor/src/main.rs:78 — snapshot IPC is accepted only from a live sender whose process-table name exactly equals `shell`, then strict request parsing runs before `snapshot::run()`.
[POSITIVE] cells/services/supervisor/src/protocol.rs:73 — snapshot request parsing accepts only opcode `0x02` plus all-zero tail, matching the full-buffer App SDK receive behavior and rejecting stale-tail payloads.
[POSITIVE] cells/tools/shell/src/snapshot_client.rs:32 — shell waits with `sys_recv_timeout(supervisor_tid, ...)`, rejects replies from other senders, and reports success only for the exact `[OP_STATUS, SNAPSHOT_STATUS_PHASE, STATUS_OK]` status tuple.
[POSITIVE] cells/services/supervisor/src/snapshot.rs:11 — any kernel snapshot error maps to bounded `STATUS_UNAVAILABLE`, preventing wrapped `usize::MAX` or false success reporting.
[POSITIVE] cells/tests/bench/src/scenarios/snapshot_authority.rs:22 — the runtime witness calls `sys_snapshot()` from an allowlisted non-supervisor bench role and passes only on `PermissionDenied`, proving the `SupervisorCap` denial path.
[POSITIVE] tests/integration/tests/launch-profile.rs:101 — QEMU runs `bench snapshot-authority`, waits for the kernel `no SupervisorCap` denial, and asserts no `[snapshot] wrote` or wrapped-frame success appears.
[POSITIVE] kernel/src/task/syscall.rs:4090 — disk format, snapshot serialization, and boot restore code are not changed by this diff; only dispatch authorization and error logging wrap the existing call.

Accepted residual risk: real MMC snapshot persistence remains deferred by scope; this review treats the QEMU unavailable/denial proof as an authority witness, not as hardware write/restore evidence.

Verification: `git diff --check` passed for tracked files; `cargo test -p api --target x86_64-unknown-linux-gnu snapshot --lib` compiled and ran zero matching tests; binary-only cell test attempts hit the existing no_std/std test harness conflict (`duplicate lang item ... panic_impl`). Existing tester evidence remains the runtime source of truth: 17/17 pass, launch-profile 1/1, hotswap-smoke 15/15, and no `[snapshot] wrote`.
