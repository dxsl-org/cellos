# D22 — VFS eviction, deadlock detection, and SMP work stealing

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

The three subquestions now have different answers:

### A. Page-cache eviction

Spec 09 §4 assigns eviction policy to the kernel, violating Spec 15. The shipped LRU and
its 90% threshold live entirely in the VFS cell
(`cells/services/vfs/src/page_cache.rs:80-107`). Policy belongs there.

### B. Deadlock watchdog

Spec 04 §6's kernel Resource Graph does not exist. The implemented model is progress/liveness
detection: RT CPU watchdog plus opt-in heartbeat termination, followed by supervisor policy
(`kernel/src/task/scheduler.rs:613-768,830-915`; Spec 12 §4.2). It detects hangs, not lock
cycles, and never selects the “lowest-priority lock participant” from a graph.

### C. Work stealing

The docket's “scaffolding only” premise is stale. `steal_from_busiest` moves bounded
Normal/Background work and never steals RT tasks
(`kernel/src/task/hart_local/ready.rs:103-169`); scheduler fallback calls it at
`kernel/src/task/scheduler.rs:912-914`. The SMP benchmark includes a work-distribution
scenario. Its scope is presently two harts, not arbitrary-N qualification.

## Recommended ruling [FINAL]

**Approve recommendation A:**

1. Spec 09: eviction is VFS-cell policy; the kernel owns only memory/accounting mechanisms.
2. Spec 04 §6: replace Resource-Graph deadlock detection with a pointer to Spec 12's
   watchdog/heartbeat/supervisor model; do not claim cycle diagnosis.
3. Spec 04 §5: mark two-hart Normal/Background work stealing implemented, with RT exclusion
   and benchmark limits. Do not re-mark it G2/unbuilt.

No runtime change is required.
