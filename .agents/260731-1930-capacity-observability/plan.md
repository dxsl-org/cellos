---
title: "Capacity Observability Implementation Plan"
description: "Expose cell-spawn OOM faithfully and replace the synthetic memory benchmark with allocator-backed MemInfo telemetry."
status: completed
priority: P1
effort: 10.5h
branch: feat/wx-post-reloc-and-f1-signing
tags: [feature, api, critical]
blockedBy: []
blocks: []
created: 2026-07-31
---

# Capacity Observability Implementation Plan

## ABI Gate

**STOP before implementation.** Law 1 requires two explicit confirmations of the unchanged package below; planning authorization does not count. Confirmation 1 presents the package, then confirmation 2 repeats it immediately before the first ABI edit. Any changed value resets both confirmations.

- A2: cell-spawn OOM returns additive `-2`; legacy/generic errors remain `-1`; existing opcodes stay unchanged.
- A3: `MemInfo = 243`, allowlist bit `56`, and fixed 32-byte `#[repr(C)] ViMemInfoV1 { total_frames: u64, used_frames: u64, free_frames: u64, page_size: u64 }`.
- MemInfo is opt-in because global used/free memory is a cross-cell side channel.
- The benchmark reports allocator-committed bytes. The boot heap alone reserves 16 MiB, so the current `<10 MB` target will likely become an honest failure rather than remain a false PASS.

## Overview

A2 makes the four cell-spawn paths distinguish real memory exhaustion and emit useful, allocation-safe diagnostics. A3 adds exact global frame accounting, exposes one bounded snapshot through a stable syscall, and removes the benchmark's 3,500,000-byte compile-time constant.

## Phases

| Phase | Name | Status |
|---|---|---|
| 1 | [Ratify the ABI package](./phase-01-ratify-abi-package.md) | completed |
| 2 | [Surface cell-spawn OOM](./phase-02-surface-cell-spawn-oom.md) | completed |
| 3 | [Expose allocator MemInfo](./phase-03-expose-allocator-meminfo.md) | completed |
| 4 | [Verify runtime truth and synchronize evidence](./phase-04-verify-runtime-and-sync-evidence.md) | completed |

## Dependencies

- Phase 1 blocks every ABI-affecting edit.
- Phase 2 precedes Phase 3 because both modify syscall dispatch and ostd return decoding.
- Preserve the uncommitted A1 DTB work and concurrent IPC edits; re-read overlapping diffs before every edit and never stage or revert unrelated changes.

## Research

- `.agents/reports/a2-oom-syscall-research-260731.md`
- `.agents/reports/a3-meminfo-benchmark-research-260731.md`
- [Scout report](./scout-report.md)

## Outcome

The mechanism and runtime gates are complete. The measured allocator-committed footprint is
135,782,400 bytes (129.49 MiB), so the unchanged `<10 MiB` performance objective honestly fails.
Memory reduction is a separate follow-up; the observability result must not be redefined to make
the target pass. Evidence: `.agents/reports/a2-a3-test-260801.md`.
