---
title: "Phase 4 DEV_REFERENCE Authority Execution"
description: "Build and evidence the approved VF2 UART-root-stream, STM32H573/SLB9672 authority, and AWS signed-time candidate without changing KMS opcodes 9–14."
status: blocked
priority: P1
effort: "not estimated"
branch: main
tags: [security, kms, dev-reference, visionfive2, stm32h5, tpm, signed-time]
blockedBy: [hardware-assets, aws-dev-account]
blocks: [260825-1726-kms-silo-production-root-phase-4]
created: 2026-08-26
---

# Phase 4 DEV_REFERENCE Authority Execution

## Objective

Produce real AC-001 through AC-011 evidence for the approved Phase 4 Build-entry contract. This plan builds prerequisite authority infrastructure; it does not satisfy post-Build AC-012, begin service-net Phase 4 Build, select a production root, or weaken ADR-0006.

## Entry State

- Candidate architecture approved: VF2 v1.3B UART-root-stream boot, STM32H573I-DK, authority-private OPTIGA TPM SLB 9672, and one-region AWS signed-time service.
- Execution is blocked until exact hardware is physically available and a dedicated AWS DEV account/region is named.
- Procurement, OTP, lifecycle closure, debug lockdown, KMS key creation, and cloud deployment require explicit operator authorization at their phase checkpoints.

## Phases

| Phase | Name | Status | Depends on |
|---|---|---|---|
| 1 | [Admission and Asset Baseline](./phase-01-admission-and-asset-baseline.md) | blocked | hardware, AWS account |
| 2 | [Private Protocol and DEV Separation](./phase-02-private-protocol-and-dev-separation.md) | blocked; SOFTWARE_HARNESS complete | 1 |
| 3 | [VF2 UART Root-Stream Boot](./phase-03-vf2-uart-root-stream-boot.md) | blocked; manifest SOFTWARE_HARNESS complete | 2, exact hardware |
| 4 | [STM32 and TPM Protected Authority](./phase-04-stm32-tpm-protected-authority.md) | in progress; chunked private v2 selected, dual-slot journal `SOFTWARE_HARNESS` complete | 2 |
| 5 | [Nonce-Bound Signed-Time Service](./phase-05-nonce-bound-signed-time-service.md) | in progress; codec, vectors, signer, allocation, receipt, and state-record cores `SOFTWARE_HARNESS` complete | 2 |
| 6 | [Frozen-ABI KMS Authority Integration](./phase-06-frozen-abi-kms-authority-integration.md) | pending | 3, 4, 5 |
| 7 | [Relay Enrollment and Legacy-Signer Compatibility](./phase-07-relay-enrollment-and-mtls-integration.md) | pending | 6 |
| 8 | [Fault Evidence and Entry-Gate Review](./phase-08-fault-evidence-and-entry-gate-review.md) | pending | 7 |

Phases 3, 4, and 5 may execute in parallel after Phase 2. Any hard-stop result blocks Phases 6–8; no fallback lane is permitted.

## Locked Contracts

- Public KMS opcodes and payloads 9–14 remain byte-for-byte unchanged; opcode 14 remains active-only.
- STM32 is the sole authority and TPM bus master. AP callers receive typed operations only; no generic sign, digest, TPM, NV, time, or profile assertion surface exists.
- JH7110 BootROM UART mode must receive the first mutable AP stage solely from the authority. No SD, QSPI, eMMC, USB, network, or AP-measurement fallback.
- The exact SRAM-loader bytes and manifest-verification key are bound into the STiRoT-approved STM32 image/policy; the approved-loader digest is verified before any XMODEM byte and persisted in PERSIST-003 tuples and the OpenBoot fact. Substituted, rolled-back, or truncated loaders never execute.
- Authority state is bound to a stable TPM identity and non-regressing NV counter; every torn or mismatched state recovers the exact tuple or seals.
- Signed time binds device, authority, boot, request, purpose, nonce, source epoch/sequence, Unix floor, and expiry. Outage seals.
- Every artifact remains `DEV_REFERENCE`; production checks retain exact `BLOCKED_BY_ADR_0006` behavior.
- ADR-0008 assigns the complete fixed relay TLS endpoint to the protected authority. Phase 7's opcode-8 probe is deterministic DEV_REFERENCE compatibility evidence only: it never connects to a relay, proves target binding, or satisfies AC-012; production providers deny the signer.
- ADR-0010 freezes one bounded post-loader XMODEM-1K transfer carrying deterministic-CBOR `COSE_Sign1` plus exact OpenSBI/DTB/Cellos/VIFS descriptors. A physically frozen initialized-DRAM aperture contains the pre-cleared quarantine, which is disjoint from loader/final ranges; containment passes before pre-clear and all digests pass before final copies. Cleanup uses an evidenced uncached path or exact clean-to-coherency primitive plus `fence rw,rw`; host clearing cannot prove physical visibility. The loader has no replay floor; protected-state and sole-sender evidence remain mandatory.
- Ownership: BootROM/XMODEM boot-stream framing is a separate pre-runtime protocol owned by Phase 3, outside the Phase 2 closed operation set. Root `Cargo.toml` workspace registration has exactly one serialized owner (Phase 6). Production-checker code stays owned by Phase 2; phases 3–5 hand off marker names only. Phase 7 is restricted to the standalone managed-CA/authority/KMS compatibility probe; ADR-0008 endpoint implementation and every service-net, net-broker, OSTD, and `embedded-tls` change wait for parent Phase 4 after Phase 8 GO.

## Evidence Gate

Phase 8 may mark this plan complete only after actual hardware/cloud traces and an independent security review pass AC-001 through AC-011. Simulator, QEMU, fixture, compile, or unit evidence cannot substitute for the named physical tests.

## Software Track (authorized 2026-08-26)

While hardware is on order, operator-approved software-only work may run in parallel with Phase 1: the Phase 1 admission tooling plus the host-verifiable deliverables of Phases 2–6 (`SOFTWARE_HARNESS`, fixture, simulator, and code artifacts only). This authorization changes no acceptance criterion: every physical/live-cloud scenario, provisioning step, deployment, and admission signature stays gated exactly as written, fixture/simulator output never counts toward any AC, and any acquired-revision divergence requires rework of the affected driver layer — logged per phase.

## Red-Team Verdicts (2026-08-26)

Security red-team (PLAN-BOOT-001, PLAN-TIME-002/003/004, PLAN-EVIDENCE-005) and simplicity reviews both returned NO-GO. Resolutions are applied in the affected phase files and recorded in each Deviation Log without weakening any hard stop, evidence requirement, or frozen contract. Parent Phase 4 remains blocked until Phase 8 returns GO.

## Source Context

- [Approved entry contract](../260825-1726-kms-silo-production-root/spec.md)
- [Candidate research](../reports/research-260826-1605-phase4-dev-reference-lane.md)
- [Codebase scout](./scout-report.md)
- [Parent Phase 4](../260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md)
- [ADR-0008 protected TLS ownership](../../docs/decisions/0008-protected-relay-tls-endpoint-ownership.md)
- [ADR-0010 root-stream manifest](../../docs/decisions/0010-use-canonical-cbor-cose-for-vf2-root-stream-manifests.md)

## Cook Handoff

After Phase 1 becomes unblocked: `/hc-cook .agents/260826-1605-phase4-dev-reference-authority/plan.md`.

Software track runs until Phase 1 admission signs `READY_FOR_PHASE_02`; hardware-gated steps then proceed in the normal phase order.