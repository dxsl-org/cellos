# QEMU App-Tiers Pause Checkpoint

> **SUPERSEDED 2026-08-23**: two-hart `S22-RV64-MIGRATION: PASS harts=2` and the
> strict feature-on/off builds have since been recorded in
> `docs/roadmap/current-focus.md` and `.agents/TODO.md`. The "blocked migration"
> notes below are historical; only the manifest predesign source re-pin remains
> open (steward-owned).

## Committed baseline
- `bd5326a` — VFS owner-lifetime lifecycle repair.
- `b57bba1` — closure documentation.
- `daef6d4` / `85a5b87` — Phase 07 atomic prerequisite and Phase 08 predesign.

## Verified before pause
- CELLOS-VFS-SMP-006 closed for RV64: API 90, RV32 release compile, fresh hooks, one-hart 2/2, RV64 SMP 7/7, final quality/security closure.
- Phase 08 predesign validator: frozen/blocked; no V3 implementation.
- Phase 07 RV64 AddressSpace substrate: default-off QEMU scope verified with `S22-RV64-ASPACE` / `ASID-REUSE` on one/two hart; feature-off VFS unchanged.

## Unverified working changes
- RV64 domain scheduler transition implementation (`native-domains`, default-off).
- `scripts/build-native-domain-test-ci.sh` and `scripts/qemu-native-domain-test.sh` runners.
- Manifest v1/v2 QEMU continuity guard and predesign source-state re-pin.
- Ledger QEMU anti-substitution regressions.

## Resume order
1. Fresh strict `-D warnings` feature-on and feature-off RV64 domain builds.
2. Run new native-domain QEMU runner for one/two hart markers.
3. Run Manifest continuity validator + QEMU guard after source re-pin.
4. Run ledger anti-substitution tests; do not promote ledger.
5. Only then start domain user-copy / copied IPC work.

## Non-negotiable boundaries
- No Tier 2 loader/installer route or qualification claim.
- No V3 layout/parser/writer/signer.
- No Phase 03 external-floor/provenance work without physical backend and approvals.
- QEMU evidence is architecture/hart scoped and cannot clear physical, approval, or C9 gates.

## Resume attempt
- Strict RV64 `native-domains,test-hooks` build passes. The strict feature-off
  test-hooks build inside `build-test-hooks-ci.sh` passes; the direct
  feature-off kernel build remains blocked by unrelated unused
  `kernel/src/admission/mod.rs` declarations.
- RV64 QEMU accepts the exact SAS fast-path terminal at one and two harts.
  The one-hart `switch` terminal reaches the root-selection fixture only; it
  does not establish SATP installation for a runnable private task.
- RV64 two-hart `migration` remains blocked. Its runner now requires the
  distinct `S22-RV64-MIGRATION: PASS harts=2` terminal, which is absent. A
  proper witness requires a private root with the explicit supervisor mappings
  needed by the handoff worker; adding those mappings is outside this
  fixture-only phase and must not be replaced with a test-only SATP bypass.
- The frozen Manifest predesign validator currently fails before QEMU:
  `inventory source input content drift`. Do not refresh its frozen artifacts
  or run continuity evidence until the source-state steward re-pins it.

## Executable-root experiment
- An explicitly bounded linker/heap/stack/UART root was built after approval,
  and the handoff worker was bound before hart-1 dispatch. QEMU repeatedly
  raised an S-mode load page fault at `stval=0x8072b010` after the selected
  root's SATP write, although the builder's software translation checks passed.
- The experiment was reverted rather than retaining a panicking test path or
  bypassing SATP. The approved follow-up plan is blocked at
  `.agents/260823-rv64-private-root-execution/plan.md`; detailed evidence is
  `.agents/debug/debug-260823-rv64-private-root-fault.md`.
