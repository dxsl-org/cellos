---
phase: 3
title: "Inject Failures and Prove Exact Restoration"
status: completed
priority: P1
effort: 1d
dependencies: [1, 2]
tier: thinking
---

# Phase 03: Inject Failures and Prove Exact Restoration

> **Required — deviation-log:** Record each Decision / Deviation / Surprise immediately. Choose the smallest reversible response; escalate any contract-breaking change.

## Overview

Prove ready-last publication under an adversarial scheduler and prove exact pre-state restoration at every denial/resource boundary, then record only the two transferred loader risks as resolved.

## Required Snapshot

The test snapshot MUST compare before/after values for:

- Scheduler tasks including relevant security fields, zombies, `next_task_id`, sweep/pending lists, every hart's current task/cell, and every priority ready queue.
- Frame allocator ownership/free count, PIE-slot availability, stack allocations/guards, and TLB-visible mapping absence after cleanup. AP-02 MUST capture the unpublished `PreparedElfTask` segment VAs while live through an active-page-table translation/PTE query, including raw access flags, then prove those same VAs have no translation after its drop.
- Quota presence/limit/heap use/DMA use, Platform singleton state, VFS handler owner/registration bit, input TID, directory inheritance, argv stash, service/driver routes, and replacement ceilings/bindings.
- Measurement entry count and aggregate plus success `CellSpawn`/`CellMeasure` audit count. A denial audit is expected and compared separately, never confused with successful-launch residue.

For syscall-level consume-on-attempt inputs, first assert the inner publication transaction restored exactly, then assert only the explicitly documented outer argv/dir consumption changed.

## Failure Injection Matrix

| ID | Inject/deny point | Required result |
|---|---|---|
| AP-00 | malformed manifest/signature/privilege/path preflight | no task/resource/success evidence change |
| AP-01 | PIE VA acquired, before segment load | VA and allocator snapshot restored |
| AP-02 | segment page `N` mapped / partial loader failure | each unpublished page's live translation resolves to its owned frame with a nonzero raw PTE/flags; after drop the active walk finds no leaf, no frame, and no TLB-visible mapping |
| AP-03 | relocation failure and W^X lowering failure | segments/VA restored; no writable executable residue |
| AP-04 | kernel-stack allocation succeeds, user-stack allocation fails | both stack/guard and frame state restored |
| AP-05 | Platform singleton unavailable | denial before task publication; no resource delta |
| AP-06 | Platform reservation acquired, then injected failure | latch returns to exact prior state; no cap/task |
| AP-07 | quota registration/slot denial and injected post-reservation failure | limit/use/DMA and task ID restored |
| AP-08 | scheduler unavailable or final precommit checkpoint | all prepared resources/reservations restored |
| AP-09 | VFS mandatory block-region denial | no task, quota, routes, measurement, or zombie |
| AP-10 | SpawnPinned RT+cluster denial | no ready task or spawn-then-exit zombie |
| AP-11 | replacement source invalid/bind reservation fails | exact frozen ceiling/binding state restored |
| AP-12 | single-hart success paused immediately before ready push | child absent from all ready queues; inspect complete TCB/routes/evidence |
| AP-13 | distinct two-hart governed probe after hart 1 is online | remote IPI reaches a real scheduler attempt while publication retains the lock; child cannot execute before final ready push |
| AP-14 | single-hart successful governed spawn | exactly one task/ready entry/quota/measurement/audit; all fields complete |
| AP-15 | successful trusted init spawn | exact boot ceiling + SupervisorCap + critical flag present at first runnable observation; no governed signature change |

No production execution-tier metadata or permanent failure-injection state may be added. Hooks compile only under `test-hooks` and use stable named case IDs.

## Related Files / Ownership

- Create: `kernel/src/loader/atomic_publication_tests.rs` — focused snapshot, injector, matrix, and stable case IDs.
- Modify: `kernel/src/loader.rs` — test module wiring only after Phase 02 behavior is complete.
- Modify: `kernel/src/loader/elf_tests.rs` — invoke matrix and emit aggregate terminal marker.
- Modify: `kernel/src/loader/manifest_section_tests.rs` — reuse/extend existing scheduler snapshot rather than keep a second weaker definition.
- Modify: `docs/specs/19-hardware-isolation-layers.md` — ready-last loader ordering, without native domains/signature claims.
- Modify: `CHANGELOG.md` — transferred loader race/cleanup fix under Unreleased.
- Modify after evidence: `.agents/260821-0642-app-tiers-completion/plan.md` and `phase-07-tier2-native-domain.md` — mark only RACE-002/CLEANUP-003 resolved; retain all Phase 03/04/Tier 2 blocks.

