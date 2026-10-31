---
phase: 2
title: "SAS-aware writable-global ownership inventory"
status: done
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

- [x] Every discovered active-x86 declaration is classified or explicit reviewed debt; no silent UNKNOWN-to-PASS conversion. 582 declarations discovered; 208 need review on the x86 profile and all 208 have entries (23 debt). `cfg-unknown` predicates count as active, and an `unknown` dimension without `debt` fails.
- [x] Throwaway `static PHASE02_FIXTURE: AtomicU32` added to `kernel/src/audit.rs`: `--check` exits 1 (`unclassified static kernel::audit::PHASE02_FIXTURE`). Removed: exits 0. The fixture tests in `tools/mutable-globals/tests/discovery.rs` cover the same cases (7 tests).
- [x] Macro/cfg/custom-wrapper coverage is stated in the generated report's "Coverage and exclusions" section. Macros are not expanded: static-emitting `macro_rules!`, their invocations, other item macros and `global_asm!` are listed for review instead. Only a closed set of immutable shapes is exempt.
- [x] No production runtime code, synchronization scheme or hot path changed.

## Assumptions

Prior grep counts are approximate, not authoritative totals. Expansion/tool availability is checked before selecting implementation.

## Security and risk assessment

This is an audit/drift tool, not an LBI verifier. It does not establish safety of dependencies, unsafe code or compiler output. Revert tooling/docs to undo; no persistent effect.

## Deviation Log

SAS review: replaced broad class-based raw-static exemptions with owner/context/lifetime evidence; no automatic Spinlock migration.

## Implementation (2026-11-01)

- Tool: `tools/mutable-globals`, a standalone std crate on `syn` 2 rather than a token scanner. It walks the real module tree (`mod`, `#[path]`, inline modules, file `#![cfg]`) and evaluates cfg against the x86 profile using each crate's resolved feature closure. Keys are `crate::module::[fn()|<impl T>]::ITEM @cfg(chain)`.
- Classification: `scripts/mutable-globals.toml`. Report: `docs/generated/mutable-globals.md`. CI runs the tool tests and `--check` in the lint job, beside the spec-anchor check.
- Deviation: the context vocabulary gained `none` (no accessor compiled into the profile; must stand alone), so dead storage is not mislabelled `boot`.
- `kernel/src/intrinsics.rs` and `kernel/src/prelude.rs` are not in the module tree, so they are not compiled; they are acknowledged in `[[unreached]]`.
- The 23 debt entries are real hazards or unestablished dimensions found by the review. They are recorded, not fixed (out of scope; no runtime change). The most severe:
  - `IRQ_MMIO_BASE`: `WaitIrq` stores an unchecked `mmio_base` that the riscv64/aarch64 IRQ path then reads and writes, a kernel write at a caller-chosen address. Not reachable on x86_64.
  - `SHM_HANDLES`: `ShmMap` has no ownership check, and handles and frames are never reclaimed.
  - `ALLOCATOR` / `SCHEDULER`: the heap lock does not mask interrupts, yet IRQ paths allocate.
  - domain_admission `GENERATION` is never written, so the lease generation check is inert.
  - `HART_LOCALS`: written through a `*const`→`*mut` cast of a non-`mut` static.
  - `HAS_X86_VIRT`: one global flag although root mode is per-CPU.
  - audit `RING` is never drained.
  - `BUMP`/`FREE`: cell VA slots leak on every publication.

## Debt follow-up (2026-11-01, user request "xử lý 23 khai báo debt")

- A read-only scout re-verified each entry. Five were wrong and are reclassified with evidence:
  - SCHEDULER and ALLOCATOR: no IF=1 path allocates. The heap is IRQ-masked anyway now, defensively.
  - `RANGES`: x86 copies the higher half and skips those records.
  - `BUMP`/`FREE`: `CellSegments::drop` frees the slot.
  - `IOMMU_CLEANUP_SERIAL`: a latency issue only, with no lock inversion.
- Internal fixes, no semantic change: #2, #5, #11, #12/#13, #16, #19, #20/#21, and #22 (the audit ring keeps the newest records, per the user's choice). Found during the work and also fixed: the `sys_cap_revoke` CELL_AUTHORITIES→SCHEDULER lock-order inversion.
- Fail-closed changes, under CP1 `p02-debt-semantic-cp1.md` (approved): #4, #10, #6 (also corrects the GCAP.ND decode), #7, #3 and #1.
- Still debt: #23 owner admission, blocked by `.agents/260821-1700-tier1-baseline-admission` Phase 03. The user kept it as debt.
- Not fixed, noted: `resource_registry::release_for` is called with the retiring cell id, but MMIO grants are keyed by task id. This is a possible pre-existing grant leak and needs its own review.
- Evidence: see the CHANGELOG `### Fixed` entry.
- Process note: a plain `cargo build -p cellos-kernel` embeds the stale `kernel/src/embedded/init`, so the shell never starts. Rebuild the default RV64 kernel only through `scripts/gen-disk-ci.sh`.
