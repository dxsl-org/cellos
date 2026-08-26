# Phase 00 — Replacement Inherits the Frozen Cell's CapSet

## Context Links
- Plan: [plan.md](plan.md)
- Law: `docs/specs/15-kernel-boundary.md` §1.2 (capability authority is kernel-only), §3.2
- Kernel cap grant: `kernel/src/loader.rs:240-317`
- Kernel hotswap ceiling: `kernel/src/cell/hotswap.rs:326-334`
- Supervisor spawn: `cells/services/supervisor/src/hotswap.rs:166-175`

## Overview
- **Priority**: P1 (blocks correct cutover). **Status**: complete. **Risk**: HIGH.
- The shipped Supervisor Cell spawns the replacement with `sys_spawn_from_path`, so the new cell's
  caps = `manifest ∩ supervisor caps` (`loader.rs:254-260`). The supervisor holds only SpawnCap +
  SupervisorCap. **A privileged service (vfs/net/compositor/input) hotswapped via the supervisor
  loses block_io/network/etc.** — its replacement is silently unprivileged and broken. The kernel
  `hotswap()` path does this correctly (`loader.rs:243`: "HotSwap passes the replaced cell's caps as
  the ceiling"). This phase gives the supervisor path the same guarantee **without** widening the
  supervisor's own authority.

## Key Insights
- Caps are kernel root-of-trust (§1.2 — LBI does not provide revocation/authority tracking). The
  ceiling for a replacement therefore MUST be computed in the kernel, not asserted by the supervisor.
- The clean shape: the kernel already knows the frozen cell's `CapSet` (it froze it). `sys_freeze_cell`
  can snapshot that `CapSet` under a kernel-side swap record; a replacement-spawn primitive names the
  frozen tid and the kernel uses the recorded `CapSet` as `Spawner::Ceiling`.
- Least-privilege preserved: the replacement can never exceed the original (`requested ∩ frozen_ceiling`),
  and the supervisor never needs the target's caps.

## Requirements
- Functional: replacement cell of a privileged service retains exactly the caps the original held,
  intersected with what its own manifest requests. No cap gained, none silently dropped.
- Non-functional: no widening of the supervisor's CapSet; cap authority stays in kernel; Law 1 honored.

## Architecture / Data Flow
```
supervisor: sys_pause_service(service_id, old_tid)                                  ← PauseService=422, bit 49
   kernel:  hide expected provider; reject cached-TID ingress; wait for accepted ingress to drain
supervisor: send Snapshot IPC; wait while old_tid remains runnable
supervisor: sys_freeze_cell(old_tid)
   kernel:  set_task_frozen(old_tid); RECORD swap_ceiling[old_tid] = CapSet::of_task(old)
supervisor: sys_spawn_replacement(old_tid, new_elf_path)                                     ← NEW
   kernel:  ceiling = swap_ceiling[old_tid] (missing record -> PermissionDenied; no fallback)
            spawn_from_path(path, Spawner::Ceiling(ceiling))
supervisor: ... restore/ready/register handshake, commit, kill old ...
   kernel:  on KillCell(old_tid) OR resume: clear swap_ceiling[old_tid]
```

## Related Code Files
- Modify `kernel/src/cell/hotswap.rs`: add `swap_ceiling` record keyed by tid; store on freeze, clear on kill/resume.
- Modify `kernel/src/task/syscall.rs`: `PauseService=422` atomically hides the expected provider while it stays runnable; `FreezeCell` later records the ceiling; add `SpawnReplacement` dispatch arm (SupervisorCap-gated).
- Modify `libs/api/src/abi/syscall.rs`: add `PauseService=422` / `SpawnReplacement` (next free number in 421-499; allowlist bits 49/57). **LAW 1.**
- Modify `libs/ostd/src/syscall.rs`: `sys_spawn_replacement(old_tid, path) -> SyscallResult`.
- Modify `cells/services/supervisor/src/hotswap.rs`: replace the `sys_spawn_from_path` call (Step 3, line 167) with `sys_spawn_replacement(old_tid, new_elf_path)`.

## Implementation Steps
1. Decide syscall shape (see plan Open Q1) — get 2× Law-1 confirmation BEFORE editing `libs/api`.
2. Add `SpawnReplacement` to `ViSyscall` + allowlist bit + ostd wrapper (Law 1 gate).
3. Kernel: in `FreezeCell`, snapshot `CapSet::of_task(old)` into a `swap_ceiling: Spinlock<BTreeMap<usize,CapSet>>` in hotswap.rs.
4. Kernel: `SpawnReplacement` arm — SupervisorCap gate; look up `swap_ceiling[old_tid]`; call `loader::spawn_from_path(path, Spawner::Ceiling(ceiling))`; on missing record, fail closed (`PermissionDenied`) so a stray call cannot spawn with ambient authority.
5. Kernel: clear `swap_ceiling[old_tid]` in `KillCell` and `unfreeze_task` (both swap-terminal points).
6. Supervisor: swap Step 3 to `sys_spawn_replacement`.
7. Keep kernel `hotswap()` path untouched (still the fallback until Phase 04).

## Todo List
- [x] Law-1 confirm syscall shape
- [x] `SpawnReplacement` ABI + allowlist bit + ostd wrapper
- [x] `swap_ceiling` record: store on freeze
- [x] `SpawnReplacement` kernel arm (fail-closed on missing record)
- [x] Clear record on kill + resume
- [x] Supervisor uses `sys_spawn_replacement`
- [x] QEMU SpawnReplacement E2E proves privileged SpawnCap retention
- [x] State-preserving hotswap replay preserves the demo state under QEMU
- [x] Build + boot 3 arches; reliability suite green

## Success Criteria
- Hotswap the privileged SpawnCap probe cell via the supervisor → replacement reports `[hotswap-demo-v2] SpawnCap retained` (assert via QEMU log or a cap-probe syscall in the replacement).
- `sys_spawn_replacement` with no matching frozen record returns `PermissionDenied` (fail-closed).
- Supervisor CapSet unchanged (still spawn + supervisor only).

## Verification Notes
- Observed in this session: fresh `gen_disk.ps1` succeeded after required artifact checks included the actual `bench` binary; failed optional Tetris-C/Tetris-Lua outputs were omitted instead of packaging stale artifacts.
- Observed in this session: QEMU `supervisor_hotswap_preserves_demo_state` passed with `v1` counter 5 -> `v2` counter 5.
- Observed in this session: `PauseService=422` is SupervisorCap-gated with allowlist bit 49, the compare/pause path is present, and supervisor order is pause mapping -> snapshot runnable old -> hard `FreezeCell`/cap ceiling -> `SpawnReplacement` -> restore/ready/register.
- Observed in this session: QEMU `[hotswap-demo-v2] SpawnCap retained` passed alongside `supervisor_hotswap_preserves_demo_state` (v1 counter 5 -> v2 counter 5).
- Observed in this session: QEMU `[hotswap-cached-sender] PASS: paused old tid rejected` proves a cached provider tid cannot mutate state after the quiesce barrier.

## Risk Assessment
- **Ceiling record leak** (freeze without terminal) → stale `CapSet` retained. Mitigation: clear on both kill and resume; bound the map (reject freeze if a live record exists for that tid — one swap per cell at a time).
- **Race: old cell exits between freeze and replacement-spawn** → record still valid (caps of a dead cell are an upper bound; safe). Fallback path already tolerated in kernel `hotswap.rs:382-387`.
- **Law 1 churn** — mitigated by reusing allowlist bit 49 (SupervisorCap group) if the bit-packing allows.

## Security Considerations
- Fail-closed on missing record is mandatory: otherwise `SpawnReplacement` becomes an ambient-authority spawn. The record is the capability that authorizes inheriting the frozen cell's caps.
- `is_critical` cells cannot be frozen (`tcb.rs:314`), so init/kernel caps can never be captured into a swap record.

## Next Steps
Unblocks Phase 01 (drain) and Phase 02 (cutover). With this complete, privileged-service hotswap preserves the frozen cell's cap ceiling and the remaining work is message draining plus CLI/shell cutover.
