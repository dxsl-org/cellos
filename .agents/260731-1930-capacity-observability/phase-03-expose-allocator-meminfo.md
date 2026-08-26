---
phase: 3
title: "Expose allocator MemInfo"
status: completed
priority: P1
effort: "4h"
dependencies: [1, 2]
tier: thinking
---

# Phase 3: Expose Allocator MemInfo

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs.

## Overview

Maintain exact frame usage, expose a fixed-width global snapshot through MemInfo, and make the benchmark consume that real value.

## Requirements

- Functional: report total/used/free frames and page size with `total = used + free`; benchmark deletes the compile-time approximation and propagates syscall failure.
- Non-functional: fixed layout across RV32/RV64/AArch64/x86_64, opt-in allowlist, one allocator lock per snapshot, no scheduler/frame lock inversion.

## Architecture

Add an internal `used_frames` counter to `FrameAllocator`, initialized through actual bitmap transitions and updated only when a bit changes. This keeps MemInfo O(1) and prevents double-free underflow. Snapshot all four fields while holding `FRAME_ALLOCATOR` once.

Add stable `MemInfo=243`, bit 56, and `ViMemInfoV1` only after Phase 1. The kernel validates `out_ptr/out_len`, copies the fixed-width struct, and returns its byte size. ostd owns the aligned local struct. The benchmark explicitly declares MemInfo and reports `used_frames * page_size` without subtracting the heap or another baseline.

## Assumptions

- **Claim:** allocator-committed bytes are the intended A3 capacity signal even though they are not resident “kernel + 3 services” bytes.
  **Confidence:** high
  **How to verify:** confirmation package in Phase 1 explicitly names this basis and its likely 10 MiB failure.

## Related Files

- Modify: `kernel/src/memory/frame.rs`
- Modify: `kernel/src/task/syscall.rs`
- Modify: `libs/api/src/abi/syscall.rs`
- Modify: `libs/api/src/abi/syscall_tests.rs`
- Modify: `libs/ostd/src/syscall.rs`
- Modify: `libs/ostd/src/runtime.rs`
- Modify: `cells/tests/bench/src/main.rs`
- Modify: `cells/tests/bench/src/scenarios/memory_footprint.rs`

## Implementation Steps

1. Add transition-aware used-frame accounting and accessors; cover initialization, single/contiguous/aligned/range allocation, deallocation, and double-free.
2. Append opcode 243, bit 56, and documented `ViMemInfoV1`; move the “unknown 243” pin to 244 and add discriminant/layout/bit tests.
3. Add kernel enum/mapping/decode/handler coverage and safe byte-copy validation for null, short, and overflow buffers.
4. Add `sys_mem_info()` plus an assertion that coarse app/service profiles never grant bit 56 implicitly.
5. Add MemInfo only to the benchmark orchestrator allowlist; delete `APPROX_BOOT_BYTES` and convert syscall failure into an explicit benchmark failure.
6. Preserve the scenario name for history, but print/document `allocator_committed_bytes` as its basis.

## Success Criteria

- [x] Allocator tests prove exact transitions and `total = used + free` without underflow.
- [x] API tests pin opcode 243, bit 56, and the 32-byte v1 layout.
- [x] Unauthorized cells are denied; the benchmark's explicit allowlist succeeds.
- [x] The emitted value is live, nonzero, and not 3,500,000.
- [x] No magic baseline or synthetic PASS remains.

## Security Considerations

MemInfo exposes coarse cross-cell activity. Keep it opt-in and return aggregate counts only; no addresses, owners, or per-cell data.

## Risk Notes

Accounting drift is possible if a bitmap transition bypasses the helpers; tests must cover every allocation method. Undo by removing opcode/bit/wrapper/counter together; already distributed binaries retain their compiled ABI expectations.

## Deviation Log

- **2026-07-31 — Decision:** `used_frames` changes only when a bitmap bit
  transitions, making repeated range marks and double frees accounting-idempotent.
- **2026-07-31 — Surprise:** The implementation compiles with the existing
  `<10 MiB` benchmark threshold unchanged; runtime is expected to report an honest
  failure because allocator-committed capacity includes the 16 MiB boot heap.
- **2026-07-31 — Deviation:** Allocator transition tests were added under the
  kernel test configuration but runtime/target execution belongs to Phase 4.
- **2026-08-01 — Closure:** Phase 4 measured 135,782,400 allocator-committed bytes
  (129.49 MiB). The mechanism is complete; the unchanged `<10 MiB` objective fails and
  remains a separate memory-reduction follow-up.
