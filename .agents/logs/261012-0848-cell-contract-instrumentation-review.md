# 2026-10-12 — Instrumentation source review

## What happened
Two independent reviewers inspected publication/retirement and allocator/reclamation slices. Parent mapped launch pre-ready and out-of-lock reaper boundaries. Eleven findings consolidated into5Major/1Minor design groups; resource contract, P01 and overview amended; instrumentation-owner-review.md added.

## Decisions
- Subject/attempt/route identities need private propagation; Elf path is caller hint, not filesystem attestation. Parked replicas remain root-only; rejected review premise that they are worker threads.
- Host unique capture headers bind runs/streams/artifacts; no kernel-attested boot freshness claim or boot-ID ABI.
- Save incarnation before task removal; TID-only cleanup/DMA/deferred records cannot attribute late release. Observe queue-full retained frames before provenance loss.
- Actual heap usage remains global until historical allocation owner metadata exists. Current execution-context charge/refund is not allocation ownership.
- Actual heap maxima require allocator critical-section updates without extra locks; post-allocation snapshots can miss peaks. Include leaked guard frames.
-256trace events is candidate only: arm/scope and worst-case fan-out/retry proof required. Observation overflow invalidates capture without changing launch/cleanup.

## Verification
28docs/308local links/0missing paths; acyclic10pending phases;38reciprocal locks/adoption rows;50ACowners;3negative controls.3174source input hashes unchanged; prior configured ISO digest and1/2CPU START/PASS logs preserved. No new runtime test, source/ABI edits or commit.

## Next steps
Exact private trace layout/metadata propagation/capacity/coherence design and implementation-diff review precede parked resource execution. P01 and both checkpoints pending; no old-plan unlock.
