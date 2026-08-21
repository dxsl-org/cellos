# Phase 06 — Tier 1 rust-std PAL C4

## Context Links
`.agents/TODO.md:21-27`; `docs/roadmap/product-stages.md:40-44`; `docs/specs/18-cell-trust-tiers.md:55-60`; Phase 03.

## Status

**Pending — dependency-blocked on umbrella Phase 03.** The child feasibility sub-slice at [`../260821-1800-tier1-rust-std-pal-feasibility/plan.md`](../260821-1800-tier1-rust-std-pal-feasibility/plan.md) reached **FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED**. This verifies the inventory, contracts, compiler strategy, and synthetic-fixture validator only; it does not complete this phase or establish PAL, target, runtime, live-capture, readiness, promotion, or publication.

## Overview
Design G4 `rust-std` as pure-Rust PAL/custom target, not a new tier.

## Key Insights
Compatibility includes intentional Unsupported behavior; mlibc is excluded.

## Requirements
Define target, PAL, panic/unwind, TLS, allocator/startup and API support. Promotion workload suite compares identical syscall/IPC operations with pinned `rust-no-std` and `rust-std` binaries on the same architecture, board/QEMU version, firmware, CPU count/frequency policy, build profile, toolchain, and service topology. After ≥5 warmups, run ≥30 independent repetitions; retain raw samples, p50/p95/p99 and noise metadata, reject runs with environmental interference or >2% baseline p99 drift between bracketing controls. Rust-std p99 syscall/IPC regression must be ≤5% for every promoted architecture/environment cell.

## Architecture
Rust `std` → Cellos PAL → `ostd`/service clients → Tier 1 SAS admission.

## Assumptions
Paths `[UNVERIFIED] libs/std-pal/` and `[UNVERIFIED] targets/*-unknown-cellos.json`.

## Related Code Files
`libs/ostd/src/lib.rs:48`; `libs/ostd/src/app.rs:136`; `libs/ostd/src/clients.rs:28-29`; `rust-toolchain.toml`; `libs/api/src/abi.rs:2-12`.

## Implementation Steps
Inventory hooks; classify support; define identical syscall/IPC workloads; pin environments; capture bracketing no-std controls and ≥30 repetitions; apply noise rejection and ≤5% p99 gate; define remaining contracts; separately approve implementation.

## Todo List
- [ ] PAL/Unsupported map approved — the 27/27-module, 36-hook map is package-verified, but both applicable human approval rows remain `NOT GRANTED`.
- [ ] Target feasible for implementation — the private in-tree PAL/source-overlay strategy is package-verified and conditionally recommended, but security backing, all named human approvals, and the implementation checkpoint remain blocked.
- [ ] Compatibility matrix accepted — the fixture-only contract is package-verified and non-promotional; human benchmark approval remains `NOT GRANTED`, with no authenticated live capture.

## Success Criteria
100% scoped hooks classified; every promoted architecture/environment cell has valid raw samples and ≤5% rust-std p99 syscall/IPC regression versus pinned no-std; noisy/missing cells fail promotion; no mlibc or ABI regression.

## Risk Assessment
Toolchain maintenance/POSIX creep. Keep profile experimental; published triples create compatibility obligations.

## Security Considerations
Entropy fails closed; capabilities remain; `std` grants no authority.

The verified child recorded 33/33 feasibility tests, 57/57 validator adversarial attacks, 36/36 security-manifest tamper attacks, host 105 passed / 0 failed / 4 ignored, 27/27 modules, 36 hooks at 8 Supported / 10 Unsupported / 18 Deferred, 46 pinned Rust sources, the exact six-path kernel security-backing inventory, and 101 approval inputs with matching digests and links. Final quality and security reviews both returned PASS with no findings. These reviews do not grant human approval or resolve the current `dev-weak-rng`/zero RNG and unvalidated `GetRandom` pointer backing.

## Next Steps
Retain this phase as pending. Create no G4 implementation child until `PAL-019` has a production tuple without `dev-weak-rng` and real entropy-or-zero/error evidence, `PAL-031` has bounded caller-owned writable validation plus hostile null/overflow/oversized/unmapped/kernel/peer direct-syscall evidence, all six named human approval rows are granted, `PAL-IMPLEMENTATION-CHECKPOINT` is unblocked, and umbrella Phase 03 production gates are approved.

## Deviation Log
None.
