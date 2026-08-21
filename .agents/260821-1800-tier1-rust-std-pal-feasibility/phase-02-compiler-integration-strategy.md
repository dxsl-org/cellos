---
phase: 2
title: "Compiler Integration Strategy"
status: "FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED"
priority: P1
effort: 1d
dependencies: [1]
tier: thinking
---

# Phase 02: Compiler Integration Strategy

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs. Escalate any irreversible compatibility commitment or expansion beyond feasibility.

## Overview

Evaluate how the pinned compiler can select a real internal Cellos PAL for a private custom target, then select or reject a strategy with explicit maintenance and exit criteria. No compiler or standard-library source is changed here.

## Requirements

- Functional: evaluate all alternatives below against the Phase 01 map and record one `SELECTED`, or record `NO-GO` if none passes.
- Non-functional: reproducible from commit `f53b654a8`; private/experimental; no published triple; no mlibc/POSIX dependency; no external PAL plug-in claim.
- The selected path must make `target_os="cellos"` select an internal `library/std/src/sys/pal/cellos` implementation rather than the current `_ => unsupported` branch.

## Architecture

| Alternative | Feasibility shape | Required proof | Rejection criteria |
|---|---|---|---|
| A. Pinned Rust source fork | Patch matching compiler target metadata plus in-tree `library/std` PAL; build a private sysroot | exact source/patch digests, repeatable custom-target build path, selector proof | fork cannot remain commit-pinned; patch touches unrelated targets; hidden ABI/POSIX dependency |
| B. Reproducible source-overlay patch | Apply a content-addressed patch to matching Rust sources during private toolchain construction | base-hash guard, deterministic patch/result, same selector and sysroot proof as A | patch can apply fuzzily/to wrong source; generated sysroot provenance is incomplete |
| C. Upstream target/PAL contribution | Carry the same internal integration through upstream review, while keeping Cellos private until separately approved | accepted upstream target/PAL design and maintenance owner | requires publishing a triple in this slice, waits indefinitely, or weakens Cellos capability semantics |
| D. External `libs/std-pal` plug-in | Link an out-of-tree crate without editing `library/std` selector | none under the pinned architecture | reject: the selector imports only internal PAL modules |
| E. Unsupported-PAL/fake-std shim | Retain fallback and provide look-alike symbols/crate | none | reject: not real `std`, silent unsupported behavior, false compatibility claim |

A and B may be compared as implementation mechanics for the same internal integration; C is a later upstreaming route, not permission to publish now. D and E are mandatory rejections unless pinned compiler architecture materially changes and is re-approved.

## Assumptions

- **Claim:** A private custom target can carry `target_os="cellos"` through the pinned compiler/sysroot build once the matching target metadata and internal PAL selector are patched.
  **Confidence:** medium
  **How to verify:** In the later feasibility execution, construct a throwaway pinned sysroot and capture cfg/selector evidence without committing product code.
- **Claim:** The Cellos linker/startup constraints can be expressed without adopting a published built-in target.
  **Confidence:** medium
  **How to verify:** Compare the private target specification with all three existing bare-metal target/linker configurations and record unsupported fields.

## Related Files

- Read only: `rust-toolchain.toml`, `.cargo/config.toml`, `.cargo/config.toml.example`
- Read only: `libs/cell-build/src/lib.rs`, `libs/ostd/src/startup.rs`, `libs/ostd/src/entry.rs`
- Read only: pinned Rust `library/std/src/sys/pal/mod.rs` and matching compiler `compiler/rustc_target/src/spec/**`
- Read only: `artifacts/pal-hook-support-map.json`
- Create during feasibility execution: `artifacts/compiler-strategy-decision.md`
- Create during feasibility execution: `approvals/compiler-integration.md`

## Implementation Steps

1. Freeze evaluation dimensions: selector correctness, target-spec ownership, sysroot reproducibility, patch maintenance, ABI stability, capability preservation, three-architecture implications, and upstream exit path.
2. For A–C, describe the exact compiler/library files that later implementation would own and the provenance chain from base commit to sysroot; do not edit them.
3. Record why D and E fail under the pinned architecture.
4. Apply universal rejection criteria: fallback remains unsupported; build requires mlibc/POSIX; frozen ABI changes lack 2× approval; provenance is incomplete; target/triple must be published; ambient authority appears; or a required Phase 01 hook is Deferred.
5. Write a single recommendation with rejected alternatives, maintenance owner, invalidation triggers, and an independent compiler/PAL reviewer decision.

## Success Criteria

- [x] Every alternative has evidence requirements, maintenance burden, exit path, and rejection criteria.
- [x] Exactly one strategy is `SELECTED`; the conditional recommendation cannot authorize implementation.
- [x] The decision explicitly rejects external `libs/std-pal` plug-in and fake/unsupported `std` for this pinned compiler.
- [x] Compiler/toolchain-owner and independent-PAL-reviewer approval states are recorded as `NOT GRANTED`; no target/triple is published.

## Verification Evidence

The selected exact, no-fuzz, content-addressed source-overlay strategy and all rejected alternatives were covered by the 33/33 passing feasibility suite. Canonical plan/artifact links and all approval-input digests matched, and final independent quality and security reviews both returned PASS with no findings. Both compiler-integration approval rows remain `NOT GRANTED`; the verified strategy remains conditional and authorizes no compiler, PAL, sysroot, target, or triple work.

## Security Considerations

Target cfgs and PAL routing are security boundaries: a wrong selector can expose host assumptions, ambient process APIs, or fail-open entropy. Patch/sysroot artifacts require content-addressed provenance and review distinct from the author.

## Risk Notes

Internal `std` PAL APIs are unstable and pin Cellos to compiler-source maintenance. Upstream acceptance timing cannot be a prerequisite for completing this feasibility slice.

## Deviation Log

None.
