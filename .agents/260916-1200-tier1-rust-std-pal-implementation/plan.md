---
title: "Tier 1 Rust std PAL In-Tree Implementation Plan"
description: "Implementation of custom target specifications, sysroot source-overlay patch, in-tree CellOS PAL hooks, and parity benchmarks for Tier 1 Real-time SAS."
status: pending
priority: P1
effort: 4d
branch: main
tags: [tier-1, rust-std, pal, compiler, sysroot, qemu]
blockedBy: []
blocks: []
created: 2026-09-16
---

# Tier 1 Rust `std` PAL In-Tree Implementation Plan

## Overview
This plan executes the implementation of the in-tree Rust `std` Platform Abstraction Layer (PAL) for CellOS Tier 1 Real-time SAS, following the successful unblocking of `PAL-IMPLEMENTATION-CHECKPOINT`. It adopts Compiler Strategy B (content-addressed source overlay against pinned `nightly-2026-05-01` / `f53b654a8` checkout) and enforces the runtime contracts defined in `CELLOS-RUST-STD-RUNTIME-v1`.

## Phases

| Phase | Title | Status | Dependencies | Tier |
|---|---|---|---|---|
| 01 | [Private Target Spec & Sysroot Overlay Pipeline](./phase-01-private-target-spec-and-sysroot-overlay.md) | completed | — | thinking |
| 02 | [Core PAL Primitives (Init, Alloc, Yield, Time)](./phase-02-core-pal-primitives.md) | completed | 01 | medium |
| 03 | [Security Primitives (PAL-019 Random & StdIO)](./phase-03-security-primitives-random-and-stdio.md) | completed | 01, 02 | thinking |
| 04 | [Unsupported Families & Boundary Shims](./phase-04-unsupported-families-and-boundary-shims.md) | pending | 01, 02 | medium |
| 05 | [Workload Parity, Benchmarking & QEMU Validation](./phase-05-workload-parity-and-validation.md) | pending | 01, 02, 03, 04 | medium |

## Key Decisions & Architecture Invariants
1. **Source Overlay (Strategy B)**: Ephemeral, content-addressed patch over `rust-src` without vendoring full compiler sources in-tree.
2. **Deterministic Sysroot**: Built via `cargo -Z build-std=core,alloc,std --target targets/<arch>-unknown-cellos.json`.
3. **Strict Abort & Single Task**: `panic=abort`, `available_parallelism = 1`, no background OS thread creation.
4. **Security Backing Gating**:
   - `PAL-019`: Governed production tuple omits `dev-weak-rng`; zero/error fail-closed.
   - `PAL-031`: Bounded caller-owned writable pointer validation prior to read/write.
5. **No Ambient Authority**: Filesystem, networking, and process creation return explicit `io::ErrorKind::Unsupported`.

## Cook Handoff
```bash
$hc-cook .agents/260916-1200-tier1-rust-std-pal-implementation/plan.md
```
