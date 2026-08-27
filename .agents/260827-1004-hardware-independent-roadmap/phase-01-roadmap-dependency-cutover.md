---
phase: 1
title: "Cut Over Roadmap Dependency Model"
status: completed
priority: P1
effort: "0.5d"
dependencies: []
tier: thinking
---

# Phase 01: Cut Over Roadmap Dependency Model

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.
## Context Links

- `docs/project-roadmap.md`
- `docs/roadmap/current-focus.md`
- `docs/roadmap/hardware-tracks.md`
- `docs/roadmap/runtime-and-platform-tracks.md`
- `.agents/TODO.md`


## Overview

Make capability dependencies and evidence ceilings authoritative so product-stage labels no longer serialize unrelated implementation.
## Key Insights

Product stages describe graduation targets; evidence type and capability dependencies determine executable order.


## Requirements

- Keep G1–G5 as release/market overlays.
- Add the evidence ladder `none → contract → host → QEMU → physical/service → production`.
- Classify every lane on two independent axes: `execution_class ∈ {ready, scope-gated, contract-gated, governance-gated, external-gated}` and `evidence_ceiling ∈ {none, contract, host, qemu, physical, service, production}`.
- Preserve every existing security, hardware, ABI, and human-approval gate.

## Architecture

Roadmap entrypoint links to capability lanes. `current-focus` names executable software work; `hardware-tracks` owns physical gates; `runtime-and-platform-tracks` owns admission/Tier 2/G4 ceilings; the open-risk register owns confirmed gaps.

## Assumptions

None — current roadmap files and active plans were read directly.

## Related Files

- Modify: `docs/project-roadmap.md`
- Modify: `docs/roadmap/current-focus.md`
- Modify: `docs/roadmap/product-stages.md`
- Modify: `docs/roadmap/runtime-and-platform-tracks.md`
- Modify: `docs/roadmap/open-risk-register.md`
- Modify: `.agents/TODO.md`

## Implementation Steps

1. State explicitly that stage order is not execution order.
2. Add the capability table with owner, canonical execution class, evidence ceiling, next slice, and reopening event.
3. Move G3, production root, physical board, and authority assets into parked external lanes without deleting acceptance criteria.
4. Link each active software lane to its owning child plan and stop condition.
5. Reconcile stale claims such as disconnected net-broker modules and already-fixed semihosting status against code/current plans.

## Todo List

- [ ] Publish the capability-lane table and evidence ladder.
- [ ] Reconcile every lane to the canonical execution-class and evidence-ceiling enums.
- [ ] Link every lane to one owning plan and reopening event.

## Success Criteria

- [ ] A reader can identify work executable today without opening the legacy roadmap.
- [ ] No QEMU/compile result is represented as physical or production PASS.
- [ ] G3 has no software implementation task before real vendor/hardware evidence.
- [ ] Every parked lane names the exact event that reopens it.

## Security Considerations

Status changes cannot weaken admission, protected-root, Tier-2, DMA, or signature gates.

## Risk Assessment

Documentation drift is the main risk. Use one lane owner per status and keep the legacy roadmap read-only.

## Next Steps

Open Phases 02–07 in parallel after the authoritative roadmap records the new ownership model.

## Deviation Log

- Published the canonical evidence ladder, execution classes, lane owners, and reopening events in the roadmap entrypoint and linked topic pages.
- `cargo test --workspace` exits 101 before and after this documentation-only phase. The exit-code gate reports no new failure, but is best-effort: the baseline stopped at missing Lua C headers and a missing Tetris C source; the comparison run also reached existing `app-shell` core-prelude errors.
