---
title: "Tier 1 rust-std PAL Feasibility and Decision Plan"
description: "Hook, compiler, runtime-contract, workload-parity, and fixture-only benchmark-validator plan for umbrella Phase 06."
status: "FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED"
priority: P1
effort: 5d
branch: main
tags: [phase-06, tier-1, rust-std, pal, compiler, benchmark, feasibility]
blockedBy: []
blocks: []
created: 2026-08-21
---

# Tier 1 rust-std PAL Feasibility and Decision Plan

## Overview

This child executes only the approved non-blocked portion of [umbrella Phase 06](../260821-0642-app-tiers-completion/phase-06-tier1-rust-std-pal.md). It inventories the pinned `rust-std` PAL boundary, decides whether a custom-target/internal-PAL path is maintainable, freezes runtime/API and identical-workload contracts, and implements a fixture-only promotion validator with behavioral tests. It does not implement a PAL/target/runtime, publish a target or triple, capture or claim promotion evidence, add fake `std`, or introduce mlibc.

The terminal child state is **FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED**. The recommendation remains **CONDITIONAL GO** only after every Deferred blocker is implemented and evidenced, including `PAL-019` production entropy without `dev-weak-rng` and `PAL-031` bounded caller-owned writable validation with hostile direct-syscall evidence. Verification does not promote either hook, grant any of the six human approvals, pass the implementation checkpoint, change umbrella Phase 06 from pending, satisfy its Phase 03 dependency, or claim PAL/target/runtime readiness.

## Phases

| Phase | Name | Status | Depends on |
|---|---|---|---|
| 01 | [Pinned Source and Hook Inventory](phase-01-pinned-source-and-hook-inventory.md) | verified; security-backing blockers preserved | — |
| 02 | [Compiler Integration Strategy](phase-02-compiler-integration-strategy.md) | verified; conditional recommendation only; human approval NOT GRANTED | 01 |
| 03 | [PAL Runtime and API Contract](phase-03-pal-runtime-and-api-contract.md) | verified; `PAL-019`/`PAL-031` Deferred; human approval NOT GRANTED | 01 |
| 04 | [Workload Parity and Benchmark Validator Implementation](phase-04-workload-parity-and-benchmark-validator-contract.md) | verified; fixture-only and non-promotional; human approval NOT GRANTED | 01 |
| 05 | [Decision Package and Approval Checkpoint](phase-05-decision-package-and-approval-checkpoint.md) | package verified; security backing and human approval BLOCKED | 02, 03, 04 |

## Stage Graph

```text
01 Pinned source + complete hook map
 ├─> 02 Compiler strategy ─────────────────────────┐
 ├─> 03 Runtime/API contract ──────────────────────┼─> 05 Decision package
 └─> 04 Fixture-only validator + tests ────────────┘       => PAL implementation + promotion BLOCKED
```

## Exact Feasibility Artifacts

Execution creates feasibility artifacts under `artifacts/` and approvals under `approvals/`. Phase 04 additionally creates `scripts/rust_std_promotion/{__init__.py,validator.py,benchmark-run.schema.json}`, `scripts/validate-rust-std-promotion.py`, and the exact fixture/test files named there. The validator consumes synthetic fixtures only and emits deterministic non-promotional reports.

## Verification and Review Evidence

Final verification passed 33/33 feasibility tests, 57/57 validator adversarial attacks, 36/36 security-manifest tamper attacks, and the host aggregate of 105 passed, 0 failed, and 4 ignored. Reconciliation confirmed 27/27 pinned `std::sys` modules; 36 hooks with 8 Supported, 10 Unsupported, and 18 Deferred; 46 pinned Rust sources; the exact six-path kernel security-backing inventory; and all 106 canonical approval inputs, including the governed GetRandom hostile-evidence report, runner, and fixture sources. Every manifest digest and artifact link matched. Final independent quality review returned PASS with no findings, and final independent security review returned PASS with no findings. These are package-verification results, not human approvals, authenticated live evidence, implementation authorization, or promotion evidence.

## Dependencies and Non-Waivable Boundary

- Evidence inputs are pinned `rust-toolchain.toml`, every module declared by installed `library/std/src/sys/mod.rs:3-30`, the transitive pinned `std` sources in the support map, every cited Cellos `libs/api`/`libs/ostd` backing source, the exact closed kernel security-backing inventory, the governed GetRandom hostile-evidence report/runner/fixture sources, and the benchmark/validator sources named by the phase files.
- The feasibility package and fixture-only validator are verified while security backing, all six named human approvals, the implementation checkpoint, and umbrella Phase 03 production gates remain blocked. PAL/target/runtime implementation, live capture, a published target/triple, and promotion remain prohibited.
- A later PAL/target/runtime implementation child may be authorized only after `PAL-019` is implemented/evidenced, every remaining Deferred prerequisite is satisfied, Phase 05 records the compiler-integration choice and all contract approvals, **and** umbrella Phase 03's production gates are approved. Steering cannot waive any condition.
