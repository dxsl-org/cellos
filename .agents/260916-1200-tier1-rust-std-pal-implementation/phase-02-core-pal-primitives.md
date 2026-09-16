---
phase: 2
title: "Core PAL Primitives (Init, Alloc, Yield, Time)"
status: completed
priority: P1
effort: "1d"
dependencies: [1]
tier: medium
---

# Phase 02: Core PAL Primitives (Init, Alloc, Yield, Time)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview
Implements foundational PAL hooks for Tier 1 Cell execution: runtime lifecycle (`init`/`cleanup`/`abort`), per-cell memory allocation (`PAL-007`), thread yielding and query (`PAL-025`), and monotonic time (`PAL-027`).

## Requirements
- Functional:
  - `init`: Once-only runtime initialization hook verifying stack alignment and loader argument invariants.
  - `abort_internal`: Abort execution immediately without unwinding or address leakage.
  - `alloc`: Bind `std::alloc::System` to `ostd` per-cell freeing heap allocator; enforce zero-sized allocation rules and abort on OOM.
  - `thread`: Return `available_parallelism = NonZeroUsize::new(1).unwrap()`; hook `yield_now` directly to `ViSyscall::Yield`.
  - `time`: Back `std::time::Instant` with `ViSyscall::GetTime` (monotonic scheduler ticks normalized to nanoseconds).
- Non-functional:
  - Zero heap allocation overhead on `yield_now` and `Instant::now`.
  - Maintain strict single-task, no-thread concurrency model.

## Architecture
```text
std::alloc::System ────────► ostd::heap::CellAllocator (per-cell heap)
std::thread::yield_now ────► ecall/svc (ViSyscall::Yield)
std::thread::available_parallelism ─► NonZeroUsize(1)
std::time::Instant::now ───► ecall/svc (ViSyscall::GetTime)
```

## Assumptions
- **Claim:** `ViSyscall::GetTime` is permitted in the baseline syscall allowlist for all cells.
  **Confidence:** high
  **How to verify:** Checked `libs/ostd/src/runtime.rs:56` where `ViSyscall::GetTime` is part of `base_syscall_set`.
- **Claim:** `ostd` per-cell heap allocator conforms to Rust global allocator contract.
  **Confidence:** high
  **How to verify:** `libs/ostd/src/heap.rs` implements `core::alloc::GlobalAlloc`.

## Related Files
- Modify: `patches/rust-std-cellos.patch`
- Create in patch: `library/std/src/sys/pal/cellos/alloc.rs`
- Create in patch: `library/std/src/sys/pal/cellos/common.rs`
- Create in patch: `library/std/src/sys/pal/cellos/thread.rs`
- Create in patch: `library/std/src/sys/pal/cellos/time.rs`

## Implementation Steps
1. Implement `common.rs`:
   - `unsafe fn init(argc: isize, argv: *const *const u8, sigpipe: u8)`: validates bounds.
   - `unsafe fn cleanup()`: no-op or runs registered dtors.
   - `fn abort_internal() -> !`: invokes `core::intrinsics::abort()`.
2. Implement `alloc.rs`:
   - Define `System` allocator calling `vi_alloc(layout.size(), layout.align())` and `vi_dealloc(ptr, layout.size(), layout.align())`.
3. Implement `thread.rs`:
   - Define `pub fn available_parallelism() -> io::Result<NonZeroUsize>` returning `Ok(NonZeroUsize::new(1).unwrap())`.
   - Define `pub fn yield_now()` executing `sys_yield()`.
4. Implement `time.rs`:
   - Define `Instant` struct storing `u64` ticks; implement `now()` and subtraction with `Duration`.

## Success Criteria
- [x] Patch includes `alloc.rs`, `common.rs`, `thread.rs`, `time.rs`.
- [x] Unit tests in a test harness demonstrate `Box::new()`, `vec![]`, `Instant::now()`, and `thread::yield_now()` compile and execute.
- [x] `available_parallelism().get()` returns exactly `1`.
## Security Considerations
Allocation ownership must never cross cell boundaries; abort must never dump memory contents to untrusted sinks.

## Risk Notes
Timer frequency calibration must match target board clock; default to 10 MHz (`MTIME_TICKS_PER_MS = 10_000`) on RISC-V.

## Deviation Log
None.
