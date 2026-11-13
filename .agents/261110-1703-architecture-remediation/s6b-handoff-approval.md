# S6b handoff approval — init reduction, hot-swap decision, `api` split (phase 13)

**Date:** 2026-11-13 · **Request:** [s6b-handoff-request.md](s6b-handoff-request.md)

The owner approved S6b **staged**:

- **Now:** step 1 only — the `api` import census, the crate design, the init/kernel contract changes and `cp1-api-split.md`. Read-only plus documents.
- **Steps 2–7:** only after the owner confirms CP1 (Law-1 confirmation 1), under the request §3 file set.
- **Hot-swap:** config completes a real swap through `serve` (key preserved, QEMU proof); the other map entries and dead implementations are removed.
- **Init death:** bounded fault record, then platform reset.
- **Commits:** one per step; the x86 boot lane after every init step. Nothing is pushed.
- **Ends** when Phase 13 closes (S6 closes with it).
**CP1 confirmed 2026-11-13** ([cp1-api-split.md §8](cp1-api-split.md)). Steps 2–7 are open under the [request](s6b-handoff-request.md) §3 file set.
