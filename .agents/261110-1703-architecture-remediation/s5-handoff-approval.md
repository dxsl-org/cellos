# S5 handoff approval — lock ranks and scheduler hot paths (phase 10)

**Date:** 2026-11-11 · **Request:** [s5-handoff-request.md](s5-handoff-request.md)

The owner approved S5 with the request §2 file set, effective now.

- **Rank assert:** on in the test posture (`test-hooks`) only. It costs nothing in dev and release kernels.
- **Cycles:** a cycle among the documented lock orders stops the work and goes back to the owner before any code changes.
- **Commits:** one per step; each violation the assert finds is fixed in its own commit. Nothing is pushed.
- **Law 1:** no delta. Any `libs/api`/`libs/types` need stops for CP1.
- **Lane:** the Intel WIP lane moves to S5; the iGPU DMA-export slice keeps waiting. The UI session keeps `cells/` and `libs/`.
- **Ends** when Phase 10 closes.
