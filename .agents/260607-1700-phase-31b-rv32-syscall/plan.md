---
title: "Phase 31b: RV32 Syscall Dispatch"
description: "Replace the riscv32 ViCell_syscall_dispatch stub with a real dispatcher mirroring RV64."
status: completed
priority: P2
effort: 3d
branch: main
tags: [rv32, syscall, hal, kernel, nano]
created: 2026-06-07
completed: 2026-06-07
---

# Phase 31b — RV32 Syscall Dispatch

Phase 31 (RV32 HAL + ViCell-Nano, commit f5d2f588) booted to S-mode with a working
trap handler, but `ViCell_syscall_dispatch` on riscv32 is a no-op stub
(`syscall.rs:1804-1808`). No cell can issue a syscall on RV32. This phase wires the
RV32 trap path into the existing arch-agnostic `handle_syscall`, so the same 50+
`ViSyscall` variants work on `riscv32imac` exactly as on `riscv64`.

Scope is one phase (~3 days). No new syscall variants, no `libs/api`/`libs/types`
changes (Law 1 avoided). Reuses `handle_syscall`, `ViSyscall::from`, `allowlist_bit`,
`current_task_id`, `system_ticks` — all already arch-agnostic.

## Phases

| # | Phase | Status | Effort | Description | BlockedBy |
|---|-------|--------|--------|-------------|-----------|
| 01 | [RV32 syscall dispatch](phase-01-rv32-syscall-dispatch.md) | completed | 3d | Replace riscv32 stub with real dispatcher: u32→usize arg promotion, allowlist gate, SUM enable (riscv32 arm), result write-back | none (Phase 31 done) |

## Key Dependencies

- **Phase 31 complete** (commit f5d2f588): RV32 trap handler `vi_trap_handler32`
  (`hal/.../rv32/trap.rs:48`) already routes ecall → `vi_handle_syscall` →
  `ViCell_syscall_dispatch` and advances `sepc += 4` afterward. No trap.rs change
  expected.
- **Arch-agnostic core** (already shipped): `handle_syscall` (`syscall.rs:427`),
  `ViSyscall::from` + `allowlist_bit` (`libs/api`), `current_task_id`
  (`task.rs:470`), `system_ticks`. None require modification.

## Out of Scope

- No new `ViSyscall` variants; no `libs/api` / `libs/types` edits (Law 1).
- No SMP / multi-hart RV32 (single-core Nano only).
- No RV32 userspace cell ELF authoring — covered by the existing cell build; this
  phase only makes the kernel side answer syscalls.

## Unresolved Questions

- Does any current syscall path use a value wider than 32 bits in an RV32-relevant
  arg slot (e.g. `BlkRead.sector: u64` packed from a0 only)? See phase-01 Risk R3.
