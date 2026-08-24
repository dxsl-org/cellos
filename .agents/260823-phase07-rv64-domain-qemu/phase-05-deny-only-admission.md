---
phase: 5
title: "Deny-only domain admission"
status: pending
priority: P1
effort: 2d
dependencies: [1, 2, 3, 4]
tier: thinking
---

# Phase 05: Deny-only domain admission

## Overview

Add an internal, developer-only selection request that can deny an eligible domain build;
it cannot make a loader, installer, manifest, or SAS fallback claim that Tier 2 is ready.

## Requirements

- Keep current signature/floor/manifest-v1/v2 classification byte-for-byte and retain
  Phase 03 ownership of `CELLOS-LOADER-SIG-001`. No Manifest v3 field/bit/parser/writer,
  installer control, route alias, or public syscall number is added.
- Introduce internal `DomainAdmissionRequest` derived only from kernel boot policy/test
  fixture. `evaluate_domain_admission` checks feature, RV64 Sv39/backend, policy generation,
  resource quota, artifact eligibility, copied-IPC readiness, and enforceable capability/MMIO/
  DMA ceiling before builder publication.
- Every failed check returns a denial with no task, `AddressSpace`, ASID, ready entry,
  capability, audit-success event, or SAS fallback. Feature off and policy disabled expose no
  request path. Unsupported architecture returns deny, never a SAS-labelled domain.
- `Enabled → Draining` is one-way for the boot. An admission holds the policy generation
  through all fallible work and publication, then either publishes fully before the linearized
  transition or denies; draining starts Phase 06 teardown and ends disabled after drain.
- Initial accepted test admission MUST have no user MMIO, PCIe DMA, raw virtio-MMIO, or
  grant use. A DMA-capable domain is denied until separate hardware confinement is qualified.

## Architecture

`existing governed preflight → immutable policy snapshot → enforceability checks → complete AddressSpace/task publication or denial`; every error exits before the scheduler-ready commit.

## Assumptions

None — the feature/policy distinction and no-SAS-fallback requirement are binding Spec 22 constraints.

## Related Files

- Modify: `kernel/src/loader.rs`, `kernel/src/loader/mem_spawn_gate.rs`,
  `kernel/src/loader/launch_profile.rs`, `kernel/src/policy.rs`, `kernel/src/audit.rs`.
- Create: `kernel/src/loader/domain_admission.rs`, `kernel/src/loader/domain_admission_tests.rs`.

## Implementation Steps

1. Place the selection after existing governed preflight and before Phase 07-style task
   publication; trusted embedded init is explicitly audited and remains SAS unless an approved
   fixture asks otherwise.
2. Encode denial as structured, non-promotional audit reason values; log policy generation and
   omission reason without artifact bytes or keys.
3. Hold an immutable admission snapshot across AddressSpace build and task publication; make
   Draining invalidate an in-flight snapshot before publication.
4. Deny all capability sets not representable by current private-root rules, including any
   MMIO/DMA request, rather than adding a broad mapping.
5. Test-only markers: `S22-RV64-ADMISSION-DENY: PASS`,
   `S22-RV64-ADMISSION-DRAIN: PASS`, and `S22-RV64-DEFAULT-OFF: PASS`.

## Test Matrix

| Runner | Cases | Gate |
|---|---|---|
| `cargo test -p cellos-kernel domain_admission --features native-domains,test-hooks` | each deny cause, no-publication snapshot, draining race | non-QEMU |
| `cargo test -p cellos-kernel domain_admission --features test-hooks` | feature absent/default-off has no domain option | non-QEMU |
| `bash scripts/qemu-native-domain-test.sh --harts 1 --case admission,rollback` | denial and no SAS fallback on RV64 | RV64 QEMU, 1 hart |

## Success Criteria

- [ ] Policy/build disabled preserves existing Tier-1/Tier-3 launch behavior exactly.
- [ ] Invalid signature, malformed ELF, unsupported arch, exhaustion, and policy drain deny
  before task/domain publication and never fall back to SAS.
- [ ] No path exposes Tier 2 to users, installers, manifests, or the acceptance ledger.

## Security Considerations

Admission is not containment and a valid signature is not a domain. Developer-only marker
success must never modify ledger status or make unsigned SAS code acceptable.

## Risk Notes

Loader ownership is sensitive: preserve the completed atomic-publication cutover and do not
reintroduce direct spawn publication or a second admission signature.

## Deviation Log

None.
