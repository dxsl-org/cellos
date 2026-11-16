# S8c handoff approval: spec reconciliation, hygiene and closure (Phase 19)

**Date:** 2026-11-16 · **Request:** [s8c-handoff-request.md](s8c-handoff-request.md)

The owner approved Phase 19 **staged, continuing**:

- **Steps 1–9** run in order under the file set in §3 of the request. Work stops for the owner only for a code change beyond deleting dead items, or a finding that needs a decision.
- **Lanes failing on `main`** (`vfs-quota`, `rust-std-promotion`, `x86hv-iso`, EL2 test-hooks): each is diagnosed. A failure that a small, local fix resolves is fixed in this phase. The rest go into the closure matrix as `residual` for the owner's written acceptance.
- **`.agents/` retention:** report sizes only; nothing is moved or deleted.
- **Allowlist re-review wave:** done in this phase, covering every entry with `review_by` before 2027-03-01, site by site.
- **Commits:** one per step, in a worktree. Nothing is pushed. Independent reviews run after steps 1–5 and at the checkpoint.
- **Ends** with the owner's written closure of the plan.
