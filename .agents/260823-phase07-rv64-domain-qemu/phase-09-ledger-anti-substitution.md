---
phase: 9
title: "Ledger anti-substitution guard"
status: pending
priority: P1
effort: 1d
dependencies: []
tier: thinking
---

# Phase 09: Ledger anti-substitution guard

## Overview

Add a narrow validation fixture that prevents QEMU evidence from being substituted for a
physical/KVM/approval witness or from silently promoting the app-tier ledger.

## Requirements

- Preserve `docs/app-tier-acceptance-ledger.json` as the authoritative, append-only ledger;
  matrix markdown remains projection only. Current expected validator terminal is
  `PASS: C9=NOT_COMPLETE`.
- Test candidate evidence tuples against architecture, environment, host VMM, hart count,
  firmware digest, command, runner, logs/artifact digests, owner, TTL, and required negative
  cases. A QEMU RV64 witness may match only `qemu-rv64`; it cannot satisfy physical RV64,
  ARM64 KVM, x86 KVM, unrelated hart count, a security-negative, human approval, or a
  ledger lifecycle transition.
- Reject evidence with `Tier2`, `Tier 2`, `USABLE`, `PASS`, `FULLY_QUALIFIED`, or C9
  promotion semantics when sourced only from this child. Preserve all blockers, including
  Phase 03 signature/floor, Phase 04 qualification, DMA/physical hostile evidence, approvals,
  and release closure.
- This is source-disjoint: it changes only validator test fixtures/modules and a new evidence
  schema fixture. It MUST NOT modify the ledger JSON, manifest artifacts, native-domain sources,
  hypervisor sources, or CI status meaning without separate steward approval.

## Architecture

`candidate evidence tuple → validator subject/provenance/negative checks → accepted non-qualifying record or rejection`; C9 calculation remains solely canonical-ledger driven.

## Assumptions

None — `C9=NOT_COMPLETE` and subject separation are existing ledger contract facts.

## Related Files

- Modify: `scripts/app_tier_acceptance/validator.py`, `tests/app-tier-acceptance/test_review_regressions.py`.
- Create: `tests/app-tier-acceptance/test_qemu_substitution.py`,
  `tests/app-tier-acceptance/fixture-data/qemu-substitution.json`.
- Excluded: `docs/app-tier-acceptance-ledger.json` and every Phase 01–08/10 source surface.

## Implementation Steps

1. Add candidate fixtures for QEMU RV64 one/two hart, ARM64 TCG machinery, ARM64 KVM boot,
   x86 KVM, wrong architecture, wrong VMM, hash mutation, expired evidence, missing negative,
   and promotion wording.
2. Validate all invalid substitutions fail with a specific field/subject mismatch and preserve
   the input ledger bytes; validate a correctly labelled QEMU input remains non-qualifying.
3. Assert the canonical current ledger still returns `C9=NOT_COMPLETE`; do not seed a new
   PASS event, lifecycle event, or evidence record as part of this work.

## Test Matrix

| Exact runner | Expected result | Scope |
|---|---|---|
| `python3 -m unittest discover -s tests/app-tier-acceptance -p 'test_qemu_substitution.py'` | hostile substitution fixtures rejected | host validator |
| `python3 scripts/validate-app-tier-acceptance.py` | `PASS: C9=NOT_COMPLETE` | current ledger |
| `python3 -m unittest discover -s tests/app-tier-acceptance -p 'test_*.py'` | existing ledger contract remains valid | host validator |

## Success Criteria

- [ ] QEMU cannot stand in for KVM, physical, approval, or a different tuple.
- [ ] QEMU evidence cannot create a qualifying status or change C9.
- [ ] Terminal is `LEDGER_SUBSTITUTION_GUARD_COMPLETE / C9_NOT_COMPLETE`.

## Security Considerations

The validator treats subject identity and provenance as security boundaries. Never trust an
artifact filename, self-reported marker, or assertion without its bound tuple/digest.

## Risk Notes

This is a guard, not evidence import. Any desired ledger mutation belongs to the app-tier
steward after independent review and all program gates, not this parallel entry.

## Deviation Log

None.
