---
phase: 3
title: "PAL Runtime and API Contract"
status: "FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED"
priority: P1
effort: 1d
dependencies: [1]
tier: thinking
---

# Phase 03: PAL Runtime and API Contract

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs. Escalate frozen-ABI, authority, or compatibility expansion.

## Overview

Freeze the minimum panic, TLS, allocator, startup, and public-API behavior a later real `std` integration must preserve. The result is a reviewable contract and support surface, not a runtime implementation.

## Requirements

- Functional: decide the observable contract for startup/termination, panic/unwind, allocation/OOM, TLS/threading, arguments/environment, time/entropy, I/O/filesystem/network/process APIs, synchronization, and error mapping.
- Non-functional: cleanly distinguish Supported, Unsupported, and Deferred; preserve Cellos Tier 1 SAS capabilities and frozen ABI; avoid POSIX creep; never treat an allowlist bit as proof of buffer provenance or entropy quality.
- Unsupported APIs must return the documented `std::io::ErrorKind::Unsupported`/equivalent or abort only where the contract requires divergence; no success-shaped stubs. Entropy is non-qualifying while the default tuple enables `dev-weak-rng`. `GetRandom` technical backing now includes bounded caller-owned writable validation and focused hostile direct-opcode evidence, but `PAL-031` remains Deferred pending named approval of this governed rebind.

## Architecture

`std` API → internal Cellos PAL hook → existing `ostd` syscall/service client → frozen `libs/api` ABI → kernel decoder/allowlist/dispatch/handler/validation/device backing. The contract must resolve:

1. **Startup/exit:** loader entry symbol, stack/ABI alignment, init-array ordering, argument ownership/encoding, `main` return/exit code, cleanup guarantees, and architecture-specific handoff.
2. **Panic/unwind:** abort-versus-unwind policy, panic logging disclosure, exit status, double-panic behavior, and whether backtraces are Unsupported or Deferred. Unwind may not be implied by `std` availability.
3. **Allocator:** per-cell ownership, alignment, allocation/deallocation/zero-size behavior, OOM divergence/status, single-task concurrency premise, and absence of cross-cell memory authority.
4. **TLS/threading:** language TLS model, destructor ordering, thread identity, synchronization primitives, and explicit behavior while Cellos cells remain single-task. Network TLS (`ostd::tls`) is unrelated and must not be confused with language TLS.
5. **API contract:** stable error translation and capability checks for filesystem, network, time, entropy, environment, process, and thread APIs; no ambient resources. Entropy requires a production tuple without `dev-weak-rng` and real entropy-or-zero/error behavior. Every writable raw buffer requires checked bounds and complete caller-owned writable provenance before access.

## Assumptions

- **Claim:** Existing `ostd` startup, allocator, argument, syscall, and service-client code is the intended semantic backing rather than a second runtime stack.
  **Confidence:** high
  **How to verify:** Obtain SDK/runtime-owner confirmation while reconciling each support-map row.
- **Claim:** Abort-only panic and explicitly Unsupported process/thread families can form a coherent initial `std` profile.
  **Confidence:** medium
  **How to verify:** Check pinned `std` dependencies for hooks that require unwind or thread functionality during minimal program startup.

## Related Files

- Read only: `artifacts/pal-hook-support-map.json`
- Read only: `libs/api/src/abi.rs`, `libs/api/src/abi/syscall.rs`, `libs/api/src/lib.rs`
- Read only: `libs/ostd/src/lib.rs`, `libs/ostd/src/startup.rs`, `libs/ostd/src/entry.rs`, `libs/ostd/src/heap.rs`, `libs/ostd/src/args.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/sync.rs`, `libs/ostd/src/app.rs`, `libs/ostd/src/clients.rs`
- Read only: `kernel/Cargo.toml`, `kernel/src/task/syscall.rs`, `kernel/src/task/drivers.rs`, `kernel/src/task/drivers/virtio_rng.rs`
- Read only: pinned Rust `library/std/src/rt.rs`, `library/std/src/sys/**`, `library/alloc/src/alloc.rs`, and panic/unwind/TLS call sites reached by Phase 01
- Create during feasibility execution: `artifacts/runtime-api-contract.md`
- Create during feasibility execution: `approvals/runtime-contract.md`

