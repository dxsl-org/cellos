---
phase: 2
title: "Mutable-global inventory and drift gate"
status: pending
priority: P2
effort: "2d"
dependencies: [1]
tier: medium
---

# Phase 02: Mutable-global inventory and drift gate

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Generate an inventory of every writable global in `kernel/` and `hal/`, classify each by owner and synchronization, and fail CI on any new unclassified entry. This is the ginux F3 idea (`/tmp/ginux/docs/FEEDBACK-HARDENING-RESEARCH.md:135-160`). It makes the existing written rule (`docs/specs/00-context.md:223`) true or explicitly excepted, before P03 multicore and before Phase 04 adds per-CPU IST state.

## Requirements

- Functional: a script that lists `static mut`, `static X: <interior-mutable type>` (Spinlock, Mutex, RwLock, Atomic*, UnsafeCell, OnceCell, Lazy), and `extern static mut` linker symbols, keyed by `path::item` (not line numbers, so edits don't churn); a checked-in classification file; a `--check` mode.
- Classes: `immutable-after-init`, `bootstrap-only`, `per-cpu`, `per-cell`, `per-task`, `shared-synchronized`, `linker-symbol`, `hardware-table`. Each entry also gets an `owner`, a `sync`, and a `reason`.
- Non-functional: no kernel behavior change in this phase.

## Architecture

`scripts/mutable-globals.py` uses a token-level scan of Rust sources (comment- and string-aware; it does not need a full parser), producing `docs/generated/mutable-globals.generated.md`. Classification lives in `scripts/mutable-globals.toml`. `--check` fails on: an unclassified item, a stale entry, or a raw `static mut` classified as anything other than `bootstrap-only`, `linker-symbol`, `hardware-table` or `per-cpu`. Those four classes cover the current raw statics: Limine requests, boot buffers, IDT/CpuLocal.

## Assumptions

- **Claim:** the scout's counts (24 `static mut` in `kernel/`, 15 in `hal/`) are complete. **Confidence:** medium (grouped search). **How to verify:** the generator's first run is authoritative; reconcile and log the difference.
- **Claim:** macro-generated statics are rare enough to list by hand. **Confidence:** medium. **How to verify:** `grep -rn 'macro_rules' kernel hal` for macros that emit `static`.

## Related Files

- Create: `scripts/mutable-globals.py`, `scripts/mutable-globals.toml`, `scripts/test_mutable_globals.py`, `docs/generated/mutable-globals.generated.md`.
- Modify: `.github/workflows/ci.yml` (one step beside `check-spec-anchors`); `docs/specs/00-context.md:223` (restate the rule as "raw `static mut` only in the four allowlisted classes, enforced by `mutable-globals.py --check`").

## Implementation Steps

1. Write the generator with stable `path::item` keys; emit Markdown grouped by class.
2. Run it and classify every item. Owner/sync come from reading each declaration; unknown items are marked `UNKNOWN` with a reason, never guessed.
3. Write host tests: a fixture tree with one item per class, an unclassified item (must fail), a stale entry (must fail), a raw `static mut` in a forbidden class (must fail), and comment/string false-positives (must be ignored).
4. Add the CI step. Update the 00-context rule.
5. Record every `UNKNOWN` and every raw `static mut` in a forbidden class as a named finding, with the controlling-plan phase that owns its code. Do not migrate code here.

## Success Criteria

- [ ] `python3 scripts/mutable-globals.py --check` exits 0 on the tree; `python3 -m unittest scripts/test_mutable_globals.py` passes, with each of the 4 negative fixtures failing for the stated reason.
- [ ] Every raw `static mut` is in an allowed class or listed as a finding with an owner.
- [ ] CI runs the check.

## Security Considerations

The inventory documents per-CPU and entry-path state that Phase 04 relies on. It enforces nothing at runtime.

## Risk Notes

A token scanner can miss statics produced by macros: the hand list covers them, and step 3's fixtures pin the scanner's contract. Undo: remove the CI step and the files.

## Deviation Log

None.
