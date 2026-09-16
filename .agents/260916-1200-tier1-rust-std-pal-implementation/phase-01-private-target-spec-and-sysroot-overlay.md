---
phase: 1
title: "Private Target Spec & Sysroot Overlay Pipeline"
status: completed
priority: P1
effort: "1d"
dependencies: []
tier: thinking
---

# Phase 01: Private Target Spec & Sysroot Overlay Pipeline

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview
Establishes the compiler target specifications and content-addressed source-overlay pipeline for CellOS Tier 1 targets (`riscv64gc-unknown-cellos`, `aarch64-unknown-cellos`, `x86_64-unknown-cellos`) using Compiler Strategy B.

## Requirements
- Functional:
  - Generate custom target JSON specifications under `targets/` defining `target_os = "cellos"`, bare-metal static relocation, and no-redzone.
  - Create the content-addressed source-overlay patch `patches/rust-std-cellos.patch` targeting `library/std/src/sys/pal/mod.rs` to register `mod cellos;`.
  - Create `scripts/build-cellos-sysroot.sh` to stage an ephemeral source tree from installed `rust-src`, apply the patch with exact context, and compile `libstd` via `cargo -Z build-std`.
- Non-functional:
  - Patch must refuse any base commit other than `f53b654a8` (nightly `2026-05-01`).
  - No source tree vendoring in git; output builds into ephemeral or cached sysroot directory.

## Architecture
```text
rust-src (f53b654a8) ─► [SHA-256 Check] ─► Apply patches/rust-std-cellos.patch
                             │
targets/*-unknown-cellos.json ┴─► cargo -Z build-std ─► target/sysroot-cellos/
```

## Assumptions
- **Claim:** Installed toolchain `nightly-2026-05-01` contains `rust-src` matching commit `f53b654a8`.
  **Confidence:** high
  **How to verify:** `rustc --version --verbose` confirms commit hash `f53b654a8`.
- **Claim:** `cargo -Z build-std` compiles standard library without requiring external C libraries.
  **Confidence:** high
  **How to verify:** Build dry run with minimal bare-metal stub.

## Related Files
- Create: `targets/riscv64gc-unknown-cellos.json`
- Create: `targets/aarch64-unknown-cellos.json`
- Create: `targets/x86_64-unknown-cellos.json`
- Create: `patches/rust-std-cellos.patch`
- Create: `scripts/build-cellos-sysroot.sh`

## Implementation Steps
1. Author custom target specifications in `targets/*.json`:
   - Set `"os": "cellos"`, `"env": ""`, `"vendor": "unknown"`, `"relocation-model": "static"`, `"panic-strategy": "abort"`.
   - Configure arch-specific flags: `no-redzone`, BTI/PAC for aarch64, medany for riscv64.
2. Create initial source overlay patch `patches/rust-std-cellos.patch`:
   - Add `target_os = "cellos" => { mod cellos; pub use self::cellos::*; }` to `library/std/src/sys/pal/mod.rs`.
   - Scaffold `library/std/src/sys/pal/cellos/mod.rs`.
3. Implement `scripts/build-cellos-sysroot.sh`:
   - Read `rustc --print sysroot` to locate `rust-src`.
   - Verify SHA-256 of base `mod.rs`.
   - Apply patch to an ephemeral staging directory.
   - Run `cargo build -Z build-std=core,alloc,std --target targets/riscv64gc-unknown-cellos.json`.

## Success Criteria
- [x] `targets/riscv64gc-unknown-cellos.json` and sibling arch targets are validated by `rustc`.
- [x] `scripts/build-cellos-sysroot.sh` executes and outputs compiled `libstd.rlib` for `riscv64gc-unknown-cellos`.
- [x] Base hash verification rejects modified or drifting `rust-src`.

## Security Considerations
Deterministic build prevents toolchain poisoning; `target_os="cellos"` prevents unintentional inheritance of host Linux/Unix assumptions.

## Risk Notes
`cargo -Z build-std` requires network access if dependencies are missing; mitigate by using pinned offline crates from `rust-src`.

## Deviation Log
None.
