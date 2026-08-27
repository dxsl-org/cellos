---
phase: 1
title: "Pinned Source and Hook Inventory"
status: "FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED"
priority: P1
effort: 1d
dependencies: []
tier: thinking
---

# Phase 01: Pinned Source and Hook Inventory

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs. Choose the smallest reversible documentation change; escalate any scope or contract expansion.

## Overview

Freeze source provenance and classify every PAL hook selected by the pinned standard library as Supported, Unsupported, or Deferred. This phase produces an auditable map, not PAL code.

## Requirements

- Functional: enumerate every module declaration, private and public, at pinned `library/std/src/sys/mod.rs:3-30`, map each declaration to one or more hook rows, then classify 100% of the scoped hooks as Supported, Unsupported, or Deferred.
- Non-functional: bind the inventory to nightly `2026-05-01`, rustc `1.97.0-nightly` commit `f53b654a8`, a digest for every declared module source, the transitive source manifest, the cited Cellos ABI/runtime backing source, and an exact-set kernel security-backing inventory with per-file roles/digests.
- Scope: include `configure_builtins`, `personality`, `cmath`, `env_consts`, `platform_version`, initialization/cleanup, abort/panic, allocator, TLS, startup, I/O, filesystem, networking, time, entropy, environment, process, thread, synchronization, and every other declared pinned sys module.

## Architecture

Pinned `rust-src` hook → support-map row → Cellos backing contract or explicit unsupported/deferred rationale. The current selector falls through to `unsupported` for an unknown `target_os="cellos"`; an external `libs/std-pal` is not a selector input.

### Support-map schema

`artifacts/pal-hook-support-map.json` has:

- `schema_version`; `toolchain {channel, rustc_version, commit_hash, rust_src_root, source_digest}`.
- `scope.sys_module_manifest[]`, with exact declaration line, visibility, module, selected source path/digest, mapped hook IDs, and declaration evidence for all 27 module declarations at lines 3–30; zero omissions is mandatory.
- `hooks[]`, each with `hook_id`, `upstream_path`, `symbol`, `cfg_gate`, `required_by`, `classification` (`Supported|Unsupported|Deferred`), `cellos_backend`, `contract`, `error_semantics`, `capability_effect`, `safety_invariants`, `dependencies`, `evidence`, `owner`, and `rationale`.
- `kernel_security_backing_inventory`, closed over the exact required paths for kernel feature defaults, syscall ABI decode/allowlist/dispatch, `GetRandom`, pointer-validation primitives, VirtIO RNG wiring/source, and the typed caller wrapper, with roles, per-file SHA-256, exact `required_paths`, and a canonical inventory digest.
- `summary {total, supported, unsupported, deferred}` with counts exactly matching `hooks`.
`Unsupported` must name the stable observable error/abort behavior. `Deferred` must name an owner, prerequisite, and why deferral cannot silently fall back. A Deferred hook needed by minimal startup, panic/abort, allocation, syscall, IPC, entropy, or writable pointer provenance rejects implementation readiness.

## Assumptions

- **Claim:** The installed `rust-src` component is a faithful library-source snapshot for commit `f53b654a8`, while full matching compiler source may need separate retrieval.
  **Confidence:** medium
  **How to verify:** Record the component manifest and source digest, then compare them with the matching Rust repository commit before accepting the map.

## Related Files

- Read only: `rust-toolchain.toml`
- Read only: `/home/dmin/.rustup/toolchains/nightly-2026-05-01-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/std/src/sys/pal/mod.rs`
- Read only: `/home/dmin/.rustup/toolchains/nightly-2026-05-01-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/std/src/sys/pal/unsupported/mod.rs`
- Read only: `/home/dmin/.rustup/toolchains/nightly-2026-05-01-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/std/src/sys/pal/unsupported/common.rs`
- Read only: matching pinned `library/std/src/sys/**` sources discovered from that selector
- Read only: `libs/api/src/abi.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/lib.rs`, `libs/ostd/src/app.rs`, `libs/ostd/src/clients.rs`, `libs/ostd/src/startup.rs`, `libs/ostd/src/entry.rs`, `libs/ostd/src/heap.rs`, `libs/ostd/src/args.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/sync.rs`
- Read only: `kernel/Cargo.toml`, `kernel/src/task/syscall.rs`, `kernel/src/task/drivers.rs`, `kernel/src/task/drivers/virtio_rng.rs`
- Create during feasibility execution: `artifacts/pal-hook-support-map.json`

## Implementation Steps

1. Record the pinned channel, rustc version/commit, installed source root, all 27 `sys/mod.rs:3-30` declarations, every selected module-source digest, and a reproducible transitive source manifest; reject mixed-version evidence.
2. Map every private and public module declaration to one or more stable hook IDs, then walk the selected `std::sys` and `std::sys::pal` interfaces outward while preserving exact upstream paths/symbols/cfg gates.
3. Trace candidate Cellos backing only through the frozen ABI and `ostd`; separately close the kernel security-backing exact path set and record roles/digests for feature defaults, syscall decode/allowlist/dispatch, `GetRandom`, user-buffer validation, VirtIO RNG, and the typed caller wrapper. Do not invent POSIX, ambient authority, or external plug-in hooks.
4. Classify each hook once; specify observable Unsupported behavior and prerequisites for Deferred rows. `PAL-019` stays Deferred while the default tuple enables `dev-weak-rng` over a zero-byte RNG stub. `PAL-031` technical backing/evidence is complete but stays Deferred pending named approval of this governed manifest rebind. Target-sensitive compiler builtins, personality, external math symbols, target constants, and thread query/yield semantics also remain blocking Deferred until implemented and proved.
5. Reconcile module and hook totals, require exact equality between the security inventory entries and `required_paths`, and record zero omitted modules, unclassified/duplicate hooks, or evidence-free rows.

## Success Criteria

- [x] Toolchain identity, 27/27 declared module scope, per-module source digests, transitive source digest, and the exact six-path kernel security-backing inventory/digest are complete and internally consistent.
- [x] Every declared module maps to at least one hook and every scoped hook has exactly one `Supported|Unsupported|Deferred` row with evidence.
- [x] All Unsupported and Deferred rows have explicit observable semantics; no default unsupported PAL, development entropy fallback, or unvalidated pointer boundary is mislabeled Supported.
- [x] No row proposes code, mlibc, a published target/triple, fake `std`, or promotion evidence.

## Verification Evidence

Final reconciliation verified all 27/27 pinned `std::sys` module declarations, all 36 hooks at 8 Supported / 10 Unsupported / 18 Deferred, all 46 pinned Rust sources, and exact equality for the six-path kernel security-backing inventory. The 106-input approval manifest, per-input digests, and artifact links matched. `PAL-019` remains Deferred for its recorded entropy blocker; `PAL-031` technical backing/evidence is complete but remains Deferred pending named approval of this governed manifest rebind. This verified inventory grants no human approval or implementation authorization.

## Security Considerations

A Supported classification may not grant capabilities absent from the calling Cell. Entropy, path, network, process, and environment hooks default to explicit failure until a capability-preserving backend is proved. `PAL-019` requires a production tuple without `dev-weak-rng` and real entropy-or-zero/error evidence. `PAL-031` has bounded caller-owned writable validation and hostile null/overflow/oversized/unmapped/kernel/peer direct-syscall evidence, but remains Deferred until named approval of the governed rebind. Unsafe invariants and pointer ownership must be stated per relevant hook.

## Risk Notes

The upstream hook set is an internal, unstable interface. Missing indirect exports would invalidate the map; mixed toolchain/source provenance invalidates the whole phase.

## Deviation Log

None.
