# Runtime and Platform Tracks

**Last updated**: 2026-10-08 — runtime work restricted to Intel C2C dependencies

[ADR-0022](../decisions/0022-intel-x86-64-c2c-only-direction.md) makes Intel
x86-64 C2C Anywhere the sole program. This page inventories runtime assets, not
independent programs. New work requires a named C2C consumer, acceptance scenario
and evidence ceiling in the portfolio. Standalone scripting, WASM, Rust std,
language/port breadth and general virtualization expansion are parked.

## Existing Native Runtime

- Lua 5.4 is an existing native scripting runtime, not an active expansion program.
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

- Intel Tier 2/C/C++ support and Intel VMX are direct prerequisites for the C2C
  target, not stages that must wait for G4/G5 product releases.
- Existing pure-Rust `rust-std`/PAL assets may be reused for an identified C2C
  consumer; standalone G4 expansion is parked.
- G5 general virtualization-platform research is parked. Bounded Intel guest
  lifecycle/storage/network and explicit C2C bridge work remain in scope.
- Tier 2 domain substrate exists in test-image profiles, but production AArch64
  and x86 admission still refuses without `test-hooks`. The x86 C++ freestanding
  runtime gap remains. [Spec 22](../specs/22-native-domain-cell-implementation-gate.md)
  and existing ABI/negative-test gates still apply; no generic contained-native
  or arbitrary Linux application support is implied.
- Historical SVM/TCG guest evidence is not Intel VMX qualification. Existing
  non-Intel regressions may protect shared changes without platform expansion.

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

Production promotion remains blocked. PAL-019 technical backing binds a production
release tuple built without defaults and a source-equivalent no-default QEMU
companion that returns zero without synthetic success. PAL-031 technical
backing binds caller-owned validation and isolated RV64 QEMU hostile evidence,
including final authorization through writes racing retirement, revocation,
and exact backing-frame reuse. The governed security manifest binds both
technical evidence sets; the authoritative support map keeps PAL-019 and
PAL-031 `Deferred` pending every named approval. This grants neither PAL
support nor real production entropy; the implementation checkpoint and
umbrella Phase 03 production gates remain blocked.

The in-tree implementation exists and is exercised. `patches/rust-std-cellos.patch`
applies to the pinned `rust-src` (`nightly-2026-05-01`, `f53b654a8`) and carries
the Cellos PAL; `targets/{riscv64gc,aarch64,x86_64}-unknown-cellos.json` are the
private target specs; `scripts/build-cellos-sysroot.sh` builds the private
sysroot overlay for all three, and `scripts/run-std-smoke-qemu.sh` boots the std
cell in QEMU on each of them — riscv64 and aarch64 on the `virt` machine with
virtio-blk, x86_64 through the Limine ISO with nvme/e1000 — asserting the PAL
invariants (freeing and over-aligned allocator paths, `Instant`, `yield_now`,
`available_parallelism = 1`, argv, and the fail-closed fs/net/process arms). The `rust-std-lane` CI job keeps the patch,
the three target specs, and those contract tests honest.

That is a host/QEMU software witness, not qualification: no triple is published,
no live benchmark was captured, the parity evidence stays fixture-only, PAL-019
and PAL-031 remain `Deferred`, no approval is granted, no promotion is
authorized, and umbrella Phase 06 remains pending and dependency-blocked on
Phase 03. The approval records are re-bound to the manifest digest re-pinned
2026-09-29 under `PAL-IMPLEMENTATION-CHECKPOINT` condition 6.

Spec 23's C2-RST row and its closing "Known gaps" sentence now state this
correctly: the amendment landed at revision `fd3d12ae`, archived as
`docs/evidence/spec23-native-sdk-contract-9265afc81b15.md`, and the acceptance
ledger's `source_binding` was re-based onto it together with the Phase 05
(Manifest-v2 tooling) `IMPLEMENTED` transition — the only carrier
`scripts/app_tier_acceptance/ledger.py::baseline` allows. The amendment is prose
only, so the ratified matrix digest is unchanged. The acceptance ledger records
Phase 06 (Tier 1 rust-std PAL) as `IMPLEMENTED` on 2026-09-29 at the bounded
implementation boundary; live benchmark evidence, `PAL-019`/`PAL-031` approval,
promotion and publication remain blocked.
