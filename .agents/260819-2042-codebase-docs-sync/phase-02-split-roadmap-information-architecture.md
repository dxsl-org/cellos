---
phase: 2
title: "Split Roadmap Information Architecture"
status: completed
priority: P1
effort: "5h"
dependencies: [1]
tier: thinking
---

# Phase 2: Split Roadmap Information Architecture

## Overview

Turn the 1807-line roadmap into a lookup index plus topic-owned roadmap files. Preserve history while stopping the single-file accumulator pattern.

## Requirements

- Functional: `docs/project-roadmap.md` becomes a short index; moved sections keep stable destinations and backlinks.
- Non-functional: preserve anchors where practical, keep history separate from current commitments, and avoid duplicate status text.

## Architecture

Data flow: baseline + current roadmap headings enter a section-routing map; sections transform into topic files; `project-roadmap.md` exits as the navigation and current priority summary.

Proposed destinations:

- `docs/roadmap/current-status.md`
- `docs/roadmap/product-stages.md`
- `docs/roadmap/hardware-tracks.md`
- `docs/roadmap/runtime-and-platform-tracks.md`
- `docs/roadmap/completed-history.md`

## Assumptions

- **Claim:** Five roadmap subfiles are enough to split current content without creating a directory maze.
  **Confidence:** medium
  **How to verify:** route every `##` and `###` heading from `docs/project-roadmap.md`; add a sixth file only if one destination exceeds 500 lines.

## Related Files

- Modify: `docs/project-roadmap.md`
- Create: `docs/roadmap/current-status.md`
- Create: `docs/roadmap/product-stages.md`
- Create: `docs/roadmap/hardware-tracks.md`
- Create: `docs/roadmap/runtime-and-platform-tracks.md`
- Create: `docs/roadmap/completed-history.md`

## Implementation Steps

1. Build a heading map from `rg -n "^#{1,3} " docs/project-roadmap.md`.
2. Move stage overlays and milestone-stage tables into `product-stages.md`.
3. Move hardware bring-up, board, driver, chipset, and real-hardware qualification tracks into `hardware-tracks.md`.
4. Move app/runtime/platform, net-broker, G4 std, G5 VMM, and service backlog into `runtime-and-platform-tracks.md`.
5. Move completed work, old phase blocks, dated next-step snapshots, and release history into `completed-history.md`.
6. Keep `current-status.md` focused on active priorities, known blockers, and near-term acceptance gates.
7. Replace `project-roadmap.md` with an index, current top priorities, and links to the subfiles.

## Success Criteria

- [ ] `docs/project-roadmap.md` is under 250 lines.
- [ ] Every original top-level section is either moved or intentionally retained.
- [ ] No roadmap destination owns both current status and archived history for the same topic.
- [ ] Links from index to each subfile work with repository-relative Markdown links.

## Security Considerations

Do not upgrade security posture claims while moving text. Fleet-secure admission remains open unless Phase 4 documents a verified closure.

## Risk Notes

- High likelihood x high impact: moving too much can break familiar anchors. Mitigation: add backlinks and a moved-section map in `current-status.md`.
- Medium likelihood x medium impact: duplicated status can drift again. Mitigation: one owner per topic, index links rather than repeats.
- Rollback: restore `docs/project-roadmap.md` from git and delete `docs/roadmap/`. Irreversible part: none.

## Deviation Log

The final split uses `current-focus.md` and `technical-milestones.md` in
addition to the planned topic names; the index links to the actual owners.
