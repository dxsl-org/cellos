---
phase: 2
title: "Surface cell-spawn OOM"
status: completed
priority: P1
effort: "3h"
dependencies: [1]
tier: thinking
---

# Phase 2: Surface Cell-Spawn OOM

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs.

## Overview

Make genuine cell-spawn memory exhaustion observable as `SyscallError::OutOfMemory` and add bounded diagnostics without redesigning unrelated syscall errors.

## Requirements

- Functional: four cell-spawn syscalls return typed OOM; permanent input/security failures do not masquerade as OOM; one source-stage and one caller/path summary log identify each refusal.
- Non-functional: preserve positive TIDs, `-1` generic errors, thread-spawn `TryAgain`, Grant sentinels, and old-cell/new-kernel compatibility.

## Architecture

Use private additive `-2` encoding in kernel and ostd; old cells still classify it as generic failure. Centralize kernel result encoding for both dispatcher widths and centralize ostd spawn decoding for all four wrappers. Fix `loader::spawn_gated` error identity before exposing the new code.

Do not log in `GlobalAlloc::alloc`. Log concrete requests at segment/stack allocation sources, then a compact syscall-boundary summary with operation, caller TID, path/name, and ELF length when available. Stop the VFS-to-bootstrap fallback when SpawnFromElf returns typed OOM.

## Assumptions

- **Claim:** a bounded RV64 fixture can exhaust cell-spawn frames without destabilizing unrelated boot gates.
  **Confidence:** medium
  **How to verify:** reuse the D5 parked `bench-probe` method with an explicit QEMU memory size during Cook recon.

## Related Files

- Modify: `kernel/src/loader.rs`
- Modify: `kernel/src/loader/elf.rs`
- Modify: `kernel/src/task/stack.rs`
- Modify: `kernel/src/task/syscall.rs`
- Modify: `libs/ostd/src/syscall.rs`
- Modify/Create: focused kernel and integration tests selected after recon

## Implementation Steps

1. Re-read live diffs in every overlapping kernel/test file; patch only task-owned hunks.
2. Replace the blanket `map_err(|_| OutOfMemory)` in `spawn_gated` with faithful propagation.
3. Append `OutOfMemory` to kernel and ostd error enums; add shared `-2` encode/decode helpers and unit cases for success, `-1`, and `-2`.
4. Map OOM faithfully in SpawnFromPath, SpawnFromElf, SpawnPinned, and SpawnFromMem; leave `Syscall::Spawn` as `TryAgain`.
5. Add allocation-safe source logs for segment and contiguous stack failures plus one syscall summary; avoid heap-lock/log recursion.
6. Return typed OOM immediately from the ostd VFS spawn path instead of retrying the bootstrap path.

## Success Criteria

- [x] All four spawn wrappers return `Err(OutOfMemory)` for kernel `-2` and `Err(Unknown)` for legacy `-1`.
- [x] Malformed/denied ELF failures remain non-OOM.
- [x] Both RV32 and 64-bit dispatchers encode OOM identically by meaning.
- [x] Thread spawn still returns `TryAgain`; Grant behavior is unchanged.
- [x] A bounded runtime refusal prints useful allocation/caller evidence and the shell remains responsive.

## Security Considerations

Logs must bound caller-controlled path/name text and must not disclose raw memory contents. Avoid retry storms and allocator-recursive logging.

## Risk Notes

False OOM is the highest risk if loader error identity is not fixed first. Undo by reverting the private `-2` path and log helpers; runtime logs already emitted cannot be withdrawn.

## Deviation Log

- **2026-07-31 — Decision:** The shared result encoder maps any internal
  `OutOfMemory` to `-2`, while only the four cell-spawn handlers intentionally
  introduce that variant today. Thread spawn and Grant sentinels remain unchanged.
- **2026-07-31 — Deviation:** The bounded runtime exhaustion fixture and
  post-refusal shell-responsiveness gate are deferred to Phase 4 specialist
  verification. This implementation pass completed focused compile checks only.
- **2026-08-01 — Closure:** Phase 4 verified typed `-2` only for cell-spawn OOM, bounded
  source and caller/path logs, no panic, and a responsive shell after exhaustion.
