# 2026-10-12 — Resource attribution contract

## What happened
Inspected existing ProcessInfoV2/CellOwner, quota/global-frame/process counter implementation and scheduler CPU sampling. Wrote resource-attribution-contract.md, integrated P01/plan/profile-budgets; no implementation/code/ABI edit or commit.

## Decisions
- Group by kernel-attested boot/node/CellId/generation; names/TIDs alone cannot support shared-Cell charge attribution. Owner lookup/watch cannot be made generic observer authority.
- ProcessInfoV2 repeats Cell heap charge per thread; owned_bytes is not disjoint physical ownership. Frame allocator committed capacity includes kernel heap reserve; zero deregistered quota counter is not settled reclamation.
- Proposed finite parked counts1/4/8 plus zero-child control,3fresh boots each; one thread/replica, no grants/DMA/retries/restarts. Workload/trace not yet implemented.
- Private test-hooks trace capacity256records, allocated before baseline; overflow invalidates. No lock nesting/allocation/log formatting inside measurement-critical allocation paths.
- Settled needs exact terminal owners and zero workload deferred/quarantine/unknown obligations. Separate live/residual/retained costs. Polling isn't exact high-water; sum of independent maxima isn't simultaneous total.
- CPU scheduler tick sampling is not latency timestamp or exact aggregate execution quota. No finite production budgets inferred.

## Verification
27docs/304local links/no missing paths; acyclic10pending phases/38reciprocal locks/adoption rows/50ACowners.3graph negative controls rejected. Throwaway mathematical controls checked shared-charge dedup/generation separation, rejected4missing/incoherent/pending/quarantine cases and distinguished simultaneouspeak100from sum-of-peaks200. These are design checks, not kernel runtime proof.

## Next steps
Review allocation owner/trace insertion/coherence details before development instrumentation; actual parked-resource evidence and exact checkpoint proposals remain pending. Earlier async runtime evidence unchanged.
