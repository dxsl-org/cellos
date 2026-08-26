# 2026-08-21 — Phase 04 prequalification

## What happened
Implemented the non-blocked Phase 04 infrastructure: a pinned 18-row negative catalog, 33 stable bare-metal IDs, strict runtime parsing, and focused validator tests. Production admission and admissible evidence remain blocked.

## Decisions
- Separate normal stale-partner admission from no-current old-slot fallback using C3-ADM-032/033.
- Keep the public CLI catalog-only; remove local capture/evidence writing because a process cannot authenticate its own shell, kernel origin, backend, or replay resistance.
- Require signed CI or a secure measured runner before retaining content-addressed Phase 04 execution evidence.

## Lessons
- Content hashes over claimant-supplied context do not establish execution provenance.
- A non-qualifying label does not excuse weak evidence integrity.

## Next steps
- Provision a signed CI/secure measured runner.
- Qualify a real external floor and production parser/common-gate path.
- Run physical replay/power-loss evidence and obtain human/release approvals before ledger PASS.
