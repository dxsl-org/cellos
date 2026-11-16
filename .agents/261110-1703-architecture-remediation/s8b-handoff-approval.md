# S8b handoff approval: driver model and Tier-1 TCB perimeter (Phase 18)

**Date:** 2026-11-16 · **Request:** [s8b-handoff-request.md](s8b-handoff-request.md)

The owner approved Phase 18 **staged, continuing**:

- **Steps 1–7** run in order under the file set in §3 of the request. Work stops for the owner only on a behaviour difference beyond the request's §4, or when an ABI or manifest change is needed.
- **Tier-gate mismatches:** a crate with a `c-ffi`/`legacy-raw-ptr` allowlist class and a Tier-1 manifest becomes an explicit, signed exception in the allowlist, with the crate and a reason. The list goes into the evidence for later triage. The gate itself is not weakened.
- **`libs/*` baseline:** today's `unsafe` sites in the Tier-1 dependency closure become the baseline. Any new site fails, and the count may only go down.
- **NIC cells:** all nine move to `libs/nic-driver`, and each keeps its chip's host tests. The pending S5 physical captures will test the shared code.
- **Commits:** one per step, in a worktree. Nothing is pushed. Independent reviews run after steps 2–5 and after steps 6–7.
- **Ends** when Phase 18 closes.
