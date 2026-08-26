# 2026-08-22 — Phase 07/08 prerequisites

## What happened
Committed the verified Phase 07 atomic-publication prerequisite and Phase 08 Manifest ABI predesign validator. Full Tier 2 and Manifest v3 remain blocked.

## Decisions
- Governed tasks stage complete security state and publish ready last; rollback restores populated shared state, mappings, quotas, routes, replacement state, argv/stash, VFS handler identity, and audit semantics.
- Atomic SMP proof requires an actual remote-hart exclusion witness; one-hart runs explicitly skip AP-13.
- Manifest predesign uses v1/v2 corpus, occurrence-level consumer pins, downgrade matrix, base-revision lineage plus derived source state; no V3 bytes are defined.

## Lessons
- Test fixture state must be independently round-tripped; owner tags alone do not prove fast-IPC handler restoration.
- A base commit identifier is not a live source identity; derived source-state hashes avoid that false claim.

## Next steps
- `CELLOS-VFS-SMP-006` is now closed at `CELLOS-VFS-SMP-006_CLOSED_VERIFIED_RV64`: API `90/0`, RV32 release compile, fresh hooks, one-hart VFS `2/2`, and RV64 two-hart lifecycle `7/7` passed. RV32 runtime remains an explicit non-blocking host-OpenSBI-firmware evidence gap.
- Complete Phase 03/04 and Tier 2 native-domain qualification.
- Obtain Phase 08 approvals before any V3 ABI implementation.
