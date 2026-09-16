---
phase: 5
title: "Workload Parity, Benchmarking & QEMU Validation"
status: pending
priority: P1
effort: "1d"
dependencies: [1, 2, 3, 4]
tier: medium
---

# Phase 05: Workload Parity, Benchmarking & QEMU Validation

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview
Demonstrates live execution of Rust `std` Cells on QEMU, proves workload parity against equivalent `no_std` baselines, and validates that syscall/IPC p99 overhead meets the $\le 5\%$ promotion gate.

## Requirements
- Functional:
  - Create a benchmark cell suite (`cells/demos/std-smoke` or parity workload) compiled against both `no_std` and custom `std`.
  - Execute on RISC-V and AArch64 QEMU under standard test-runner topology.
  - Measure execution latency and syscall throughput over ≥30 repetitions after ≥5 warmup cycles.
  - Validate benchmark results with `scripts/validate-rust-std-promotion.py`.
- Non-functional:
  - p99 regression of `std` versus `no_std` across identical operations must be $\le 5\%$.
  - Noise rejection: reject runs with $>2\%$ baseline p99 drift between bracketing controls.

## Architecture
```text
[cells/demos/std-smoke] ──► build with target/sysroot-cellos
                                 │
           ┌─────────────────────┴─────────────────────┐
           ▼                                           ▼
   RISC-V 64 QEMU                              AArch64 QEMU
   (≥30 reps, warmups)                         (≥30 reps, warmups)
           │                                           │
           └─────────────────────┬─────────────────────┘
                                 ▼
               [validate-rust-std-promotion.py] ──► PASS / PROMOTABLE
```

## Assumptions
- **Claim:** `scripts/validate-rust-std-promotion.py` exists and is already tested against promotion schemas.
  **Confidence:** high
  **How to verify:** Ran unit tests in `tests/rust-std-promotion` passing 33/33 tests.

## Related Files
- Create: `cells/demos/std-smoke/Cargo.toml`
- Create: `cells/demos/std-smoke/src/main.rs`
- Create: `scripts/run-std-parity-benchmark.sh`
- Output: `evidence/benchmark-std-parity-rv64.json`
- Output: `evidence/benchmark-std-parity-arm64.json`

## Implementation Steps
1. Author `cells/demos/std-smoke`:
   - Exercising `String`, `Vec`, `Box`, `Instant::now()`, `println!`, and `thread::yield_now()`.
2. Author identical operations in `cells/demos/nostd-smoke` for bracketing baseline.
3. Write `scripts/run-std-parity-benchmark.sh`:
   - Automate QEMU launch, serial trace capture, repetition cycles, and timing metrics collection.
4. Execute benchmark runs on RISC-V QEMU and AArch64 QEMU.
5. Feed captured benchmark report into `scripts/validate-rust-std-promotion.py`.

## Success Criteria
- [ ] `std-smoke` cell boots, executes all standard library exercises, and prints expected log lines to console.
- [ ] `validate-rust-std-promotion.py` validates the live benchmark report without schema or noise rejections.
- [ ] Measured p99 regression between `std` and `no_std` is $\le 5\%$.

## Security Considerations
Benchmark cells must execute as unprivileged Tier 1 cells under standard capability filters.

## Risk Notes
QEMU host CPU contention can introduce timing jitter; runs must use locked CPU pinning or discard runs exceeding the 2% noise threshold.

## Deviation Log
None.
