# S7c handoff approval: frame ownership and shootdown contract (Phase 16)

**Date:** 2026-11-14 · **Request:** [s7c-handoff-request.md](s7c-handoff-request.md)

The owner approved Phase 16 **staged, continuing**:

- **Steps 1–7** run in order under the request §3 file set. Work stops for the owner only on a behaviour difference other than "unconfirmed shared-root invalidation panics → quarantines", or if an ABI change is needed.
- **Quarantine cap:** once quarantined frames exceed 1/64 of RAM, new frame allocations are refused. The cap is reported in the log and by a counter.
- **Hypervisor tables** (EPT, Stage-2, VMCB) migrate in this phase.
- **Commits:** one per step or per owner. Nothing is pushed.
- **Ends** when Phase 16 closes. S7 closes with it.
