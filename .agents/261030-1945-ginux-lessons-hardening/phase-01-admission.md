---
phase: 1
title: "Admission, ownership and CellosFS inputs"
status: pending
priority: P1
effort: "0.5d"
dependencies: []
tier: fast
---

# Phase 01: Admission, ownership and CellosFS inputs

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Docs-only phase. It gets maintainer admission for phases 02–09 under `CELL-CONTRACT-0024`, and it hands item 7 (two-slot transactional record) to the plan that already owns CellosFS crash safety, instead of opening a second storage plan.

## Requirements

- Functional: portfolio admission entry; one adoption row per phase in the controlling plan (file set, owner phase, acceptance mapping, Law-1 status, evidence ceiling); a bounded adopted-slice entry in `.agents/cell-contract-0024-locks.md` per phase, as its handoff procedure requires (`cell-contract-0024-locks.md:308-312`, `plan-portfolio.md:46-50`); ginux design inputs appended to `261025-cellosfs-crash-safety/plan.md`.
- Decisions to obtain: admission and order; README vs controlling-plan status truth; **D-04a** (NMI non-fatal); **D-08** (AHCI fail-closed: no FLR means no re-grant until cold boot).
- Non-functional: no source edits; all 38 existing lock relations are preserved unless the maintainer narrows a specifically applicable scope.

## Architecture

| Phase | Intel C2C link (portfolio rule) | Owner | Ceiling |
|---|---|---|---|
| 02 | SMP prerequisite: per-CPU vs shared state before P03 multicore | P02/P03 | host + CI |
| 03 | P10 measurement on the x86 lane | P10 | x86 q35 KVM (budget) / TCG (report) |
| 04 | Node kernel integrity on Intel entry paths | HAL x86; P02–P06 for `user_copy` | x86 q35 TCG + KVM |
| 05 | Truthful status and POSIX contract (ADR-0018) | P07 | host + CI |
| 06 | Law-1 hygiene for future records | P01 | host + x86 q35 |
| 07 | Spec 25 AC-01..AC-05 evidence | P02/P05 | x86 q35 |
| 08 | P05 device/buffer revoke under ambiguous timeout | P05/P04 | x86 q35 |
| 09 | P06 AC-14..AC-18 replacement evidence | P06 | x86 q35 |

CellosFS inputs to record in 261025 (decisions stay with that plan):
1. Per-record SHA-256 vs the current CRC32C: CRC detects torn writes, not deliberate tampering. Pick one and state why.
2. Accept only the newest **unambiguous** committed generation; two equal generations with different content means refuse to mount.
3. Write order: prepare → flush → commit → flush.
4. Keep "runtime never formats" (`vfs/src/manager.rs:68-70`) as an explicit negative test: foreign, corrupt and geometry-mismatched media are refused, not reformatted.

## Assumptions

- **Claim:** admission = portfolio entry + controlling-plan adoption rows + adopted-slice entries in the lock register, with the 38 relations retained. **Confidence:** medium. **How to verify:** read `cell-contract-0024-locks.md:308-312` and portfolio §"No automatic unlock" with the maintainer.

## Related Files

- Modify: `.agents/plan-portfolio.md`, `.agents/261011-1200-cell-contract-authority/plan.md` (adoption ledger section), `.agents/cell-contract-0024-locks.md` (adopted-slice entries), `.agents/261025-cellosfs-crash-safety/plan.md` (new "External design inputs" section), this plan's `status`.

## Implementation Steps

1. Present the table above, the four decisions, and the README/plan status conflict (scout report §Observed inconsistency) to the maintainer.
2. After approval: add the portfolio entry, the adoption rows and the lock-register adopted-slice entries (exact files, owner, acceptance ceiling, retained gates, remaining lock scope). Set `status: admitted` here.
3. Append the four CellosFS inputs to 261025 under a dated heading, citing `/tmp/ginux/docs/STORAGE.md` and `crates/volume-service/src/lib.rs:168-244`.
4. Run `python3 scripts/check-spec-anchors.py --check`.

## Success Criteria

- [ ] Portfolio contains an admission entry that names the maintainer decision and date.
- [ ] Controlling plan has 8 adoption rows (02–09), each with file set, owner phase, Law-1 status and ceiling.
- [ ] 261025 plan has the inputs section; its status and phases are otherwise unchanged.
- [ ] Lock register has one adopted-slice entry per admitted phase; all 38 existing relations are still present (count check).
- [ ] `check-spec-anchors.py --check` passes; decisions D-04a and D-08 are recorded with the date.

## Security Considerations

N/A (documentation).

## Risk Notes

If admission is refused, the whole plan stays parked. Undo: revert the doc commit. Nothing irreversible.

## Deviation Log

None.