## Implementation Steps

1. Add test-only snapshots/accessors for otherwise private quota/singleton/VA/mapping/measurement state; expose no production diagnostics API.
2. Add deterministic failure hooks at every matrix point and a scheduler barrier that lets another hart attempt to tick/steal.
3. Run AP-12/AP-14 on a pre-secondary governed probe, then arm AP-13 only after hart 1 is online and run its distinct barrier probe. A one-hart runner records AP-13 as `SKIP`, never aggregate success.
4. Run every AP-00 through AP-11 denial against a fresh populated baseline with distinct VFS, input, service, replacement, quota, and route owners/values; compare the complete snapshot and tear that fixture down to its prior state after each case.
5. Compare exact snapshots and separately assert expected denial audit/input consumption.
6. Exercise governed success and direct trusted init success at first-runnable observation.
7. Run the focused boot-time corpus, RV64 `test-hooks` build/boot, production-shaped builds, and unchanged host aggregate; preserve emitted logs as evidence under the program's authenticated-evidence rules.
8. Obtain independent quality and security review focused on lock order, RAII ownership, stale authority, append-only evidence ordering, and shim absence.
9. Update changelog/spec/umbrella only after all proof passes. Emit the exact terminal marker while retaining the Phase 07 block.

## Verification Commands

- `bash scripts/build-test-hooks-ci.sh`
- `cargo test --manifest-path tests/integration/Cargo.toml --test vfs-quota` — one-hart VFS contract; AP-13 must skip.
- `cargo test --manifest-path tests/integration/Cargo.toml --test atomic-publication` — `-smp 2`; AP-00 through AP-15 and aggregate must pass.
- Production-shaped RV64 builds with and without the applicable policy/signing features and `RUSTFLAGS="-D warnings -C relocation-model=pic"`.
- Existing host aggregate used by umbrella Phase 05 (`cargo test -p types -p api --target x86_64-unknown-linux-gnu`) to prove no v1/v2 ABI regression.

## Separate VFS SMP blocker — closed independently

The historical VFS SMP `40 PASS, 10 FAIL` result was a release blocker independent of AP-13. AP-13 continues to prove only the loader's two-hart ready-last observation and MUST NOT be reported as VFS evidence. The separately owned VFS lifecycle work has now closed `CELLOS-VFS-SMP-006` at `CELLOS-VFS-SMP-006_CLOSED_VERIFIED_RV64`, with RV64 two-hart VFS lifecycle `7/7`; see the [owner-lifetime closure record](../260822-cellos-vfs-smp-006-owner-lifetime/plan.md#closure-record--2026-08-22). That closure does not alter this atomic proof or unblock umbrella Phase 07.

## Success Criteria

- [ ] AP-00 through AP-12, AP-14, and AP-15 pass with stable IDs under the single-hart applicable contract; AP-13 is explicitly `SKIP`.
- [ ] The `-smp 2` runner proves AP-13 and emits the AP-00 through AP-15 aggregate; no single-hart result emits that aggregate.
- [ ] Every denial has zero task/zombie/quota/mapping/frame/VA/singleton/route/success-evidence residue.
- [ ] No raw spawn compatibility symbol/call/comment remains.
- [ ] Independent quality and security reviews have no unresolved Critical/High/Medium finding in this slice.
- [ ] Evidence emits `ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED` exactly.
- [ ] Umbrella Phase 07 and Phase 08 remain blocked; `CELLOS-LOADER-SIG-001` remains Phase 03-owned and unresolved here.

## Security Considerations

Run restoration cases against pre-populated singleton/quota/route state to catch rollback that clears another owner's state. Do not claim production loader readiness while SIG-001 remains open.

## Risk Notes

The existing manifest corpus snapshots only scheduler state and only early malformed denials; it is insufficient until allocator/quota/singleton/route/evidence state and late failure points are covered.

## Assumptions

- **Claim:** The existing RV64 test-hooks runner can deterministically hold publication at a pre-ready barrier before secondary-hart startup is changed for the race case.
  **Confidence:** medium
  **How to verify:** During Build, inspect SMP startup ordering and add the smallest test-only two-hart harness if the boot-time runner is still single-hart at this point.

## Deviation Log

None.
