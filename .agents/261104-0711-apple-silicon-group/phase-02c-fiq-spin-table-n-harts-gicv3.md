---
phase: 2c
title: "FIQ path, spin-table secondaries, N harts, GICv3"
status: pending
priority: P2
effort: "8d"
dependencies: ["2a"]
tier: thinking
checkpoint: false
---

# Phase 02c: FIQ, spin-table, N harts, GICv3 (AS-2)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `hal/arch/arm/src/aarch64/trap.rs:371`, `el2.rs:291,322-326` (FIQ → sync today); `backend/mod.rs:113-118` (`SecondaryStart`); `kernel/src/task/smp.rs:16` (`MAX_HARTS = 2`)
- x86 breadth [phase 07a](../261103-2218-x86-platform-breadth/phase-07a-percpu-generalization.md) (per-CPU N harts, Intel-owned)

## Overview

1. A real FIQ vector path dispatched through the backend's interrupt controller (Apple delivers the timer, IPIs and PMU as FIQ).
2. `SecondaryStart::SpinTable` from the FDT `cpu-release-addr` (Apple has no PSCI).
3. aarch64 `MAX_HARTS` above 2 with P/E cluster topology recorded.
4. A GICv3 controller (needed by the hosted track and group 2).

## Key Insights

- QEMU `virt` with `gic-version=3` can route a Group 0 interrupt as FIQ. That gives an S1-qemu test of the FIQ path without Apple hardware.
- Spin-table: write the entry address to `cpu-release-addr`, clean the cache line, `sev`. QEMU `virt` uses PSCI, so spin-table is host-tested here and proven in phase 04.
- N harts depends on x86 07a's per-CPU generalization. Apple extends it to aarch64 after it lands and does not take its files.

## Requirements

- FIQ vectors (current EL and lower EL, including the vCPU path) call `Board::handle_fiq()`; a backend without FIQ sources logs and panics on an unexpected FIQ instead of decoding a stale ESR.
- `SpinTable` start with a bounded online wait and a logged per-core outcome.
- aarch64 `MAX_HARTS` raised (target 32: covers M1 Ultra's 20 cores); MPIDR → hart mapping with cluster IDs.
- `gicv3` controller behind `ArmIrqController`: distributor, redistributors, system-register CPU interface, SGIs.

## Related Code Files

- Modify: `hal/arch/arm/src/aarch64/{trap,el2,vcpu,boot}.rs`, `backend/{mod,qemu_virt}.rs`, `kernel/src/task/smp.rs` (after x86 07a), `kernel/src/task/hart_local.rs`
- Create: `hal/arch/arm/src/aarch64/gicv3.rs`

## Implementation Steps

1. FIQ dispatch + GICv3 Group-0 timer FIQ test on `virt`.
2. GICv3 controller; `virt` lane with `gic-version=3`, `-smp 4`.
3. SpinTable start with host tests (FDT parsing, release protocol against a fake memory).
4. N harts on aarch64 after x86 07a lands.

## Todo List

- [ ] FIQ dispatch
- [ ] GICv3
- [ ] SpinTable
- [ ] N harts (after x86 07a)

## Success Criteria

- [ ] `virt` GICv3 lane: timer tick through FIQ, shell, `-smp 4` all harts online, cross-hart TLB flush.
- [ ] GICv2 `virt` and `raspi3b` lanes unchanged.

## Risk Assessment

- Raising `MAX_HARTS` touches scheduler and TLB-shootdown arrays shared with x86. Wait for 07a, which owns that change.

## Security Considerations

An unexpected FIQ must never be treated as a syscall or a data abort (today it reaches the sync decoder).

## Next Steps

Phase 03 (GICv3) and phase 04 (FIQ, spin-table).

## Deviation Log

None.
