---
title: "Consume IRQ Topology"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 02 — Consume IRQ Topology

## Requirements

- Keep legacy IRQ and local-source public constants source-compatible.
- Source timer enable/pending masks from the topology.
- Preserve register offsets and mechanism code.

## Todo List

- [x] Rewire legacy IRQ aliases.
- [x] Rewire local-source aliases.
- [x] Rewire system-timer IRQ bit use.

## Risk Assessment

An alias mismatch could alter trap dispatch. Rollback restores literal constants; compile and profile tests catch parity drift.

## Success Criteria

ARM HAL exposes the same constants and performs the same register writes with no topology literal duplication.
