---
title: "Complete x86 HAL separation"
description: "Add the generic ACPI PC board descriptor and move static x86 platform facts into hal/soc/x86 without weakening firmware gates."
status: completed
priority: P1
effort: 6h
branch: fix/structure
tags: [refactor, critical]
blockedBy: []
blocks: []
created: 2026-08-18
---

# Complete x86 HAL Separation

## Overview

Bring the previously merged x86 hardware lane under the same board/SoC ownership model as ARM and RISC-V. Preserve the verified boot order and keep all ACPI-discovered addresses fail-closed.

## Scope Contract

- Deliverables: generic x86_64 ACPI PC descriptor, `hal/soc/x86` profile, kernel selection/integration, boundary/build gates, and current docs.
- Boundaries: `boards/` has identity/firmware/wiring/driver selection only; `hal/soc/x86` owns static PC facts; `hal/arch/x86` retains mechanisms; shared drivers remain single-copy.
- Runtime invariant: `boot -> COM1 -> ACPI -> timer -> SMP -> PCIe -> NVMe`; QEMU evidence is not physical-hardware evidence.
- Excluded: adding a second physical x86 board, inventing fallback LAPIC/IOAPIC/HPET/MCFG addresses, moving ACPI parsing into a board package.

## Phases

| Phase | Name | Status |
|---|---|---|
| 1 | [Model generic x86 PC](./phase-01-model-generic-x86-pc.md) | completed |
| 2 | [Consume x86 board and SoC policy](./phase-02-consume-x86-policy.md) | completed |
| 3 | [Enforce and verify x86 boundaries](./phase-03-enforce-and-verify.md) | completed |

## Dependencies

- Phase 2 depends on Phase 1 public types and constants.
- Phase 3 depends on the complete integration diff.
