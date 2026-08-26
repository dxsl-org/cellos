# D37 — Triage the apparent never-started plan batch

**Status:** approved/applied 2026-08-01. Documentation/portfolio only.

## Finding

Checkbox/status metadata is too stale to justify a blanket defer. Several named examples
have implementation evidence despite untouched plan checklists (robot demo, MMC,
compositor grants, KASLR, release repair, portions of VFS/network/reliability). Marking
them "deferred" would replace one false status with another.

The portfolio needs an explicit four-way state: active, queued/deferred, completed, and
superseded/retired. Exact implementation truth belongs in source/tests and generated
status; a portfolio index owns scheduling intent.

## Recommended ruling [FINAL]

**Reject blanket defer; approve evidence-based triage.**

1. Create one portfolio index listing the canonical active program, queued programs,
   completed/closed records, and retired/superseded plans.
2. Defer only genuinely unstarted work with a named trigger/owner; retire plans whose
   scope was removed; mark landed-but-stale plans completed/superseded.
3. Treat untouched checkboxes as historical, not proof that code is absent.
4. Re-run the inventory after D34-D39; do not advertise the old 76/20/9/44/3 counts as current.
