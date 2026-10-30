---
phase: 2
title: "SAS-aware writable-global ownership inventory"
status: pending
priority: P2
dependencies: [1]
tier: medium
---

# Phase 02: SAS-aware writable-global ownership inventory

## Overview

Adopt item 8 without replacing Rust ownership with blanket locking. Shared address space does not make every global shared mutable authority. A Spinlock alone does not prove principal ownership, interrupt safety or retirement safety.

## Requirements and architecture

- Inventory kernel/HAL writable statics, including interior mutability, linker symbols and macro-generated declarations. Distinguish declaration discovery from a proof of synchronization.
- Record independent dimensions: storage lifetime (boot/per-CPU/per-Cell/per-thread/global); mutation mechanism; semantic owner; execution contexts (normal/IRQ/NMI/AP); initialization seal; retirement/quiescence; safety rationale and review reference.
- `per-cpu` is not an automatic exception: migration, nested interrupts and NMI can still alias that CPU's state. `hardware-table` is not an automatic justification for `static mut`.
- Do not wrap boot tables/CPU-local state in locks solely to satisfy a textual rule. Never add locks or allocation to NMI/terminal paths.
- Stable keys include module path, item and cfg variant, not just source line or item name. Cover active Intel cfgs first; retain other declarations without claiming their platforms are qualified.
- Reuse existing Rust syntax tooling if available. A source/token scanner is a discovery aid: include explicit expansion coverage and unknown types/macros. It must not claim to discover every interior-mutability wrapper from a fixed list of type names.

## Related Files

Candidate new tool/classification: `scripts/mutable-globals.py`, `scripts/mutable-globals.toml`; generated report under `docs/generated/`; CI beside the existing spec-anchor check. Update `docs/specs/00-context.md` only after adopting the ownership rule, not to bless current raw statics.

## Implementation Steps

1. Enumerate declarations and cfg variants; inspect custom wrappers/macros and their expansion strategy.
2. Classify ownership, synchronization, preemption/reentrancy and teardown independently, citing code.
3. For unresolved existing declarations record a bounded, individually reviewed debt entry with owner and reason. New unknown declarations fail; old debt is visible, never represented as certified safe.
4. Exercise the tool against nested modules, cfg duplicates, macro emissions, custom wrappers and comments/strings. Test missed/unclassified declarations and stale entries as behavioral discovery failures, not source-wording snapshots.
5. Integrate a drift gate. Phase 04 adds its IST state to this same inventory.

## Success Criteria

- [ ] Every discovered active-x86 declaration is classified or explicit reviewed debt; no silent UNKNOWN-to-PASS conversion.
- [ ] Add an unclassified declaration in a throwaway fixture: check refuses it. Resolve/remove it: check succeeds.
- [ ] Macro/cfg/custom-wrapper cases have stated coverage; exclusions cannot be mistaken for full LBI proof.
- [ ] No production runtime code, synchronization scheme or hot path changed.

## Assumptions

Prior grep counts are approximate, not authoritative totals. Expansion/tool availability is checked before selecting implementation.

## Security and risk assessment

This is an audit/drift tool, not an LBI verifier. It does not establish safety of dependencies, unsafe code or compiler output. Revert tooling/docs to undo; no persistent effect.

## Deviation Log

SAS review: replaced broad class-based raw-static exemptions with owner/context/lifetime evidence; no automatic Spinlock migration.
