---
phase: 03
title: "Snapshot trigger authority to Supervisor"
status: complete
priority: P2
effort: "2-3d"
dependencies: [00]
tier: thinking
---

# Phase 03 — Snapshot Trigger Authority → Supervisor

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs. Snapshot authority is a Law 1 semantic ABI change; stop at both confirmation checkpoints before implementation.

## Context Links
- Plan: [plan.md](plan.md)
- OBSERVED: `Snapshot=420` is stable ABI in `ViSyscall` (`libs/api/src/abi/syscall.rs:157-159`) and maps from raw id 420 (`libs/api/src/abi/syscall.rs:772`).
- OBSERVED: Snapshot allowlist reuses bit 32 (`libs/api/src/abi/syscall.rs:645-652`), and `SyscallSet::ALL` is the permit-all sentinel for cells without a syscall section (`libs/api/src/abi/syscall.rs:520-524`; loader default `kernel/src/loader.rs:208-217`).
- OBSERVED: `Syscall::Snapshot` dispatch currently has no `caller_has_spawn` or `caller_has_supervisor` check; it directly calls `serialize_snapshot()` (`kernel/src/task/syscall.rs:4083-4089`).
- OBSERVED: shell manifest is `spawn=false` (`cells/tools/shell/src/main.rs:10-17`) and explicit `declare_syscalls!` omits `Snapshot` (`cells/tools/shell/src/main.rs:22-58`), while the builtin still calls `sys_snapshot()` directly (`cells/tools/shell/src/executor.rs:612-623`).
- OBSERVED: `ostd::runtime::app_syscall_set(..., spawn=true)` still adds `Snapshot` by default (`libs/ostd/src/runtime.rs:42-47`, `libs/ostd/src/runtime.rs:82-100`).
- OBSERVED: supervisor has `SupervisorCap` by loader path and `spawn=true` manifest (`cells/services/supervisor/src/main.rs:1-5`, `cells/services/supervisor/src/main.rs:130-141`); current handler only accepts `OP_HOTSWAP` (`cells/services/supervisor/src/main.rs:39-70`).
- OBSERVED: protocol stub says `SnapshotRequest { target_service: [u8;64] }` (`cells/services/supervisor/src/protocol.rs:3-18`), but snapshot is global whole-RAM state, not per-service.
- OBSERVED: QEMU path has no kernel block device: `NullBlock` read/write return `NotFound`; `block_device()` returns MMC only if present, else `NullBlock` (`kernel/src/task/drivers/block.rs:8-24`, `kernel/src/task/drivers/block.rs:36-56`).
- OBSERVED: save writes all allocated frames via `block::write_sector()` and logs success only after header write (`kernel/src/snapshot.rs:91-181`); restore runs before cells and cold-boots when `block::read_sector()` fails (`kernel/src/main.rs:532-539`, `kernel/src/snapshot.rs:195-200`).

## Overview
- **Priority:** P2. **Status:** complete. **Risk:** HIGH because this corrects a false security assumption: `Snapshot=420` is currently allowlist-gated but not capability-gated.
- Scope is trigger authority only. Kernel keeps frame-walk, CRC, fixed LBA format, and boot `try_restore()` because snapshot is privileged global RAM state and restore is pre-cell.
- Deliverable: shell asks Supervisor IPC; Supervisor authenticates exact shell sender; only Supervisor can call `sys_snapshot()`; QEMU proof reports NullBlock/unavailable honestly; real MMC success remains host-gated.

## Key Insights
- The old plan's "SpawnCap-gated Snapshot" claim is false. The dispatch arm calls `serialize_snapshot()` without any capability check (`kernel/src/task/syscall.rs:4083-4089`).
- Current shell snapshot is already denied before the handler because the shell explicit allowlist omits `Snapshot` (`cells/tools/shell/src/main.rs:22-58`) even though it calls `sys_snapshot()` (`cells/tools/shell/src/executor.rs:612-623`).
- Allowlist alone is not authority: cells without `__ViCell_syscalls` default to `u64::MAX` permit-all (`kernel/src/loader.rs:208-217`), and spawn=true app defaults include Snapshot (`libs/ostd/src/runtime.rs:82-100`).
- Snapshot request must be opcode-only. The existing `{ target_service: [u8;64] }` stub is semantically wrong for a whole-RAM snapshot and is not deployed because the supervisor handler ignores `OP_SNAPSHOT`.

