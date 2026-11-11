# S5b handoff approval — one IPC operation model (phase 11)

**Date:** 2026-11-11 · **Request:** [s5b-handoff-request.md](s5b-handoff-request.md)

The owner approved S5b **staged**:

- **Now:** step 1 only. Classify the send, lookup and `TaskState` reader call sites and write `cp1-ipc-model.md`. This is read-only plus documents: no change to `libs/api`, `libs/ostd`, `cells/` or the kernel.
- **Steps 2–8:** only after the owner confirms CP1 (Law-1 confirmation 1). They then use the request §3 file set.
- **Kernel `Rename`:** stays in the kernel as an exact operation with expiry.
- **Commits:** one per step, split per directory inside migration steps. Nothing is pushed.
- **Lane:** the Intel WIP lane moves to S5b; the iGPU DMA-export slice keeps waiting.
- **Ends** when Phase 11 closes.
