---
phase: 3
title: "Sync Architecture And Codebase Docs"
status: completed
priority: P1
effort: "4h"
dependencies: [1]
tier: medium
---

# Phase 3: Sync Architecture And Codebase Docs

## Overview

Update the architecture, summary, and standards docs so they describe current source instead of older crate counts and pre-HAL-split structure.

## Requirements

- Functional: reflect current module layout, board/SoC/HAL boundaries, active runtimes, and HAL-to-kernel ABI ownership.
- Non-functional: keep generated counts linked to generated metrics instead of hard-coded prose where possible.

## Architecture

Data flow: Phase 1 baseline enters three docs; docs transform stale structure and ownership sections; output is a consistent description of `boards -> hal/soc -> hal/arch -> kernel` and active cells/services/drivers.

## Assumptions

- **Claim:** The current draft changes in these docs are partially useful.
  **Confidence:** medium
  **How to verify:** compare each changed sentence with Phase 1 baseline before keeping it.

## Related Files

- Modify: `docs/codebase-summary.md`
- Modify: `docs/system-architecture.md`
- Modify: `docs/code-standards.md`

## Implementation Steps

1. Update codebase summary counts and directory tree from the Phase 1 baseline.
2. Replace any claim that every cell forbids unsafe with the narrower audited-boundary rule if source confirms exceptions.
3. Align HAL wording with shared ABI owner `hal/traits/arch/src/kernel_abi.rs` and caller/definition evidence.
4. Align runtime wording: Lua active; MicroPython historical unless a current crate is found.
5. In architecture docs, keep data-first board ownership and SoC profile boundaries; avoid saying board descriptors own shared drivers.
6. Replace hard-coded moving nLOC totals with links to `docs/code-metrics.generated.md`.

## Success Criteria

- [ ] The three docs agree on active runtimes and HAL ownership.
- [ ] No stale crate counts remain unless backed by a current command output.
- [ ] Every code symbol named in these docs exists in Phase 1 evidence.

## Security Considerations

Architecture docs must not imply complete fleet signing, PKU enforcement, or two-node net-broker runtime unless Phase 1 verifies those closures.

## Risk Notes

- Medium likelihood x high impact: architecture docs can overstate completion while summarizing. Mitigation: use status labels: shipped, partial, historical, planned.
- Medium likelihood x medium impact: crate counts drift quickly. Mitigation: prefer generated metrics links and dated baseline notes.
- Rollback: restore these three docs from git; roadmap split remains unaffected. Irreversible part: none.

## Deviation Log

Completed in the merged docs-only change set; generated metrics references
were retained rather than hand-editing derived counts.