## Requirements
- Functional:
  - Add a kernel dispatch guard: `Snapshot=420` succeeds only when `caller_has_supervisor(caller_id)` is true; keep opcode 420, allowlist bit 32, OSTD wrapper name, on-disk format, and boot restore path unchanged.
  - Change shell `snapshot` builtin from direct syscall to Supervisor IPC.
  - Add supervisor `OP_SNAPSHOT` handler that accepts only the exact shell sender identity before calling `sys_snapshot()`.
  - Replace the unused `SnapshotRequest { target_service }` stub with opcode-only request bytes and a bounded status reply.
- Non-functional:
  - No simulated snapshot success. QEMU expected result is unavailable/NullBlock, not `[snapshot] wrote N frames`.
  - No restore-path redesign, no per-service snapshot targeting, no new disk protocol, no new syscall number.
  - Preserve `#![forbid(unsafe_code)]` in Supervisor.

## Architecture / Data Flow
```
shell "snapshot"
  -> lookup service::SUPERVISOR (11)
  -> send [OP_SNAPSHOT]
  -> supervisor AppEvent::Message
      -> authenticate sender task name == "shell"
      -> call ostd::syscall::sys_snapshot()
  -> kernel Snapshot=420
      -> allowlist bit 32 still checked
      -> NEW caller_has_supervisor(caller_id) check
      -> serialize_snapshot()
          -> QEMU: NullBlock write fails -> Err(Unknown/unavailable status)
          -> real MMC: frame writes may succeed; restore remains host-gated proof
  -> supervisor sends OP_STATUS reply
  -> shell prints success or explicit unavailable/denied status

direct non-supervisor caller with Snapshot allowlist
  -> reaches Snapshot handler
  -> caller_has_supervisor false
  -> PermissionDenied
  -> no snapshot mutation
```

## Related Code Files
- Modify `kernel/src/task/syscall.rs`: add `caller_has_supervisor` guard to `Syscall::Snapshot`; add unit/handler proof for allowlisted non-supervisor denial if feasible in this file.
- Modify `libs/api/src/abi/syscall.rs`: correct Snapshot allowlist comment from SpawnCap wording to Supervisor authority wording only; keep number 420 and bit 32.
- Keep `libs/ostd/src/runtime.rs` and allowlist bit 32 unchanged for compatibility. An allowlist bit permits dispatch only; the new kernel `SupervisorCap` check remains the authority boundary.
- Modify `cells/tools/shell/src/executor.rs`: route `snapshot` through Supervisor IPC and print precise denied/unavailable status.
- Modify `cells/tools/shell/src/main.rs`: add only the syscalls needed for Supervisor IPC/reply if missing; do not add `Snapshot`.
- Modify `cells/services/supervisor/src/main.rs`: add `OP_SNAPSHOT` arm and exact sender-name authentication for shell.
- Modify `cells/services/supervisor/src/protocol.rs`: change snapshot request to opcode-only and keep bounded status reply.
- Create `cells/services/supervisor/src/snapshot.rs`: keep syscall result mapping out of `main.rs`, which is already near the project file-size limit.
- Modify `cells/tools/shell/src/main.rs` and create `cells/tools/shell/src/snapshot_client.rs`: keep the bounded Supervisor IPC client out of the already-large executor; the executor change is only a builtin dispatch call.
- Modify `cells/tests/bench/src/main.rs` and add one focused scenario module: explicitly allow `Snapshot`, call it without `SupervisorCap`, and emit a bounded negative-proof marker.
- Modify `cells/tests/bench/src/scenarios.rs` and create `cells/tests/bench/src/scenarios/snapshot_authority.rs` for that negative runtime witness.
- Modify `tests/integration/tests/launch-profile.rs`: replace the stale shell-denied expectation with shell-to-Supervisor NullBlock/unavailable proof and assert the kernel denial marker for the direct bench caller.

