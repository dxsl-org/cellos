# Runtime and Platform Tracks

**Last updated**: 2026-08-21

This page groups the runtime and higher-level platform overlays that sit next
to the physical hardware tracks.

## Active Native Runtime

- Lua 5.4 is the active native scripting runtime.
- It is the only current native scripting runtime that should be documented as
  supported in-tree. It is a trusted Tier 1 `lua` runtime profile, not a
  separate application tier.

## Historical Runtime

- MicroPython is historical roadmap text only.
- Do not describe it as a current workspace member, supported runtime, or
  shipping Python path.
- Python workloads belong in the Tier 3 Linux VM path.

## Other Runtime-Adjacent Paths

- The WASM host cell is a tool/runtime-adjacent path for `.wasm` workloads.
- Native Tier 1 remains the Rust-first path; use the platform boundary instead
  of drifting POSIX assumptions into native cells.
- Trusted C/C++/Zig interop is the Tier 1 `ffi-posix` profile. Historical
  `Tier 1b` text refers to that profile, not a distinct execution tier.

## Platform Overlays

- G4 is the planned pure-Rust `rust-std` runtime profile for Tier 1.
- G5 is the later virtualization-platform overlay.
- Untrusted Linux/POSIX application compatibility stays in Tier 3 VM lanes
  until Tier 2 native domains exist.
- Tier 2 native domains remain unimplemented. [Spec 22](../specs/22-native-domain-cell-implementation-gate.md)
  is the mandatory design and negative-test gate before a private-MMU native
  runtime may be implemented or offered; current native cells remain shared-SAS
  code and are not treated as contained.

## Tier 1 Rust `std` Feasibility

The Phase 06 feasibility package is verified, but security backing and human
approval remain blocked. The pinned Rust `std` boundary covers 27/27 sys
modules and 36 hooks: 8 Supported, 10 Unsupported, and 18 Deferred, across 46
pinned Rust source files. The selected conditional strategy is an exact,
no-fuzz, content-addressed source overlay against a private matching Rust
checkout, producing an in-tree Cellos PAL and private sysroot. It is not an
external PAL plug-in, target-OS impersonation, `std` over mlibc/POSIX, or
permission to publish a target or triple.

The implemented benchmark validator is fixture-only and non-promotional. Its
synthetic runs can verify deterministic schema, parity, ordering, interference,
and closed-linker-input behavior; they are not live captures or authenticated
promotion evidence.

Implementation remains blocked. `PAL-019` is Deferred because the current
default development tuple enables predictable `dev-weak-rng` success over a
zero-byte VirtIO RNG source. `PAL-031` is Deferred because `GetRandom`
constructs a mutable output slice without first proving bounded, complete,
caller-owned writable provenance. A later implementation child must close both
backing defects and every other blocking Deferred row, retain the exact
six-path kernel security inventory, obtain all six named human approvals and
the implementation checkpoint, and satisfy umbrella Phase 03 production
gates.

There is currently no Cellos PAL, target JSON, private or published sysroot,
published triple, or Tier 1 `rust-std` runtime. No live benchmark was captured,
no approval is granted, no promotion is authorized, and umbrella Phase 06
remains pending and dependency-blocked on Phase 03.
