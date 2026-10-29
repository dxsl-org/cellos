# 2026-10-19 — Unified desktop implementation planning

## What happened
Created .agents/261019-1310-unified-desktop/plan.md with 11 pending phases, 20 acceptance IDs, proposed contracts, source recon, verification handoff and adversarial review. Updated TODO pointers; no runtime implementation files changed.

## Decisions
- Execute actual native session/theme/launcher contracts before app migrations; keep one integrating owner for shared SDK/API/kernel changes.
- Shared bounded thumbnails move into Ocel cutover before Files; Quick Look adds scheduling/isolation later.
- Files needs complete directory enumeration and coherent copy/move; Terminal needs inherited external-child I/O and one stdin reader lease.
- Tier3 requires real VMM dispatch, base+payload device topology, app-only guest display and resize, not current launcher success logs.
- Frame configure and group/power close bind exact membership/revisions; file open binds authority to exact launch recipient.

## Lessons
- Existing TID-owned windows, app-local palettes, log-only Terminal catalog and background VM scanout do not implement ADR-0027.
- Cancellation alone cannot terminate stuck PDF C work; a watchdog and confirmed provider death precede grant reuse.
- Completed plan headings do not resolve conflicting frozen-wire/VMX approval records.

## Verification
- Three read-only recon slices and four review lenses; 20 unique findings mitigated in plan (17 Major, 3 Minor).
- Final artifact validator passed: 17 documents, 11 phases, 20 AC IDs, local links, index/header DAG equality, required sections/lengths and failure-ledger JSON.
- No builds/runtime tests run: this task produces a plan, not implementation evidence.

## Next steps
Review plan, then hc-cook /home/dmin/cellos/.agents/261019-1310-unified-desktop/plan.md interactively, beginning with exact phase-01 admission/contracts and separate Law-1 checkpoints.
