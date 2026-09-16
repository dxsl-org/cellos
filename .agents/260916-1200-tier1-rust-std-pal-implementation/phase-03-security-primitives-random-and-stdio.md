---
phase: 3
title: "Security Primitives (PAL-019 Random & StdIO)"
status: completed
priority: P1
effort: "1d"
dependencies: [1, 2]
tier: thinking
---

# Phase 03: Security Primitives (PAL-019 Random & StdIO)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview
Implements secure system randomness under `PAL-019` and `PAL-031` constraints, along with controlled Standard I/O routing for Tier 1 Cells.

## Requirements
- Functional:
  - `random`: Implement `fill_bytes` / `getrandom` backed by `ViSyscall::GetRandom`.
  - `PAL-019`: Governed release tuple forbids `dev-weak-rng`; unavailable entropy fails closed with an error.
  - `PAL-031`: Validate caller buffer pointer, bounds, and writable mapping prior to issuing syscalls.
  - `stdio`: Route `stdout` and `stderr` writes to `ViSyscall::Log`; format line by line or buffer in small chunks without unbounded allocations.
- Non-functional:
  - Prevent any synthetic success or unverified random fallback.
  - Reject null, overflowed, kernel-mapped, or foreign-cell pointers before kernel boundary.

## Architecture
```text
std::io::stdout().write(buf) ──► sys_log(chunk) ──► kernel UART / log queue
std::io::stdin().read(buf)    ──► sys_recv(input_service) / ErrorKind::Unsupported
std::sys::pal::random::fill   ──► [PAL-031 Validate Buffer] ──► sys_get_random(buf)
```

## Assumptions
- **Claim:** `ViSyscall::GetRandom` fills up to 64 bytes per call and returns actual bytes written.
  **Confidence:** high
  **How to verify:** `libs/ostd/src/syscall.rs:1880-1890` documents min(len, 64) per invocation.
- **Claim:** Production images build without `dev-weak-rng`.
  **Confidence:** high
  **How to verify:** Checked `tests/rust-std-promotion` test fixtures for release tuple validation.

## Related Files
- Modify: `patches/rust-std-cellos.patch`
- Create in patch: `library/std/src/sys/pal/cellos/random.rs`
- Create in patch: `library/std/src/sys/pal/cellos/stdio.rs`

## Implementation Steps
1. Implement `random.rs`:
   - Enforce buffer slice validation: pointer must not be null, slice must fit within cell address range (`PAL-031`).
   - Loop calling `ViSyscall::GetRandom` in chunks up to 64 bytes.
   - If syscall returns zero or negative, return `io::Error::from_raw_os_error(EIO)` fail-closed (`PAL-019`).
2. Implement `stdio.rs`:
   - Define `Stdin`, `Stdout`, `Stderr` structures.
   - `Stdout::write`: split into valid UTF-8 chunks (or raw bytes) and invoke `sys_log`.
   - `Stderr::write`: invoke `sys_log` with diagnostic prefix.
   - `Stdin::read`: return `io::ErrorKind::Unsupported` if no input capability held.

## Success Criteria
- [x] `println!("Hello from std!")` successfully prints to CellOS console output.
- [x] Hostile random calls with null/invalid pointers are caught before execution or return explicit errors.
- [x] Entropy failure does not fall back to synthetic PRNG in release mode.

## Security Considerations
`PAL-019` and `PAL-031` are critical security gates. No weak RNG fallback is permitted in production builds; all buffer pointers must be checked before issuing syscalls.

## Risk Notes
`sys_log` takes string slices; non-UTF8 writes to stdout should be safely escaped or lossily converted to avoid kernel panics.

## Deviation Log
None.