## Implementation Steps
1. **Confirmation checkpoint 1 — Law 1 authority approval:** CONFIRMED by user on 2026-08-08: `Snapshot=420` remains stable but gains `caller_has_supervisor`; spawn/no-section allowlist cannot authorize snapshot by itself.
2. Update protocol to define `[OP_SNAPSHOT]` only and status reply codes for `ok`, `denied`, `unavailable`, and `malformed`; delete the `target_service` semantics.
3. Add supervisor snapshot handling with exact shell-sender authentication before `sys_snapshot()`. Reuse existing `sys_get_procs` sender-name lookup pattern from hotswap auth.
4. Redirect shell builtin to Supervisor IPC. Do not add `Snapshot` to shell `declare_syscalls!`.
5. Add kernel `caller_has_supervisor` guard in `Syscall::Snapshot` before `serialize_snapshot()`.
6. Correct the stale Snapshot authority comment in `libs/api`; retain the existing allowlist bit and runtime defaults so this phase changes authority only once, at the kernel capability gate.
7. Add the negative proof: a non-supervisor caller whose allowlist permits `Snapshot` reaches the handler, causes an explicit kernel `no SupervisorCap` denial marker, and receives failure; verify no `[snapshot] wrote` log and no header invalidation/write path is invoked.
8. **Confirmation checkpoint 2 — implementation-ready gate:** CONFIRMED by user on 2026-08-08 for the exact file list, QEMU NullBlock/unavailable proof wording, and deferred real-MMC success proof.
9. Run build/unit gates proportional to touched files, then QEMU proof:
   - shell -> Supervisor -> kernel attempt returns explicit NullBlock/unavailable status on QEMU;
   - unauthorized direct syscall returns `PermissionDenied`;
   - no claim of successful write/restore unless real MMC evidence exists.

## Todo List
- [x] Confirmation checkpoint 1 recorded (2026-08-08).
- [x] Protocol changed to opcode-only SnapshotRequest.
- [x] Supervisor authenticates exact shell sender and invokes `sys_snapshot()`.
- [x] Shell builtin routes through Supervisor IPC, not direct `sys_snapshot()`.
- [x] Kernel Snapshot handler adds `caller_has_supervisor`.
- [x] Snapshot allowlist comment corrected; bit 32 and runtime defaults preserved.
- [x] Negative allowlisted non-supervisor direct syscall proof added.
- [x] QEMU proof records NullBlock/unavailable honestly.
- [x] Confirmation checkpoint 2 recorded before implementation (2026-08-08).

## Success Criteria
- [x] `git grep -n "sys_snapshot(" cells/tools/shell cells/tools/sys-tools cells/apps cells/demos cells/services` shows only Supervisor-owned call sites, not shell/client direct calls.
- [x] A non-supervisor caller with allowlist bit 32 set reaches `Syscall::Snapshot`; QEMU observes the kernel `no SupervisorCap` denial marker and caller failure.
- [x] Shell `snapshot` reaches Supervisor IPC and reports the kernel snapshot result through bounded status.
- [x] QEMU run proves unavailable/NullBlock behavior without logging or claiming `[snapshot] wrote N frames`.
- [x] Real MMC successful save/restore is explicitly deferred unless run on real MMC hardware with preserved logs.
- [x] No change to snapshot disk format, `Snapshot=420`, allowlist bit 32, or boot `try_restore()` path.

## Risk Assessment
- **HIGH — false authority boundary persists if only shell routing changes.** Mitigation: kernel `caller_has_supervisor` guard is mandatory before success.
- **HIGH — QEMU false-green.** Mitigation: expected QEMU success is reaching the path and returning unavailable; real write/restore proof is host-gated and must not be simulated.
- **MED — backwards compatibility for spawn=true apps.** Mitigation: keep opcode/wrapper stable; behavior change is denial for non-supervisors, documented as Law 1 semantic ABI authority change.
- **MED — supervisor down means shell cannot snapshot.** Mitigation: shell reports supervisor unavailable; snapshot is operator-triggered and non-critical.
- **Rollback:** revert shell IPC routing, supervisor `OP_SNAPSHOT`, kernel guard, and comment/default changes. On-disk snapshots and boot restore are unchanged. Cannot undo: any real MMC snapshot image written during testing; invalidate it with existing snapshot invalidation path or cold-boot procedure if needed.

## Security Considerations
- Full-RAM snapshot is sensitive global state. Authority must be kernel-enforced, not only caller UX or syscall allowlist.
- Exact shell-sender authentication prevents arbitrary clients from using Supervisor as a snapshot broker.
- No per-service argument is accepted because it would imply selective snapshot semantics the kernel does not implement.

