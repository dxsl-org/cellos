---
phase: 8
title: "CPU inference (NEON) and Haily measurement"
status: pending
priority: P2
effort: "8d"
dependencies: ["3 or 7"]
tier: thinking
checkpoint: false
---

# Phase 08: NEON inference and Haily measurement (AS-8)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `libs/tensor-math`, `libs/ai-engine`, `cells/services/ai` (no aarch64 SIMD today); [Haily roadmap](../../docs/roadmap/haily-personal-assistant.md) H1/H3 comparability rules

## Overview

Make the native inference path useful on Apple CPUs (NEON dot products for `Q8_0`/`F16`/`F32`, runtime feature detection for `dotprod`, `fp16`, `i8mm`), then measure against Asahi Linux and macOS (CPU-only) on the same Mac.

## Key Insights

- The reason people run agents on Macs is unified memory and GPU/ANE under macOS. Cellos only gets the CPU part. The comparison must say that plainly.
- NEON kernels also help QEMU `virt` and RPi; they are host-testable against the scalar path.
- Haily's port is deferred until its first version is complete. Until then measure the Cellos `ai` service with the repository models; Haily workloads come with Haily H1.

## Requirements

- NEON kernels with bit-exact or tolerance-bounded equality to the scalar path (host tests).
- Feature detection from `ID_AA64ISAR0/1_EL1`; scalar fallback.
- Measurement: tokens/s, time to first token, resident memory, for the same model and prompt set on Cellos (S1-hvf or S2), Asahi Linux, and macOS CPU-only.

## Related Code Files

- Modify: `libs/tensor-math/**`, `libs/ai-engine/**`
- Create: `scripts/bench/apple-inference.md` (procedure), evidence under this plan

## Implementation Steps

1. Scalar baseline numbers on QEMU and the target.
2. NEON kernels + equality tests.
3. Measurements on the three systems; report unfavourable results unchanged.

## Todo List

- [ ] Baseline
- [ ] NEON kernels + tests
- [ ] Comparison report

## Success Criteria

- [ ] Host tests prove equality; a report records all three systems with exact model, prompt set, versions and hardware.

## Risk Assessment

- Measuring in the hosted track includes hypervisor overhead; label it.

## Security Considerations

None beyond the existing `ai` service authority.

## Next Steps

Haily H1 on the Apple lane once Haily's first version is accepted.

## Deviation Log

None.
