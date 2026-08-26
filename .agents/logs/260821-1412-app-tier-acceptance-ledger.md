# 2026-08-21 — App-tier acceptance ledger

## What happened
Implemented and adversarially hardened the Phase 02 acceptance ledger, validator, CI baseline resolver, review projection, and regression suite. Independent verification passed 33/33 tests; the program correctly remains `C9=NOT_COMPLETE`.

## Decisions
- Qualification uses exact canonical Rust build denominators: three native targets across the 32-value feature lattice.
- Runtime environments remain independent witness facts; unratified rust-std, FFI, and Tier-2 applicability cannot qualify.
- Supplied baselines are always validated; unchanged snapshots are no-ops and changed snapshots require one append-only transition.
- Manual CI resolves the parent of the latest first-parent ledger-changing commit instead of trusting `HEAD^` after unrelated commits.
- Physical evidence checks cache only within one top-level validation so later same-size mutations remain detectable.

## Lessons
- Coverage set equality is insufficient when the membership key omits ratified compiler, flags, profile, runtime, or tier fields.
- Long lifecycle fixtures need per-validation evidence caching, but persistent caches would weaken tamper detection.

## Next steps
- Commit the ratified contract and ledger source revision.
- Record the real adjacent Phase 02 lifecycle transition against a trusted external baseline.
- Keep Phase 02 and C9 open until those integration events exist.