## Implementation Steps

1. Define a minimum executable lifecycle from loader entry through normal exit and panic/OOM termination for each existing Cellos architecture, marking architecture gaps Deferred.
2. For panic/unwind, allocator, and language TLS, state invariants, divergence/error semantics, required hooks, and dependencies; reconcile every item to the support map.
3. Build an API family table with `Supported|Unsupported|Deferred`, exact observable behavior, capability source, and whether availability is required for the initial profile. Keep `PAL-019` Deferred because its default entropy backing is non-qualifying; keep `PAL-031` Deferred while named approval reviews its completed technical backing and evidence.
4. Confirm `std` creates no new authority and that every service-backed operation retains admission/capability checks. Record the completed hostile direct-syscall evidence for null, overflowed, oversized, unmapped, kernel, and peer writable pointers, with rejection before access.
5. Record invalidation triggers: frozen ABI drift, closed kernel security-backing path/digest drift, production `dev-weak-rng`, entropy behavior drift, pointer-validation drift, loader/startup ABI drift, allocator concurrency-model change, thread model introduction, panic strategy change, or pinned-toolchain change.
6. Obtain SDK/runtime-owner and security-owner approval only after the blockers are implemented and evidenced; require 2× explicit approval separately if the contract discovers any frozen ABI change.

## Success Criteria

- [x] Panic/unwind, allocator/OOM, startup/exit, language TLS/threading, and API-family semantics are explicit and cross-referenced to support-map hook IDs.
- [x] Every unavailable API fails observably; there are no no-op or success-shaped stubs, including deterministic entropy reported as success.
- [x] Network TLS and language TLS are explicitly separated.
- [x] `PAL-019` remains Deferred pending a no-`dev-weak-rng` production tuple and real entropy-or-zero/error evidence; `PAL-031` technical backing/evidence is complete and remains Deferred pending named approval of this governed rebind.
- [x] The contract preserves frozen ABI and existing capability checks; any future ABI change remains a non-waivable implementation blocker requiring 2× approval.
- [x] SDK/runtime-owner and security-owner approval states are recorded as `NOT GRANTED` and remain blocked on the named security-backing evidence.

## Verification Evidence

The contract and its closed security-backing manifest passed all 36/36 security-manifest tamper attacks within the final 33/33 feasibility suite. Reconciliation confirmed the exact six-path kernel inventory and matching digests/links; final independent security review returned PASS with no findings. Focused QEMU direct-opcode evidence now covers `GetRandom` hostile rejection and final-authorization races. This verifies the fail-closed contract only: current `dev-weak-rng`/zero RNG backing keeps `PAL-019` Deferred; `PAL-031` remains Deferred until named approval of the governed rebind; both human approval rows remain `NOT GRANTED`, and implementation authorization remains blocked.

## Security Considerations

Entropy must fail closed: the current `dev-weak-rng` default over a zero-byte VirtIO RNG stub is development-only and non-qualifying. Writable syscall buffers require bounded, complete caller ownership and writable mapping before dereference; `GetRandom` now has this validation and focused hostile direct-opcode evidence, but PAL-031 remains Deferred until named approval of the governed rebind. Filesystem/network/process APIs cannot gain ambient authority; panic output must not leak secrets or addresses beyond existing policy; allocator/TLS state is per-cell and cannot cross authority boundaries. Error mapping must not turn denial into absence or success.

## Risk Notes

Current `ostd` assumes single-task cells in allocator design, while upstream `std` may instantiate synchronization or TLS machinery earlier than expected. Treat such hidden dependencies as Deferred/blocking rather than emulating them unsafely.

## Deviation Log

None.
