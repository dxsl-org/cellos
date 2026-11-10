# Admission approval — architecture remediation (plan 261110-1703)

**Date:** 2026-11-10 · **Approver:** the accountable owner (the user), answering the Phase 00 form in-session · **Request:** [admission-request.md](admission-request.md)

Answers recorded verbatim (form values):

| Item | Answer | Meaning |
|---|---|---|
| Admission | `yes` | Plan admitted as measured-defect work under ADR-0022; no ADR amendment |
| Scope | `all` | Slices S0–S8 in the planned order (S7 may move ahead of S5/S6, never in parallel) |
| S0 exception | `yes` | File-bounded exception for exactly request §4; effective now; ends when Phase 01 closes |
| D1 | `exclude-keep-source` | Spec 26 skeleton crates leave workspace `members` and images; source kept with a "not wired, fail-open" doc line |
| D2 | `promote` | `CellAuthority` becomes the single per-incarnation authority record (Phase 07) |
| D3 | `cfg-gate` | `snapshot.rs` gated behind `snapshot-qualified` in place (Phase 09) |
| D4 | `quarantine` | 32-bit targets kept, quarantined under `kernel/src/arch/legacy` (Phase 14) |
| x86 breadth lane | `record-s4-closed` | Owner states x86 breadth S4 (phases 10–12) is closed; the Intel WIP lane is recorded free |
| Notes | — | none |

## Limits of this approval

- Admitting the plan and the slice order does **not** admit S1–S8 execution. Each slice needs its own handoff request with an exact file set; slices with `libs/api`/`libs/types` changes also need Law-1 CP1 before edits and CP2 after evidence.
- No ABI, protected, physical or production gate is released. All 38 `CELL-CONTRACT-0024` relations keep their whole-plan scope.
- The S4 close is recorded on the owner's statement; no new verification of S4 was run for this record. S5 (NIC driver code already landed) has no handoff record; that is left for the x86 breadth plan owner and is not touched here.
