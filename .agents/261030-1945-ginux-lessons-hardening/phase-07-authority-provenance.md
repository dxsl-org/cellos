---
phase: 7
title: "Boot authority provenance invariant"
status: pending
priority: P1
effort: "1-1.5w"
dependencies: [6]
tier: thinking
---

# Phase 07: Boot authority provenance invariant

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

The ginux invariant (`/tmp/ginux/docs/decisions/0001-*.md:23-33`): every runtime capability is authorized by the boot root, or is a strict subset of an existing one. Cellos has the pieces:

- per-path `boot_ceiling` (`loader/boot_ceiling.rs:1-97`);
- spawn intersection (`governed_spawn.rs:114-150`);
- CSpace lineage, attenuation and revoke (`cell/cspace.rs:400-770`).

What is missing is an end-to-end proof. The legacy global `CapTable` has no provenance (`cell/cap_registry.rs:65-104`), but the red team found it has no production allocation callers: it is only revoked at retirement (`task.rs:1453-1456`), and `alloc` is used only in its unit tests (`cap_registry.rs:237-267`). It is therefore **deleted**, not extended.

This phase enforces and tests the invariant over the **existing** root. Replacing that hard-coded root with a signed boot manifest is a Law-1 decision (D-07): drafted here, not implemented.

This phase also creates the shared **test-hooks lifecycle image** used by Phases 08 and 09. The production lifecycle builder has no `test-hooks` (`scripts/build-x86_64-c2c-lifecycle-ci.sh:10-18,35-44`), and the integration helper boots QEMU with its default single CPU (`tests/integration/src/lib.rs:796-830`).

## Requirements

- Invariant (Spec 25 annex, mapped to AC-01..AC-05):
  - For every live Cell C: `CapSet(C) ⊆ boot_ceiling(path(C)) ∩ CapSet(spawner(C))`, rooted at `boot_ceiling("/bin/init")`.
  - Every CSpace slot has either a root grant recorded at publication or a parent whose rights ⊇ its own, with a lifetime no longer than its parent's.
- Provenance is recorded at the real CSpace root-slot insertion and publication paths.
- Legacy `CapTable` is removed with its retirement revoke and its tests. The CSpace file-backing helpers in `cap_registry.rs:200-229` are retained. Clean cutover.
- Test-hook audit walk at a quiescent point, under the same locks used for mutation. It emits `AUTH-ENVELOPE: PASS cells=N slots=M` or the first violating `(cell, slot, reason)`.
- Negative tests:
  - (a) spawn requesting caps beyond the ceiling → stripped or denied, per current policy;
  - (b) a test-hook attempt to insert a root slot without provenance → refused;
  - (c) a delegated child after parent revoke → handle dead;
  - (d) a stale-generation handle → refused.
- Test-hooks lifecycle image: a separate build output (it cannot overwrite the production witness), packaging the authority, replacement and provider fixtures. An x86 boot helper with an explicit `-smp` that requires a positive AP-execution marker for 2 CPUs.
- D-07 ADR draft: signed canonical boot authority manifest as the sole root (record shape per Phase 06), insertion sealed after init, and an observer-READ / service-WRITE split **only for objects with a named consumer**. Status `proposed`.

## Assumptions

- **Claim:** legacy `CapTable::alloc` has no production caller. **Confidence:** high (red team). **How to verify:** `xd://lsp` references on `CapTable::alloc` and `CapTable` before deleting.
- **Claim:** the root-slot insertion sites are few and kernel-internal. **Confidence:** medium. **How to verify:** LSP references on the CSpace insert/publish functions in `cell/cspace.rs`.
- **Claim:** a quiescent walk point exists after autoload settles. **Confidence:** medium. **How to verify:** reuse the P01 private-trace snapshot point (controlling plan §"Private implementation runtime evidence").

## Related Files

- Modify: `kernel/src/cell/{cap_registry.rs,cspace.rs}`, `kernel/src/task.rs` (remove legacy revoke), `kernel/src/loader/{boot_ceiling.rs,governed_spawn.rs}` (provenance at publication), `kernel/src/task/cap.rs`.
- Create: an audit-walk module under `kernel/src/cell/` (test-hooks), `scripts/build-x86_64-c2c-lifecycle-hooks-ci.sh`, an `-smp`-aware x86 helper in `tests/integration/src/lib.rs`, a lifecycle-hooks integration test, and `docs/decisions/00NN-signed-boot-authority-manifest.md` (proposed).
- Docs: Spec 25 annex.

## Implementation Steps

1. LSP references for `CapTable` and the CSpace insert/publish sites; log them.
2. Delete legacy `CapTable` and its revoke and tests.
3. Provenance at root-slot insertion and publication.
4. Audit walk plus the four negative hooks.
5. Test-hooks lifecycle image builder; `-smp` helper with AP marker.
6. Run 1-CPU and 2-CPU.
7. Draft the D-07 ADR.

## Success Criteria

- [ ] The test-hooks lifecycle test passes with `AUTH-ENVELOPE: PASS` on 1 CPU and on 2 CPUs (AP marker observed).
- [ ] Hooks (a)–(d) each produce their refusal marker. A negative control build that skips provenance recording fails the audit.
- [ ] No reference to legacy `CapTable` remains (LSP clean). The production lifecycle test `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test local-service-lifecycle-x86` still passes.
- [ ] The D-07 ADR is filed as `proposed`; no Law-1 bytes are changed.

## Security Considerations

This is the core authority invariant. An audit that reads without the mutation locks can report a false PASS, so the lock discipline is part of the review.

## Risk Notes

Serialized P02/P05 ownership: run this as the single admitted slice. Undo: revert and cold boot; there is no persistent state. Deleting `CapTable` is reversible by revert.

## Deviation Log

None.
