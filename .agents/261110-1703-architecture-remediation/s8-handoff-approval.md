# S8 handoff approval: quota ownership and shared-root hardening (Phase 17)

**Date:** 2026-11-15 · **Request:** [s8-handoff-request.md](s8-handoff-request.md)

The owner approved Phase 17 **staged, continuing**:

- **Steps 1–7** run in order under the request's §3 file set, 17a (charging) then 17b (shared-root hardening), in this phase. Work stops for the owner only on a behaviour difference beyond the request's §4, or if an ABI or manifest change is needed.
- **Guest-RAM budget:** a separate per-cell guest-memory budget beside the heap and DMA counters, defaulting to 0. The hypervisor cell's budget is set at launch to cover its largest profile (the Chrome guest, about 1.5 GiB).
- **x86 hardening:** SMEP, SMAP and UMIP are turned on in every x86 profile once their lanes pass. The Cargo features stay as off-switches for one cycle.
- **Commits:** one per step, in a worktree, rebased before the merge. Nothing is pushed. An independent review runs after 17a and after 17b.
- **Ends** when Phase 17 closes.
