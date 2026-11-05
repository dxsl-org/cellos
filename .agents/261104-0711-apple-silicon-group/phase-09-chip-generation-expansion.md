---
phase: 9
title: "Chip-generation expansion"
status: pending
priority: P3
effort: "per chip: 8-15d + hardware"
dependencies: ["7"]
tier: thinking
checkpoint: true
---

# Phase 09: Chip-generation expansion (AS-9)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [Research §2, §4](research/asahi-linux-reference.md); ADR-0029 §5 (one image per chip generation)

## Overview

Extend the bare-metal track one chip at a time. Each chip is its own image, backend, profile and HCL row.

| Order | Chip | New blocks | Gate |
|---|---|---|---|
| 1 | M2 (t8112), Mac mini | AICv2, DART t8110 | permissive references exist (OpenBSD) |
| 2 | M1/M2 Pro/Max/Ultra (t600x/t602x) | multi-die AICv2, DRAM at 1 TiB (02b), more PCIe ports | memory capacity for local models |
| 3 | M3 (t8122, t603x) | per Asahi M3 support (2026-09) | a permissive reference for each needed block |
| — | M4 and later | AICv3; PCIe/DART/NVMe undocumented | **parked** until public, permissively referenced documentation exists |

## Key Insights

- The value for local agents grows with memory: 64–192 GB on Max/Ultra parts. That is why t600x/t602x come before M3.

## Requirements

- Per chip: backend, profile, FDT facts, ledger rows, S2-hv then S2 capture, HCL row.

## Related Code Files

- Create per chip: `hal/arch/arm/src/aarch64/backend/apple_<chip>.rs`, `hal/soc/apple/<chip>`, `boards/apple/<model>/**`

## Implementation Steps

1. Pick the next chip by the table; confirm the gate.
2. Repeat phases 04–07 for that chip.

## Todo List

- [ ] M2
- [ ] Pro/Max/Ultra
- [ ] M3

## Success Criteria

- [ ] Each chip has its own S2 HCL row; no row is copied between chips or models.

## Risk Assessment

- Multi-die (Ultra) adds cross-die interrupt and memory topology; treat it as a separate chip.

## Security Considerations

Same DMA rules as phase 05/06 for every chip.

## Next Steps

Revisit M4+ when documentation changes.

## Deviation Log

None.