## Assumptions
- **Claim:** exact sender task name for the interactive shell remains `"shell"`.
  **Confidence:** medium.
  **How to verify:** boot/QEMU process table or inspect cell spawn name in init/embedded image generation before Build.
- **Claim:** a narrow non-supervisor allowlisted test cell can fit an existing integration lane without broad fixture churn.
  **Confidence:** medium. **How to verify:** inspect `tests/integration/tests/*` and existing cells/tests manifests during Build.

## Verification Matrix
- Unit: protocol parser/status encoding accepts only opcode-only SnapshotRequest; rejects trailing payload if parser is strict.
- Kernel unit or selftest: `Syscall::Snapshot` with allowlist bit 32 and no SupervisorCap returns `PermissionDenied`.
- Integration/QEMU: shell sends `OP_SNAPSHOT`; supervisor authenticates shell; kernel returns unavailable on NullBlock; shell prints unavailable.
- Integration/QEMU negative: direct non-supervisor `sys_snapshot()` is denied and no snapshot write log appears.
- Host-gated: real MMC save writes frames and boot restore works; deferred until real board/MMC access exists.

## Next Steps
- Phase 04 is next and remains pending. Phase 03 does not unblock claims of snapshot write/restore success on QEMU.

## Deviation Log
- Surprise: re-grep disproved the old SpawnCap-gated Snapshot assumption.
  Why: `Syscall::Snapshot` directly calls `serialize_snapshot()` with no cap check.
  Impact: phase risk raised to HIGH and implementation must add kernel authority gate.
  Revert: plan-only correction; no code touched.
- Decision: Law 1 checkpoint 1 approved on 2026-08-08.
  Scope: keep syscall 420, allowlist bit 32, wrapper, disk format, and boot restore stable; add kernel-enforced `SupervisorCap` authority and route shell through Supervisor IPC.
  Impact: exact implementation and proof file list remains gated by checkpoint 2.
- Decision: Law 1 checkpoint 2 approved on 2026-08-08.
  Scope: twelve-file implementation/proof boundary accepted; QEMU must report NullBlock/unavailable honestly, direct non-supervisor denial must be runtime-visible, and real-MMC save/restore remains deferred.
  Impact: Phase 03 may enter Build without expanding into snapshot format, restore, or block-driver redesign.
- Surprise: `AppContext` exposes the full receive buffer and does not clear bytes beyond a short message before the next receive.
  Why: `parse_event_owned` copies `buf[2..]`, while kernel receive copies only the current message length; strict opcode-only parsing can otherwise see stale non-zero tail bytes from an earlier request.
  Decision: the shell snapshot client sends one full zero-filled `IPC_BUF_SIZE` envelope with only the App prefix and `OP_SNAPSHOT` set, preserving strict all-zero-tail validation without expanding into `libs/ostd`.
  Revert: return to a short envelope only if AppContext later exposes message length or clears its receive buffer before every receive.
- Verification: final diff-aware validation passed 17/17 cases with zero failures and zero skips on 2026-08-08.
  Evidence: the launch-profile test observed the shell unavailable status, kernel `no SupervisorCap` denial, and bench PASS marker while excluding `[snapshot] wrote`; the full hotswap-smoke suite remained 15/15 green.
  Boundary: real MMC save/restore remains deferred. Coverage was not rerun because its baseline already fails on the missing Lua `signal.h` and absent tetris-c source; no new coverage regression was introduced.
  Gap: syscall 420 and allowlist bit 32 have source and QEMU coverage but no standalone host-runnable API unit test.
- Review: production-readiness and independent security/authority passes both returned PASS with no findings on 2026-08-08.
  Evidence: both passes confirmed the kernel gate dominates mutation, shell IPC is bounded, exact sender authentication precedes the syscall, the negative witness reaches the capability gate, and status mapping cannot claim QEMU write success.
  Tooling warning: the hc-cook artifact validator entrypoint `kit/hooks/haily-artifact.cjs` is not present in this checkout or the installed skill bundle, so the finalize gate could not be executed after two resolution attempts; all five JSON artifacts were written and parsed successfully.
