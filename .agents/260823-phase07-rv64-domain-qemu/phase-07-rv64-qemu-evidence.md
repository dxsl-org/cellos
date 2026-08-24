---
phase: 7
title: "RV64 QEMU domain evidence"
status: pending
priority: P1
effort: 2d
dependencies: [1, 2, 3, 4, 5, 6]
tier: thinking
---

# Phase 07: RV64 QEMU domain evidence

## Overview

Build a deterministic native-domain guest fixture and QEMU assertion runner. This collects
implementation evidence only; it is not qualification, physical closure, or ledger promotion.

## Requirements

- Create a test-only fixture that requests the internal policy after boot; production images
  keep both controls off. Build runner must compile RV64 with `native-domains,test-hooks`,
  sign fixture inputs with existing test signing flow, and create no manifest V3 bytes.
- `scripts/qemu-native-domain-test.sh --harts {1|2} --case <csv>` builds the
  fresh isolated `native-domains,test-hooks` artifact, then invokes
  `qemu-system-riscv64 -machine virt -m 256M -nographic -bios default -smp <harts>`.
  It accepts only `switch`, `sas-fastpath`, and `migration`; each case gets its
  own raw/normalized log plus QEMU version, ELF digest, feature tuple, firmware
  descriptor, exact command, and hart-scoped metadata. It rejects panic,
  unclassified cell fault, an S22 failure terminal, an absent exact terminal,
  duplicate case, or one-hart migration.
- Required runner case IDs: `switch` (domain-switch terminal), `sas-fastpath`
  (no SATP/flush terminal), and `migration` (two-hart domain-switch terminal).
  Each maps to one isolated QEMU invocation and PASS/FAIL result; it makes no
  claim for the broader Spec 22 matrix.
- One-hart may prove local paths only. Cases 07–09 and tag reuse require a non-SKIP two-hart
  run; no runner may relabel one-hart output as SMP evidence. QEMU results are labeled
  `environment=qemu`, `architecture=riscv64`, `hart_count=N`, `host_vmm=QEMU TCG`.

## Architecture

`case manifest + signed fixture → isolated RV64 test image → QEMU command → raw/normalized logs → anchored case terminals + bound evidence manifest`; host parser controls completion.

## Assumptions

None — all QEMU conclusions are explicitly scoped to captured command, architecture, firmware, and hart count.

## Related Files

- Create: `scripts/build-native-domain-test-ci.sh`, `scripts/qemu-native-domain-test.sh`,
  `tests/integration/tests/native-domain-qemu.rs`, `tests/guests/native-domain-probe/`.
- Modify: `scripts/assert-boot-markers.sh`, `kernel/src/embedded-test-hooks/` only for the
  test image. This phase MUST NOT modify loader, scheduler, paging, ABI, manifest, or ledger source.

## Implementation Steps

1. Make build output a separate `cellos-kernel-native-domain-test`, never overwrite standard
   or existing test-hooks images.
2. Have the guest print bounded structured terminals `S22-RV64 CASE=<id> HARTS=<n> RESULT=PASS`;
   the runner compares all expected IDs, rejects duplicates/SKIP where required, and prints
   `S22-RV64-QEMU-SUITE: PASS HARTS=<n>` only after its full requested set.
3. Run one-hart local matrix and two-hart cross-hart matrix in distinct log directories; retain
   the actual QEMU version rather than a generic "QEMU passed" assertion.
4. Publish an evidence manifest with hashes/commands, but label it `NON_QUALIFYING_QEMU` and
   reject any `PASS`, `USABLE`, `FULLY_QUALIFIED`, or readiness mutation in the ledger.

## Test Matrix

| Exact runner | Expected evidence | Scope |
|---|---|---|
| `bash scripts/build-native-domain-test-ci.sh` | fresh isolated `native-domains,test-hooks` RV64 artifact | build only |
| `bash scripts/qemu-native-domain-test.sh --harts 1 --case switch,sas-fastpath` | exact local S22 terminals; `S22-RV64-QEMU-SUITE: PASS HARTS=1` | QEMU RV64, 1 hart |
| `bash scripts/qemu-native-domain-test.sh --harts 2 --case migration` | exact cross-hart domain-switch terminal; `S22-RV64-QEMU-SUITE: PASS HARTS=2` | QEMU RV64, 2 harts |
| `cargo test --manifest-path tests/integration/Cargo.toml --test native-domain-qemu -- --ignored --nocapture` | invokes the one- and two-hart runner matrix | explicit RV64 QEMU integration target |

## Success Criteria

- [ ] Every Spec 22 negative has a target test or an explicit still-blocking hardware reason.
- [ ] Raw/normalized logs bind the exact QEMU configuration and no marker is accepted by prefix.
- [ ] Terminal is `S22_RV64_QEMU_EVIDENCE_COMPLETE / TIER2_UNQUALIFIED / LEDGER_BLOCKED`.

## Security Considerations

QEMU counters/logs are evidence inputs, not security controls. A guest must not control the
host assertion format, request list, expected hart count, or accepted terminal.

## Risk Notes

QEMU TCG has no physical IOMMU/DMA containment proof. A passing two-hart `virt` run is not a
claim about more harts, another firmware, physical RV64, Tier 3, production admission, or C9.

## Deviation Log

None.
