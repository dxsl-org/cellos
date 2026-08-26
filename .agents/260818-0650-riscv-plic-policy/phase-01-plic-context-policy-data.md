---
phase: 1
title: "Add PLIC Context Policy Data"
status: completed
priority: P2
effort: "2h"
dependencies: []
tier: medium
---

# Phase 1: Add PLIC Context Policy Data

> **Required - deviation-log:** Log every Decision / Deviation / Surprise in Section Deviation Log the moment it occurs.

## Overview

Add the smallest data-only PLIC S-mode context policy to `hal/soc/riscv` so RV64 callers stop baking `context 1` into shared mechanism. Do not add board wiring, IRQ numbers, MMIO maps, or driver code to this crate.

## Requirements

- Functional: Add a `PlicContextPolicy` or equivalent immutable type that maps logical hart to S-mode PLIC context for current SiFive/T-Head layouts.
- Functional: Add the policy to `RiscvSocProfile`, set it for `GENERIC_VIRT`, `JH7110`, and `SG2042`, and expose it from `hal-soc-riscv`.
- Non-functional: Keep `hal/soc/riscv` `#![no_std]` and data-only as documented in `hal/soc/riscv/src/lib.rs:1` and `hal/soc/riscv/src/lib.rs:5`.
- Non-functional: Keep files below the project target where practical; current `hal/soc/riscv/src/*.rs` files are 20-37 lines and can absorb a small type or a new focused module.

## Architecture

Data flow: SoC profile constants enter `hal/soc/riscv` at build time, `kernel/src/platform.rs` selects the active profile, and later phases consume `profile.plic_context` to compute the boot hart S-mode context. Existing profile selection is feature-based at `kernel/src/platform.rs:174` and already picks Pioneer before VF2 at `kernel/src/platform.rs:175`.

## Related Files

- Modify: `hal/soc/riscv/src/profile.rs`
- Modify: `hal/soc/riscv/src/catalog.rs`
- Modify: `hal/soc/riscv/src/lib.rs`
- Modify: `hal/soc/riscv/src/tests.rs`
- Optional create: `hal/soc/riscv/src/plic_policy.rs`

## Implementation Steps

1. Define a data-only context policy with a checked `s_mode_context_for_logical_hart(logical_hart: usize) -> Option<usize>` or saturating equivalent that cannot overflow silently.
2. Add the field to `RiscvSocProfile`; current fields are compatible lists and access policies at `hal/soc/riscv/src/profile.rs:9`.
3. Set current profiles to the existing SiFive-style S-mode context sequence: logical hart 0 maps to context 1 and logical hart 1 maps to context 3, matching the current comments in `hal/arch/riscv/src/common/plic.rs:12`.
4. Add tests for `0 -> 1`, `1 -> 3`, Pioneer profile selection data, and SG2042 retaining `VirtioMmioPolicy::Absent` from `hal/soc/riscv/src/catalog.rs:28`.
5. Run `cargo test -p hal-soc-riscv --target x86_64-unknown-linux-gnu`.

## Success Criteria

- [x] `hal-soc-riscv` tests prove context mapping and existing access policies.
- [x] No board descriptor or shared driver file is modified in this phase.
- [x] No `libs/api/` or `libs/types/` file is modified.

## Evidence

- Final QA report `qa-2026-08-18-final.md` records `cargo test -p hal-soc-riscv --target x86_64-unknown-linux-gnu` with `3 passed` and `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu` with `8 passed`.
- The same report closes the slice with all 11 final gates passed against `HEAD c6a31372`, including QEMU boot `PASS: FAT16 mounted — kernel booted (no disk)`.

## Security Considerations

Fail closed on invalid hart mapping; wrong context can starve or misroute external interrupts. Do not accept DTB-provided context arithmetic in this phase.

## Risk Notes

- Risk: medium likelihood x high impact, wrong context formula breaks external IRQs on all RV64 boards. Mitigation: encode current known layout as explicit data and test hart 0/1.
- Risk: JH7110 secondary-hart boot can still select physical hart0 before any S-mode PLIC context exists; current policy fails closed and makes no secondary external IRQ claim.
- Rollback: revert the `hal/soc/riscv` field/type/tests; no runtime behavior changes exist until Phase 2. Irreversible part: none.

## Assumptions

- Claim: Current QEMU/JH7110/SG2042 PLIC S-mode context layout is the SiFive-style odd sequence.
  Confidence: medium
  How to verify: inspect board manuals/DTB interrupt-controller context docs before hardware enablement; current code only documents QEMU-style layout at `hal/arch/riscv/src/common/plic.rs:12`.

## Deviation Log

None.
