# 2026-10-12 — Cell contract P01 preparation

## What happened
Created ABI delta, 38-plan migration, authority/50-AC handoff and profile-budget inventories. No source/ABI edits, runtime activation or commit. P01 and all later phases remain pending; 38 scoped locks remain.

## Decisions
- Prepare exact approval prerequisites, not invent target bytes or measured capacity.
- Removed two obsolete Spec25 controller-ledger paragraphs; approved independent Budget Authority semantics unchanged.
- Source drift in signing/PAL/legacy VMX paths is explicit, not treated as production closure.

## Evidence and limits
- Document smoke:22 documents,264 local links,0 missing paths;38 adoption rows/reciprocal locks;50 AC owner rows;10 pending phases/acyclic DAG.
- In-memory cycle, missing-dependency and missing-link controls rejected.
- Find failed provider-side; configured rust-analyzer exited on CapId references. Fallback source sightings are not exhaustive caller closure.
- Current quota table64 includes excludedzero; operation bound64 is perTask; watchdog500ticks is not aggregate CPU quota. No runtime measurement or qualification claimed.

## Next steps
- Close exact caller/generated/artifact census and measured finite parameter gates before exact Law-1 checkpoint1 proposals.
- Keep protected Build/activation, Intel VMX/physical and production approvals independent; no predecessor unlock or fixture-backed growth.
