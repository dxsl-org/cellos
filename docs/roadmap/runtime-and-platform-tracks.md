# Runtime and Platform Tracks

**Last updated**: 2026-08-22

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

## Platform Overlays and Capability Gates

- G4 is the planned pure-Rust `rust-std` Tier 1 runtime profile. Its next
  remediation work is `governance-gated`; host results cannot establish a PAL,
  target, sysroot, runtime, or production qualification.
- G5 remains a virtualization-platform research/design overlay after G4. It
  does not prevent Tier 3 QEMU runners, persistence work, or transport parity
  from advancing to their own documented software evidence ceilings.
- Untrusted Linux/POSIX application compatibility stays in Tier 3 VM lanes
  until Tier 2 native domains exist.
- Tier 2 native domains remain unimplemented. [Spec 22](../specs/22-native-domain-cell-implementation-gate.md)
  is the mandatory design and negative-test gate before a private-MMU native
  runtime may be implemented or offered; current native cells remain shared-SAS
  code and are not treated as contained.

The canonical cross-lane execution classes and reopening events are in the
[roadmap capability table](../project-roadmap.md#capability-lanes).

## Manifest-v3 ABI Predesign

Phase 08's Manifest-v3 ABI predesign has a final validator PASS (20/20) and
pinned consumer-inventory/content-digest artifacts. It is explicitly
`PREDESIGN_COMPLETE / PHASE08_BLOCKED`, with direct dependencies on Phases 03,
05, and 07. It adds no Manifest-v3 implementation, readiness determination, or
approval; Phase 08 is not a Tier 2 implementation authorization.

The Phase 07 atomic-publication prerequisite is separately verified, but full
Phase 07 and Phase 08 remain blocked by the Phase 03 provenance/signature,
Phase 04 production-admission, and Tier 2 native-domain gates.

`CELLOS-VFS-SMP-006` is closed: the owner-lifetime lifecycle implementation
passed API90, an RV32 release compile, fresh `test-hooks`, one-hart VFS 2/2,
and two-hart VFS 7/7, followed by final quality and security closure PASS.
RV32 runtime remains unavailable on this host because OpenSBI firmware is
missing; this is a non-blocking compile-only evidence gap, not runtime
evidence.

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

Implementation remains blocked. PAL-019 technical backing binds a production
release tuple built without defaults and a source-equivalent no-default QEMU
companion that returns zero without synthetic success. PAL-031 technical
backing binds caller-owned validation and isolated RV64 QEMU hostile evidence,
including final authorization through writes racing retirement, revocation,
and exact backing-frame reuse. The governed security manifest binds both
technical evidence sets; the authoritative support map keeps PAL-019 and
PAL-031 `Deferred` pending every named approval. This grants neither PAL
support nor real production entropy; the implementation checkpoint and
umbrella Phase 03 production gates remain blocked.

There is currently no Cellos PAL, target JSON, private or published sysroot,
published triple, or Tier 1 `rust-std` runtime. No live benchmark was captured,
no approval is granted, no promotion is authorized, and umbrella Phase 06
remains pending and dependency-blocked on Phase 03.
