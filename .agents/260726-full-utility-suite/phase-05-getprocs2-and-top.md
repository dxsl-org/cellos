---
phase: 5
title: "GetProcs2 and top"
status: complete
effort: "4-6 days"
---

# Phase 5 — GetProcs2 and Top

## Context Links

- Plan: [plan.md](plan.md)
- Stable ABI: `libs/api/src/abi/syscall.rs`
- Kernel process enumeration/accounting: `kernel/src/task/{tcb,scheduler,syscall}.rs`
- Existing top: `cells/tools/shell/src/commands.rs`

## Overview

- **Priority:** P1
- Add backward-compatible telemetry and turn `top` into a useful interactive/batch observer.

## Key Insights

- Growing `ProcessInfo` in place is ABI-unsafe; `GetProcs2` must be separate.
- CPU% is a userspace delta of cumulative scheduler ticks.
- The exported memory value is an **owned-memory footprint**, not RSS: heap plus task-owned
  stack/segment allocation ranges. It excludes grants, DMA and shared pages.

## Requirements

- Preserve `GetProcs=30`, its allowlist mapping, and `ProcessInfo` layout exactly.
- Add `GetProcs2=239`, allowlist bit 55, and `#[repr(C)]` flat `ProcessInfoV2` containing only
  fixed-width fields: id/state/explicit padding/name, sample ticks, cumulative CPU ticks, heap
  bytes, and `owned_bytes`; pin `size_of` and `align_of` for both ABI versions.
- Add cumulative `cpu_run_ticks` at the scheduler charge point with saturating/wrapping-safe deltas.
- Define CPU% as `task_delta / (wall_sample_delta * online_hart_count) * 100`, so 100% means all
  online CPU capacity; clamp display to 100% and test one- and multi-hart samples.
- `top`: CPU%, heap/MEM, `-b`, `-n COUNT`, `-d SECS`, and
  `-o cpu|mem|heap|pid|state|name`; interactive `q/Q` remains.
- Explicit telemetry permission in shell manifest; keep bit 55 out of
  `ostd::runtime::app_syscall_set(..., spawn=true)` defaults and deny callers lacking it.

## Architecture

Kernel snapshots flat rows only. `ostd` exposes `sys_get_procs2`; shell samples twice and performs
delta, sorting, formatting and batch policy. Old callers remain on v1.

## Related Code Files

- **Modify:** `libs/api/src/abi/{syscall,syscall_tests}.rs`, `libs/ostd/src/syscall.rs`
- **Modify:** `kernel/src/task/{tcb,scheduler,syscall,tests}.rs`,
  `kernel/src/memory/cell_quota.rs`
- **Create:** `cells/tools/shell/src/top.rs`
- **Modify:** `cells/tools/shell/src/{commands,main,shell_test}.rs`
- **Docs:** roadmap, PDR, changelog, architecture as warranted

## Implementation Steps

1. After the second explicit Law-1 confirmation, pin old/new ABI numbers, layouts and allowlist
   bits with compile-time/tests.
2. Test `ViSyscall::from(239)`, argument decode/dispatch, handler routing, and bit-55 deny/allow.
3. Add cumulative CPU and owned-memory accounting; snapshot without holding locks across copies.
4. Add `ostd` wrapper and shell permission.
5. Extract pure top option/sampling/sorting/render helpers; add interactive and batch front ends.
6. Run host tests, shell guest integration, ABI/scheduler tests, and RV64/AArch64/x86_64 checks.
7. Sync all phase/plan/docs status using runtime evidence; do not mark verified on compile alone.

## Todo List

- [ ] ABI v2 added without v1 drift
- [ ] CPU/owned-memory accounting
- [ ] top modes/sorting
- [ ] full tests/build/runtime evidence
- [ ] plan/docs synchronized

## Success Criteria

- [ ] ABI tests prove old layout/opcode unchanged and new opcode/bit exact.
- [ ] `top -b -n 2 -d 1 -o cpu` terminates, sorts, and shows CPU/heap/MEM.
- [ ] Shell pipelines and all new utility tests pass in the dedicated QEMU lane.
- [ ] Targeted crates build cleanly for riscv64gc, aarch64, and x86_64.

## Risk Assessment

- Scheduler hot-path overhead: one cumulative integer update per charge point; benchmark/check.
- MEM is an owned allocation footprint, not RSS or security/billing accounting.
- Stable ABI implementation remains blocked until the second explicit confirmation; rollback
  removes only v2 callers/opcode.

## Security Considerations

- Rich telemetry is opt-in via bit 55 and bounded by caller-provided row capacity.
- Kernel validates destination ownership/length before writing rows.

## Next Steps

After review/test gates, update the completed v1 plan cross-reference and ask before committing.
