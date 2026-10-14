# 2026-10-12 — Private trace specification

## What happened
Specified private x86 test-hooks trace layout, scoped arm/close, attempt/source/subject propagation and snapshot sidecar in private-trace-spec.md; integrated P01/resource/overview. No implementation/source/ABI edit or commit.

## Decisions
-160byte little-endian records; fixed readiness/pool/subject/component/epoch storage. Host capture identity is not kernel freshness attestation.
-Compact schema logs lifecycle/component boundaries; allocation/retry transitions update counters, not unbounded rows.
-For N≤8root-only children, at mostN+2launch attempts and51+21N≤219records in256slots.37margin is not permission for new events. Rollback isn't admitted child or duplicate terminal.
-Fixed serialized payload56896bytes; header/control/alignment/export overhead additionally measured.128obligation entries may overflow: invalidate observation without changing admission/reclamation.
-Release-ready slot publication, source epochs and Closing writer exclusion are implementation-review requirements, not proven concurrency. Missing/unknown metrics cannot become zero/settled.

## Verification
Throwaway model checked160byte offsets/capacity for N0..8;8record/presence/duplicate-final and5settled negative controls. Not kernel execution or complete parser/concurrency proof.29docs/315local links/no missing paths; acyclic10pending phases/38reciprocal locks/adoptionrows/50ACowners;3graph negative controls;3174source hashes unchanged.

## Next steps
Review source-bound producer memory safety/locking/coverage and specified schema against actual implementation diff before first parked resource run. Existing async baseline remains separate; all gates pending.
