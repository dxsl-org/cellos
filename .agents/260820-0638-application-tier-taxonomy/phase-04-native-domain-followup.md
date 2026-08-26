# Phase 04 - Native Domain Follow-Up

## Context Links

- `docs/specs/18-cell-trust-tiers.md`
- `docs/specs/19-hardware-isolation-layers.md`
- `docs/security-model.md`
- `kernel/src/memory/paging.rs`
- `kernel/src/task/tcb.rs`

## Overview

Priority: P3. Status: completed. Effort: 3h planning only. Tier: thinking.

Keep actual Tier 2 native domains separate from the terminology cleanup. This
phase delivered the future design gate; it is not implementation approval.

## Requirements

- Functional: define prerequisites for private page tables, domain scheduling,
  IPC/grant mapping, and admission selection.
- Non-functional: do not conflate signature absence with implemented containment.
- Backwards compatibility: Tier 1 SAS performance remains unchanged.

## Architecture / Data Flow

Admission evidence enters the loader, selects Tier 1 or future Tier 2 only after
hardware-domain support exists. Tier 2 tasks run with private mappings; IPC data
crosses via copied messages or explicitly mapped grants.

## Related Code Files

Design candidates only: `kernel/src/memory/paging.rs`, scheduler/context-switch
paths, loader admission, grant mapping, architecture MMU backends.

## Implementation Steps

1. Write a separate Tier 2 spec/update before code.
2. Trace context-switch lifetime and page-table ownership.
3. Define copied IPC vs explicit grant mapping contracts.
4. Add threat model and failure-mode tests before implementation.

## Todo List

- [x] Separate Tier 2 ADR/spec (`docs/specs/22-native-domain-cell-implementation-gate.md`).
- [x] Architecture-specific MMU feasibility matrix.
- [x] Define IPC/grant data-flow and required negative-test contracts.
- [x] Define rollback and feature-flag plan.

## Success Criteria

- Tier 2 has a separate accepted design/implementation gate covering approval,
  required tests, feature gating, and rollback.
- The gate explicitly preserves the Tier-1 SAS fast path and does not authorize
  runtime page-table switches or native-domain admission.

## Risk Assessment

- High x High: accidental Tier 1 performance regression. Mitigation: feature
  gate and preserve SAS->SAS fast path.
- High x High: false containment claim. Mitigation: do not expose user choice
  until page-table mechanism and negative tests exist.
- Undo: disable feature and revert domain path.
- Irreversible: on-disk app metadata version changes would be hard to roll back;
  avoid them until manifest v3 is explicitly approved.

## Security Considerations

Tier 2 is a containment feature. It needs negative tests showing hostile native
code cannot read SAS memory, not just successful boot tests.

## Next Steps

Open a separate implementation plan only after review of Spec 22. Its owner must
implement the private address-space, scheduler, syscall-copy, IPC/grant, DMA,
admission, and rollback contracts, then execute the required hostile negative
tests. Until then, Tier 2 remains unimplemented and unsigned native code is not
contained by this phase.

## Evidence

- `docs/specs/22-native-domain-cell-implementation-gate.md` exists and is marked
  “Accepted design gate 2026-08-21. Not implementation approval.”
- The spec contains the architecture feasibility matrix (§2.2), SAS fast-path
  invariant (§2.3), recoverable domain-aware syscall-copy contract (§2.4),
  copied-IPC and explicit-grant contract (§2.5), DMA/IOMMU boundary (§2.6),
  admission compatibility (§2.7), required negative-test matrix (§3), and
  feature/rollback policy (§4).
- `git diff --check` completed with exit 0; `git status --short` showed no
  kernel or other product-source modifications from this phase.
- The amended Specs 18/19 and `docs/security-model.md` explicitly state that
  Tier 2 remains unimplemented and that Spec 22 is the required gate.
