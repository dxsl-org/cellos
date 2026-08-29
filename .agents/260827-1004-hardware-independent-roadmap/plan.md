---
title: "Hardware-Independent Capability Roadmap"
description: "Replace global G1→G2→G3 execution ordering with parallel capability lanes while preserving physical and production qualification gates."
status: in-progress
priority: P1
effort: "36.5d"
branch: main
tags: [roadmap, hardware-gates, qemu, sdk, security]
blockedBy: []
blocks: []
created: 2026-08-27
---

# Hardware-Independent Capability Roadmap

## Scope Contract

Deliver a dependency-based execution program for work that can advance on host/QEMU before boards, sensors, secure roots, cloud accounts, or NPU SDKs arrive. Product stages remain release labels; they stop acting as a global work queue. QEMU/host evidence never becomes physical, secure-root, cloud, NPU, or production qualification.

## Decision

Use capability lanes with an evidence ladder: `contract → host tests → QEMU runtime → physical/service evidence → production promotion`. This fits the existing board/HAL/shared-driver split and the repository's explicit QEMU-versus-hardware rules. Reject both strict G1→G2→G3 serialization and blanket removal of hardware gates.
Estimated implementation total is 36.5d for one pass through every phase, including one 0.5d Phase 08 projection. Phase 08 costs another 0.5d for each later lane transition.


## Phases

| Phase | Workstream | Execution class | Evidence ceiling | Depends on |
|---|---|---|---|---|
| 01 | [Cut over roadmap dependency model](./phase-01-roadmap-dependency-cutover.md) | ready | contract | — |
| 02 | [Close hardware-independent security defects](./phase-02-security-prerequisite-closure.md) | governance-gated | host | 01 |
| 03 | [Define the next QEMU desktop and SDK slice](./phase-03-desktop-sdk-qemu-lane.md) | scope-gated | qemu | 01 |
| 04 | [Reconcile and complete local Cell-to-Cell runtime](./phase-04-local-c2c-runtime.md) | scope-gated | host | 01 |
| 05 | [RPi3 HDMI software boundary — completed](./phase-05-rpi3-hdmi-software-gate.md) | ready | host | 01 |
| 06 | [Build Tier 3 hostile QEMU runners](./phase-06-tier3-qemu-evidence.md) | scope-gated | qemu | 01 |
| 07 | [Authenticate software evidence pipeline](./phase-07-authenticated-evidence-pipeline.md) | ready | host | 01 |
| 08 | [Project completed lane status](./phase-08-roadmap-ledger-sync.md) | ready | contract | 01 |
| 09 | [Implement ARM64 Tier 3 persistent QEMU storage](./phase-09-tier3-persistent-qemu-storage.md) | scope-gated | qemu | 01, 06 |
| 10 | [Pin and wire x86 Tier 3 VirtIO parity](./phase-10-tier3-x86-virtio-parity.md) | scope-gated | qemu | 01, 06, 09 |

Canonical axes: `execution_class ∈ {ready, scope-gated, contract-gated, governance-gated, external-gated}` and `evidence_ceiling ∈ {none, contract, host, qemu, physical, service, production}`. Phase frontmatter `status` tracks plan lifecycle only.

## Dependency Graph

`01 → {02,03,04,05,06,07,08}; {01,06} → 09; {01,06,09} → 10`. Phase 05 completed its `kernel/src/task/syscall.rs` work and handed ownership to Phase 02. Phase 06 first freezes scenario matrices, guest probes, and runners without production edits. Phase 09 then owns the shared persistent block backend and must pass those scenarios; Phase 10 owns x86 transport integration and must pass the same runners. Phase 07 owns evidence schema; Phase 08 owns roll-up status. No all-lanes join gate exists.

## Ownership Matrix

| Shared surface | Sole integration owner | Producers |
|---|---|---|
| `kernel/src/task/syscall.rs` | Phase 02 after completed Phase 05 handoff | HDMI and `GetRandom` child slices |
| Tier 3 VMM/VirtIO production files | Phase 09 → Phase 10 | Persistent block backend, then x86 transport parity |
| Tier 3 hostile scenarios/runners | Phase 06 | Phases 09/10 consume runners without transferring production-file ownership |
| Evidence schema/validator | Phase 07 | All implementation lanes emit raw evidence only |
| Roadmap, risk register, child status, acceptance ledger | Phase 08 | Every lane emits a bounded status record |

## External Parked Lanes

- RPi3 sensors, VF2, Pioneer, RPi4, and physical x86: retained physical logs only.
- VF2 + STM32H573 + OPTIGA TPM + named AWS DEV account/region: required before protected-authority Phase 4 entry gates.
- RK3588 + accepted RKNN package/license: required before any accelerator probe; X390 remains the second implementation.
- Production root: remains blocked by ADR-0006 and a superseding GO ADR.

## Non-Goals

No `ViAccelerator`/tensor ABI, placeholder NPU probe, Manifest-v3 implementation, Tier-2 readiness claim, rust-std target/sysroot, generic KMS signing API, DEV_REFERENCE downgrade, or QEMU-to-hardware status promotion.

## Primary References

`docs/project-roadmap.md`; `docs/roadmap/{current-focus,hardware-tracks,product-stages,runtime-and-platform-tracks,open-risk-register}.md`; `.agents/TODO.md`; App Tiers, KMS/Silo, RPi3, and common-driver plans cited in `reports/scout-report.md`.
