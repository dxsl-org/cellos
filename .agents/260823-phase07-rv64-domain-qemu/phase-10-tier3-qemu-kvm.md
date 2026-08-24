---
phase: 10
title: "Tier 3 QEMU/KVM hardening evidence"
status: pending
priority: P2
effort: 2d
dependencies: []
tier: thinking
---

# Phase 10: Tier 3 QEMU/KVM hardening evidence

## Overview

Harden Tier 3 evidence classification and its existing QEMU harness without crossing into
RV64 native domains. ARM64 TCG machinery, ARM64 KVM boot, and x86 KVM are separate subjects.

## Requirements

- Preserve `scripts/qemu-hypervisor-smoke.sh` distinction: `HV_SMOKE_MODE=machinery` is
  QEMU TCG liveness only; `HV_SMOKE_MODE=boot` needs real KVM/EL2 and a guest shell. Do not
  turn the documented TCG nested-walk exception into a generic fault allowance.
- A result record contains target architecture, QEMU version, accelerator (`tcg`/`kvm`), CPU
  model, machine, hart/vCPU count, firmware/kernel/disk hashes, exact command, expected marker,
  and allowed-fault classification. Cross-subject copying is rejected.
- Harden parser assertions to anchor the existing `[hv] vCPU ready` machinery marker and strict
  shell marker, reject panic/cell fault/hypervisor error first, and emit one final scope marker:
  `TIER3-EVIDENCE: PASS ARCH=<...> ACCEL=<...> MODE=<...> VCPUS=<n>`.
- Tier3 QEMU/KVM evidence is never used for RV64 `satp`, AddressSpace, copied IPC, DomainGrant,
  Manifest V3, Tier2 admission, or ledger qualification. RV64 H-ext remains unimplemented/
  unsupported per the current Tier3 guide unless separately designed.

## Architecture

`subject-specific build/artifacts → TCG machinery or KVM strict runner → anchored host assertion → tuple-bound terminal`; no subject shares evidence with another accelerator or architecture.

## Assumptions

None — the existing runner already distinguishes machinery from strict boot; the phase hardens that distinction without redefining it.

## Related Files

- Modify: `scripts/qemu-hypervisor-smoke.sh`, `tests/integration/tests/tier3b-el2-alpine.rs`.
- Create: `tests/integration/tests/tier3-evidence-subject.rs` and a Tier3 evidence fixture.
- Read/conditional only: `kernel/src/hypervisor/**`, `kernel/src/memory/stage2.rs`,
  `kernel/src/memory/ept.rs`, `hal/arch/arm/src/aarch64/**`, `hal/arch/x86/src/hypervisor.rs`.
- Excluded: Phase 01–09 source surfaces, loader/manifest/ledger, and RV64 domain code.

## Implementation Steps

1. Split artifacts and CI/manual invocations by subject directory; each assertion reads only its
   own raw/normalized log and fails if configuration or digest metadata is absent.
2. Retain exactly the ARM64 TCG machinery exception already decoded by the runner; add negative
   fixtures for wrong accelerator, wrong architecture, missing vCPU marker, unexpected fault,
   and false shell prefix.
3. Require KVM mode to verify `/ #` (not vCPU liveness) and return SKIP with host capability
   reason when KVM/EL2 is unavailable; a SKIP is not a PASS and cannot satisfy a KVM row.
4. Report existing Tier3 limitations unchanged: volatile guest backing, only current ARM64 guest
   path, and no RV64 H-extension guest claim.

## Test Matrix

| Exact runner | Expected result | Scope |
|---|---|---|
| `HV_SMOKE_MODE=machinery bash scripts/qemu-hypervisor-smoke.sh` | anchored `[hv] vCPU ready` or only documented decoded TCG outcome | ARM64 QEMU TCG; configured default vCPU count |
| `HV_SMOKE_MODE=boot bash scripts/qemu-hypervisor-smoke.sh` | strict Alpine `/ #`; otherwise explicit host-capability SKIP | ARM64 QEMU KVM/EL2; configured default vCPU count |
| `cargo test -p integration-tests --test tier3-evidence-subject` | marker/tuple substitution negatives | host parser |

## Success Criteria

- [ ] TCG machinery cannot be represented as KVM boot evidence.
- [ ] KVM shell evidence cannot be accepted with a wrong architecture/accelerator/vCPU tuple.
- [ ] Terminal is `TIER3_EVIDENCE_HARDENED / TIER2_UNCHANGED / LEDGER_BLOCKED`.

## Security Considerations

The tolerated TCG fault is narrowly decoded and host-side asserted. KVM access is capability-
detected, never silently replaced by TCG. Guest output is untrusted evidence text.

## Risk Notes

This sidecar is parallel only while source-disjoint. Altering VM syscalls, task capabilities,
or the app-tier ledger requires its owning plan and is not authorized here.

## Deviation Log

None.
