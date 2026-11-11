# S4b handoff approval — kernel residents, selftests, module layout (phase 09)

**Date:** 2026-11-11 · **Request:** [s4b-handoff-request.md](s4b-handoff-request.md)

The owner approved S4b with the request §2 file set, effective now.

- **Spec 15:** pipes, futex and sessions are admitted as kernel residents. Spec 15 is amended to match, with an amendment note.
- **Order:** the dispatcher extraction (request step 5) runs first, then selftest, snapshot, residents, directory hoist, cfgs → features, metric.
- **Commits:** per step, and per domain or directory inside steps 4–5. Nothing is pushed.
- **Law 1:** no delta. Any `libs/api`/`libs/types` need stops for CP1.
- **Lane:** the Intel WIP lane moves to S4b; the iGPU DMA-export slice keeps waiting.
- **Ends** when Phase 09 closes.
