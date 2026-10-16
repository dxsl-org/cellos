---
phase: 2
title: "Kernel Cell and shared object-capability authority"
status: complete
priority: P1
tier: thinking
dependencies: [1]
---

# P02 — Kernel Cell and CSpace

## Deliverable and requirements

AC-01..AC-05: an exceptional Kernel root principal, atomic bootstrap delegation and a single shared authority space per Cell incarnation. Implement only after P01 exact authority API checkpoint 1. Keep existing service-specific semantic authority outside the kernel.

AC-04/AC-05 are shared criteria, not fully discharged here: P02 proves kernel
delegation and admission cut; P05 supplies service-resource delegation and
CPU/DMA quiesced-completion evidence. P02 completion cannot enable an unqualified
device/memory revocation path.

## Related files / ownership

- Modify `kernel/src/main.rs`, `kernel/src/loader/{governed_spawn.rs,launch_profile/,boot_ceiling.rs}`.
- Modify `kernel/src/task/{tcb.rs,cap.rs,launch.rs,scheduler.rs}`, `kernel/src/cell/{registry.rs,cap_registry.rs,service_registry.rs}`, `kernel/src/task.rs`.
- Public definitions and wrappers: approved subset of `libs/api/src/abi/`, `libs/types/`, `libs/ostd/`.
- Boot consumers: `cells/tools/init/src/{service_table.rs,supervisor.rs}` and launch-policy generation/manifest consumers found in P01.
- New internal CSpace implementation belongs under existing `kernel/src/cell/`; exact module/file split chosen in P01, not a competing task registry.

## Implementation steps

1. Introduce the internal per-incarnation Cell control/authority record referenced by its TCBs. Reuse CellOwner/generation/publication lifecycle. Reserve Kernel identity explicitly; deny payload-forged/ordinary allocated root identity.
2. Make bootstrap grants explicit and bounded, issued before child publication; seal bootstrap-only routes after handoff. Preserve minimal BootFS loading until P07 proves its replacement/remaining boundary. Init receives scoped recovery rights, not an all-powerful credential.
3. Implement bounded typed generational slots and one-parent revocation lineage with charged depth/node limits. Allocate metadata at object/grant creation; invocation and terminal paths do no avoidable heap allocation or full-table scan.
4. Define lock order in code: scheduler/lifecycle membership, ordered CSpace records, object lineage; do not hold these across service IPC, allocator slow paths or device waits. Preparation reserves memory first and revalidates at publish. Linearize lookup/use against revoke; retain object references until quiescence.
5. Implement grant/attenuate/delegate/close/revoke with immutable ceiling and explicit receive policy. Parent close retains live descendant lineage; issuer death invalidates issuer objects; resource-specific detach/owner-tied policy is explicit. Charge retained ancestor metadata to the resource issuer's bounded ledger, not a dead caller.
6. Migrate existing task-local capability consumers to authoritative shared resolution. Remove stale snapshots and path/name minting only when every launch edge has equivalent artifact-attested scoped authority. Do not temporarily replace exact launch profiles with ambient Spawn permission.
7. Preserve current object-specific service handles as service authority, not duplicate kernel resource ACL. Migration of service protocols stays P05; before then preserved checks remain authoritative and documented.
8. Integrate root retirement closing the CSpace before new spawn/delegation and only release slots after exact incarnation quiescence. Fail closed on generation exhaustion and partial preparation.

## Verification / success criteria

- [ ] Unit/model scenarios: foreign/stale/wrong-kind handles, parent close vs revoke, over-depth delegation, ceiling/receive-policy widening, exhaustion, resource rollback and concurrent revoke.
- [ ] Boot x86 image; inspect Kernel/init/child authority ledger and deny ungranted spawn/publish/kill. Real child with a permitted narrow grant succeeds; same request by an ungranted peer does not deliver.
- [ ] Invoke through sibling threads before/after revoke and incarnation replacement; no copied authority survives. P03 supplies full SMP race qualification; do not claim it here from single-CPU proof.
- [ ] Validate artifact provenance on every migrated launch route including caller-supplied ELF bytes; arbitrary bytes cannot borrow a privileged path label.
- [ ] P01 migration ledger covers every replaced consumer and removal; Law-1 checkpoint 2 freezes only observed exact delta/evidence.

Correction 2026-10-16: prior checkmarks and cited standalone-model/storage-boot receipts did not establish these criteria. Acceptance remains pending; historical observations are not runtime qualification.

## Security and rollback

Kernel root is nontransferable, not a capability apps can acquire. Opaque handles must be protected by actual execution-tier admission; SAS unsafe code remains TCB. Refuse new profile on failure and cold-boot a known-safe image with old valid enforcement, never mixed live CSpaces. Revoke cannot undo effects already admitted; do not reuse handles/epochs on binary rollback.

## Assumptions and deviation log

Existing registry/cap state is uneven (CellId-only cap ownership versus generation-aware CellOwner). Treat migration atomicity as a proof obligation, not a cast. Exact kernel identity encoding and table sizes come from P01 approved revision. Deviation log: none.
## Proposed N02-A prerequisite handoff — not active

P02 hands only the internal per-authority arena invariant prerequisite to controlling P01 under [native development handoff r1](native-development-handoff-r1.md), conditional on later explicit reviewed revision/digest approval and decision record. Allowed implementation is limited to the existing cspace.rs module/actual-module harness; scheduler/cap/cell consumers are read-only. No public byte/API cutover, global registry bound, native dispatcher execution, device/Gone qualification or full P02 acceptance follows. WIP is one tranche; N02-B and all later successors require separate exact handoffs. All 38 legacy locks stay effective.

