---
title: "Consume BCM MMIO Profile"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 02 — Consume BCM MMIO Profile

## Requirements

- Source RPi3 peripheral/local-controller paging bounds from `BCM2837`.
- Source RPi3 GPIO/AUX allowlist bases and widths from `BCM2837`.
- Source GPIO IRQ owner lookup from the same GPIO fact.
- Preserve cfg gates and all existing access flags.

## Related Code Files

- `kernel/src/memory/paging.rs`
- `kernel/src/resource_registry.rs`
- `kernel/src/task/drivers/gpio_irq.rs`

## Todo List

- [x] Rewire paging bounds.
- [x] Rewire resource allowlist.
- [x] Rewire GPIO owner lookup.

## Risk Assessment

A mismatched fact could desynchronize mapping, granting, and notification. Rollback restores the three literals; cross-target compile gates detect cfg regressions.

## Success Criteria

No RPi3 address literal remains in these consumers, and generated behavior is exactly unchanged.
