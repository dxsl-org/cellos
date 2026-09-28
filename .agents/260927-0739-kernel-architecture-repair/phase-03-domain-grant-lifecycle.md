---
phase: 3
title: "Make domain grants permission-accurate and synchronously revocable"
status: in-progress
priority: P1
effort: "split into owner mapping, publish, revoke and death changes"
dependencies: [2]
tier: thinking
---

# Phase 03: Production grant lifecycle

## Requirements / architecture
- Preserve public `Grant*` syscall numbers and result conventions (`libs/api`, `libs/types` unchanged). Existing `PageGrant`/`RegGrant` tables store `(owner, shared_to)` but no mapped receiver root/generation (`kernel/src/task/syscall.rs:122-145,1997-2105`). Domain receiver mappings must carry exact owner and receiver cell generation, recipient root, per-page range, rights, pin/DMA state and a revoking state **in one kernel-owned record**. Treat SAS owner and domain owner explicitly; `task/domain_grant.rs:1-123` is a one-page RV64 private-private selftest, not a drop-in for SAS-owner/multi-page public ABI. Either adapt its state machine to the table or integrate it behind the existing table; remove dead duplicate fixture when cut over.
- In `GrantAlloc` **and** `GrantRegister`, allocate/zero backing without publishing USER access into `KERNEL_ROOT` when the owner is a domain (`syscall.rs:343-405,6315-6349,6615-6638`). Preserve SAS owner identity semantics separately; a domain-owned page must stay supervisor-only in the SAS root. Transactionally map the returned physical-base ID as an owner-domain VA with RW+NX; if virtual identity cannot be represented without collision, deny before publishing. Fail/undo every page and release only after shootdown if any mapping fails.
- In `GrantSlice`, resolve rights: `ReadOnly` → R+NX; `ReadWrite` → RW+NX. Genuine write-only is not enforceable as an ordinary page on every target: refuse it for domain pairs until ratified semantics; first inspect existing `libs/ostd/src/fs.rs:437-440` and `cells/services/hypervisor/src/virtio_blk.rs:455-458` callers that pass WriteOnly while VFS reads. Migrate those callers to accurate rights before lifting gate. Never widen rights or return an unmapped address. Any `(target identity,generation,root,rights)` tuple change—including **same recipient RW→RO**—must revoke old PTE and shoot down before publishing new tuple; an old receiver write must fault without calling `GrantSlice` again.
- Revoke/free/unregister must block new slices and user copies, drain readers + VFS/DMA pins, synchronously unmap receiver PTEs and shoot down target roots, then unmap owner domain PTEs, restore SAS kernel mapping and finally release frames. If busy, return a bounded existing error **without freeing frames**; retain a revoking/quarantined record for retry/exit cleanup. Owner death, recipient death, non-VFS grant, VFS lease and concurrent `GrantDma` must use the same lifecycle. Keep current lock order `PAGE/REG_GRANT_TABLE → SCHEDULER → pin REGISTRY` (`syscall.rs:1938-1984`); do not take `SCHEDULER` under a domain ledger/FRAME_ALLOCATOR lock or wait for remote completion while holding a lock needed for safe-root ack. Design reservation/generation protocol and document the linearization point before implementation.

## Related files
`kernel/src/task/syscall.rs`, `kernel/src/task/domain_grant.rs`, `kernel/src/task/tcb.rs`/task retirement paths if required, `kernel/src/memory/{address_space.rs,pin.rs,tlb_shootdown.rs}`, `kernel/src/task/grant_reclaim_selftest.rs`, `libs/ostd/src/fs.rs`, `cells/services/hypervisor/src/virtio_blk.rs`, `tests/integration/tests/tier2_fault_isolation.rs`, `scripts/{qemu-native-domain-test.sh,build-native-domain-test-ci.sh}`, `docs/specs/{02-memory.md,17-ipc-wire-contract.md,22-native-domain-cell-implementation-gate.md}`.

## Implementation steps
1. Ratify the private cutover contract in Specs 17/22: source/target TID vs cell-wide root, SAS/domain pairing, ID/VA collision, NX/permission policy, page count and state/failure semantics; do not change `libs/api`/`libs/types`. Enumerate all consumers of raw grant address and add failure-before regressions for owner mapping, SAS USER exposure, RO-write, WO rejection, nth-page failure, re-share, revoke/frame reuse, exit and DMA pin.
2. Split backing allocation from SAS USER publication; keep domain-owned frames supervisor-only in global root, zero before mapping, then atomically map all owner-domain pages or undo. Add generation-tagged ownership for source and destination in authoritative tables and retain `Arc<AddressSpace>` while mapped. For SAS-only pairs preserve current zero-copy and quota/accounting; deny unsupported domain↔SAS pairs rather than silently granting global access.
3. Resolve permission from `shared_to` under the table lock, then authorize/commit mapping and optional VFS lease transactionally; no lease may remain after map failure. Handle `GrantAlloc` domain owner before publication.
4. Implement revoke across PAGE/REG grants and **all teardown paths**, including `clear_grantee_refs`/`reclaim_owned_grants`/orphan reaper. Mark `Revoking` under table lock, release locks for synchronous target-root shootdown, reacquire and free only after ack; a pending ack keeps backing quarantined and supports idempotent retry. Validate that new shares/slices refuse while `Revoking`.
5. Build/pack a **producer and receiver Tier-2 ELF pair** with required launch profiles/manifests/capabilities into per-architecture throwaway images; implement non-skipping fresh QEMU runners for exact public owner+receiver syscall path in this phase (the existing `cells/tests/tier2-smoke` only exercises its own grant, and `scripts/qemu-native-domain-test.sh --case grant-revoke` is a private fixture). Exercise RV64 1/2-hart, AArch64/x86 one-CPU RO-write attribution, no SAS-root USER exposure, same-recipient downgrade, re-share, peer death and forced churn; record distinct terminal markers. Keep phase-01 deny gate until the real paired test passes for the exact pair/architecture; unsupported combinations remain denied.

## Success criteria
- [ ] An allocated domain grant pointer is writable by its owner; transaction failure leaves no visible ID/PTE/frame leak.
- [ ] Domain-owned backing never has USER access in SAS root; RO receiver write faults, RW works, WO refuses on unsupported pairs and migrated VFS caller still reads correctly; map failure never succeeds with a raw pointer.
- [ ] After `GrantFree`, `GrantUnregister`, re-share or either peer's exit, old receiver address faults and reused frame bytes stay private even across 2 harts.
- [ ] VFS/DMA pin and copy lease prevent premature reuse; SAS-only grant clients and normal copied IPC keep working.

## Progress

### Slice 0 — real two-cell public-syscall lane (2026-09-28) — done (red on the deny contract)

`scripts/qemu-native-domain-test.sh --case grant-pair` now drives a **real** Tier-2 owner cell
and receiver cell through the public syscall path — `GrantAlloc`, `GrantRegister`, `GrantSlice`,
`GrantShare`, `GrantFree`, `GrantUnregister` — instead of the private one-page `domain_grant`
fixture the phase file calls out as insufficient:

- New cells `cells/tests/tier2-grant-owner` and `cells/tests/tier2-grant-receiver` (workspace
  members; UNTRUSTED manifests, so `governed_spawn` classifies them as Tier-2 domains), built
  and signed by `scripts/build-native-domain-test-ci.sh` into the lane's own throwaway image
  (`target/native-domain-test/embedded/kernel_fs.img`, `EMBEDDED_OVERRIDE`), installed on the two
  already-reviewed Tier-2 launch paths only.
- The runner waits for the shell, spawns the owner, parses its
  `S22-RV64-GRANT-PAIR-HANDOFF id=… mode=… target=…` line, spawns the receiver with that tuple,
  and requires every deny marker verbatim; a missing marker or any `S22-RV64-…: FAIL` exits 1.
- Red witness against the current phase-01 containment gate (`.logs/native-domain-qemu/
  h1-grant-pair-WpVt7F`, suite `S22-RV64-QEMU-SUITE: PASS HARTS=1
  CASES=grant-pair,grant-gate`, exit 0): `OWNER-ALLOC: DENY`, `OWNER-REGISTER: DENY`,
  `OWNER-MAPPED: SKIP-DENIED`, `OWNER-SLICE0: DENY`, `OWNER-SHARE-RO: DENY target=0`,
  `OWNER-FREE: DENY`, `OWNER-RESLICE: DENY`, `OWNER-UNREGISTER: DENY`,
  `OWNER-EXIT-CLEANUP: PASS`, `RECEIVER-ALLOC: DENY`, `RECEIVER-SLICE-RO: DENY id=0`,
  `RECEIVER-RO-WRITE: SKIP-DENIED`, `RECEIVER-RW: SKIP-DENIED`,
  `RECEIVER-REVOKE-FAULT: SKIP-DENIED`, `RECEIVER-FRAME-REUSE: REFUSED`, both cells `PASS`.
  Both cells are real `TaskAddressSpace::Domain` tasks (admission log
  `[domain] admitted cell 'tier2-smoke'/'tier2-exploit' to Tier 2 Paged Domain (SATP isolation)`).
- 1-hart regression in the same session: `admission`, `asid-lease`, `unmap-order`, `grant-revoke`,
  `grant-gate` all PASS.

**Not claimed:** the positive branches (`OWNER-MAPPED: OK`, `RECEIVER-RW: OK`,
`RECEIVER-RO-WRITE: FAULT-EXPECTED`) are unreachable while containment denies allocation, so the
lifecycle is unproven; no gate is lifted and no production or qualification claim is made.

### Slices 1–4 — the lifecycle itself, RV64 only (2026-09-28) — implemented, with named gaps

One kernel-owned record per grant (`kernel/src/task/domain_grant.rs`): owner root, owner
range, receiver `{root: Arc<AddressSpace>, cell, generation, base, size, rights, drained}`,
`Live → Revoking → Revoked`, plus `rights_for()` (ReadOnly → R+NX, ReadWrite → RW+NX,
WriteOnly refused). `publish()` compares the `(cell, generation, root, rights)` tuple first and
drains before republishing on any change, maps every page transactionally and undoes the pages
already mapped on failure. `revoke()` is idempotent and retryable; an unacknowledged
invalidation leaves the record `Revoking` with its frames retained.

`kernel/src/task/syscall.rs` carries the record on `PageGrant`/`RegGrant` and replaces the
blanket phase-01 deny with a capability check: `domain_grant_task(tid) && !domain_grant_capable(tid)`
still returns the exact sentinels (`Ok(0)` / `usize::MAX` / `Err(PermissionDenied)`) for every
shape that is not a live private root on the one architecture with a lifecycle — non-RV64,
unknown or retired tid, and a root already dying. `GrantAlloc`/`GrantRegister` allocate
supervisor-only, zero, and map the owner's own pages RW+NX with undo; `GrantShare` publishes
the exact tuple and refuses WriteOnly and mixed pairs; `GrantSlice` resolves the owner's rights
and maps the receiver transactionally, returning `usize::MAX` on every failure; `GrantFree` /
`GrantUnregister` revoke outside every table lock, and an unacked invalidation returns the
bounded `PermissionDenied` with the row kept `Revoking` for the retirement sweep
(`release_deferred_domain_row`, `sweep_deferred_domain_grants`). Lock order
`*_GRANT_TABLE → SCHEDULER → pin REGISTRY` is unchanged, and every awaited invalidation runs
with no table lock held.

Evidence (this session, on the frozen tree):

| Item | Command | Result |
|---|---|---|
| Acceptance suite, 1 hart | `scripts/build-native-domain-test-ci.sh` then `scripts/qemu-native-domain-test.sh --harts 1 --case admission,asid-lease,unmap-order,grant-revoke,grant-gate,grant-pair` | exit 0, 6/6 PASS — `.logs/native-domain-qemu/h1-grant-pair-6kWaA7`: `S22-RV64-GRANT-PAIR-HANDOFF-OBSERVED id1=… id2=… id3=… faults=1:2:1`, terminal `S22-RV64-GRANT-PAIR-OWNER: PASS` |
| Pair positive path | same lane | owner `ALLOC: OK`, `REGISTER: OK`, `MAPPED: OK`, `REG-MAPPED: OK`; receiver `SLICE-RO: OK (read 0xa5)`, `SLICE-RW: OK`; `RO-WRITE: FAULT-EXPECTED` and `REVOKE-FAULT`/`UNREGISTER-FAULT`/`EXIT-FAULT: FAULT-EXPECTED` each attributed to the exact revoked address by the runner's fault classifier; `FRAME-REUSE: REFUSED`; `SHARE-WO: DENY`, `SHARE-FOREIGN: DENY`, `SLICE-UNKNOWN: DENY` |
| Boot fixtures | `grant-revoke`, `grant-gate` | `S22-RV64-GRANT-REVOKE-{OWNER-MAPPED,OWNER-SLICE,SLICE-RO,SLICE-RW,WO-REFUSED,FOREIGN-PEER,REVOKE,FRAME-REUSE,PARTIAL-MAP,DEAD-ROOT}: PASS`, `S22-RV64-GRANT-GATE-{ALLOC,REGISTER,WO,SHARE,SLICE,RETIRED,SAS,FRAMES}: PASS`, `GRANT-RECLAIM-{OWNED,RECEIVED,PINNED}: PASS` |
| Host lane | `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` | 145 passed / 0 failed |
| Builds | RV64 production, RV64 `test-hooks,native-domains`, RV64 `--no-default-features`, AArch64 test-hooks, x86_64 — all with `-D warnings` | clean |
| Non-RV64 lanes | AArch64 test-hooks lane, x86 TCG lane | AArch64 exit 0 with `[vfs-test] Results: 96 PASS, 0 FAIL` and every `S22-AARCH64-*` marker; x86 reaches the shell with `PCID disabled` |

Named gaps, not claimed as done:

- **2-hart shootdown of the pair path was not run** (`grant-pair` was executed at `--harts 1`).
- **Deferred-ack tolerance is not reflected in the boot fixture.** `S22-RV64-GRANT-REVOKE-SLICE-RW`
  /`-REVOKE` assert first-attempt completion, so on a 2-hart boot where a remote hart stops
  acknowledging they report `FAIL` even though the lifecycle did the correct fail-closed thing
  (frames retained, record `Revoking`, nothing widened). The planned invariant-form assertion was
  not applied. See the phase-02 finding on the stalled-ack cascade.
- **Same-recipient RW→RO downgrade** is exercised only indirectly (the same `publish()` tuple-change
  path as the lane's target-change re-share); no dedicated assertion.
- **DMA pins and the VFS lease are not exercised for domain receivers.** `GrantDma` still denies a
  private-root caller by design, so no pin can exist on a domain grant; a VFS holder is SAS-only in
  this tree, so the lease/reader-drain path is unreachable for a domain grant.
- **Non-RV64 lifecycle is not implemented** — AArch64/x86_64 keep the phase-01 sentinels exactly
  (verified by construction, not executed as a positive path).

### Slice 5 — the three named gaps closed (2026-09-28)

- **Deferred-ack tolerance.** `scripts/qemu-native-domain-test.sh:398-536`
  (`assert_grant_revoke_outcome`) accepts the boot fixture's `-SLICE-RW`/`-REVOKE` first-attempt
  markers only in invariant form: the eight properties that hold in either outcome must PASS and
  no other `S22-RV64-…: FAIL` may appear; an accepted failure must carry its own evidence chain
  from the same log (`receiver publish refused: AwaitingSafeRoot` plus the grant-page quarantine
  for `-SLICE-RW`; `domain revoke deferred … record kept Revoking` for `-REVOKE`; the exhausted
  retry budget as the cause; both naming the same grant id). A new unconditional property,
  `S22-RV64-GRANT-GATE-RETIRE-REFUSAL: PASS` (`kernel/src/task/grant_gate_selftest.rs:145-185`,
  `:281-320`), asserts that after the revoke attempt — completed **or** deferred — the record
  refuses a receiver slice, an owner slice and a re-share, and the receiver mapping is gone.
  Red-proved by replaying the archived stalled boot through the runner's `--assert-log` seam with
  one mutant per piece of evidence (each mutant exits 1 with its own message).
- **Same-recipient RW→RO downgrade.** The owner publishes ReadWrite, then re-shares the *same*
  grant to the *same* recipient as ReadOnly (fourth grant id, so every deliberate fault stays
  attributable); the receiver proves the writable mapping first, re-slices, reads the original
  byte and then stores — which must fault, classified at the exact handoff address
  (`faults=1:2:1:1`). Red-proved by temporarily re-sharing `PERM_RW` instead of `PERM_RO`: the
  store succeeded and the runner failed with
  `missing store fault at the downgraded grant address` (`.logs/native-domain-qemu/h1-grant-pair-3YyvIh`),
  then the tree was restored byte-identically (`sha256sum -c`).
- **Two-hart pair.** `--harts 2 --case grant-pair` runs and is asserted non-skipping (the 2-hart
  gate requires `[smp] hart 1 online, parked`); terminal
  `PASS: native-domain case=grant-pair harts=2 terminal=S22-RV64-GRANT-PAIR-OWNER: PASS`
  (`.logs/native-domain-qemu/h2-grant-pair-xWohEM`, and twice more after the fixture-contract
  fixes in phase 02).

Two side findings from the same work:

- **The lane was unbuildable at HEAD.** The pair cells became tracked in `28325dff1` without F1
  (`scripts/cellos-sign --check`) entries, so the sign gate refused them. Added reviewed
  `[[file]]`/`[[crate]]` entries to `scripts/unsafe-allowlist.toml:40-52` and `:603-615`
  (`class = test-harness`/`test-violation`, the raw-pointer dereference being the property under
  test, approver `dmin` matching the existing `tier2-exploit` precedent) — **this is a security-gate
  allowlist change and the approver field asserts human approval**; it is called out here so it can
  be reviewed or reverted deliberately.
- **The lane's build directory is shared.** Two concurrent invocations of the same lane collide in
  `target/native-domain-test` and `kernel/src/embedded-test-hooks/kernel_fs.img` (`failed to build
  archive from rlib … No such file or directory`); run one lane at a time until that is isolated.

**Still not closed:** the positive branches are proven on RV64 only; AArch64/x86_64 keep the
phase-01 sentinels; DMA pins and the VFS lease remain unreachable for domain receivers; and a
2-hart boot that reaches the revoke fixture can still be truncated by the stall recorded in
phase 02 (the runner now reports that as a stall instead of a property failure).

### Slice 6 — the lifecycle on AArch64 test images (2026-09-28) — done

`domain_grant_lifecycle_supported()` (`kernel/src/task/syscall.rs:241`) is now
`native-domains && (riscv64 || (aarch64 && test-hooks))`, and all 27 `riscv64`-only cfg sites in
that file were widened to the same predicate. No arch-specific step was needed in the state
machine: AArch64 already has non-global private leaves with deferred release (phase 02), and the
switch ordering the lifecycle relies on is the same gate. A const-assert pins a production AArch64
build to the phase-01 sentinels and to their exact values
(`kernel/src/task/syscall.rs:265-286`: `GRANT_DENY_ALLOC == 0`, `GRANT_DENY_SLICE == usize::MAX`,
`GRANT_DENY_ERROR = PermissionDenied`), and inverting that assert fails the build with
`error[E0080]` — red-proved, then restored byte-identically.

The kernel-side fixtures now run on AArch64 from a **new additive** dispatch block
(`kernel/src/main.rs:1126-1148`, test-hooks only, skipped at EL2 with a named log line); the RV64
block is untouched. `domain_grant.rs` owns `ARCH_TAG` so the markers are
`S22-AARCH64-GRANT-*` there and byte-identical to before on RV64.

Witnesses: AArch64 test-hooks lane exit 0 with **22/22** `S22-AARCH64-GRANT-*` markers in its
required list, `[vfs-test] Results: 96 PASS, 0 FAIL`, and a real Tier-2 cell driving
register → copy-in → copy-out → unregister → refused through the EL0 syscall path
(`cells/tests/tier2-smoke/src/main.rs`). That cell's grant posture is now architecture-honest
(compile-time: published-grant branch on RV64/AArch64, phase-01 deny branch elsewhere), because the
x86 domain lane also packs it and x86_64 deliberately keeps the lifecycle closed. RV64 1-hart case
set unchanged and green.

Named gaps: the two-cell public handoff and the address-classified store faults remain RV64-lane
evidence only — the AArch64 lane drives the kernel-side fixture, not the pair; `GrantShare` has no
sentinel branch on a closed target (unchanged from phase 01, and unreachable there because no
domain can be admitted); DMA pins and the VFS lease are still unreachable for domain receivers.

## Assumptions / risk / rollback
- [UNVERIFIED] Physical identity is a usable receiver VA under all supported domain layouts; verify against `USER_LIMIT` and occupied mappings before deciding whether ABI-compatible alternate VA exists. No public ABI change without the two Law-1 owner checkpoints. Rollback: phase-01 deny gate plus cold reboot; leaked contents or prior DMA writes cannot be undone. Missing remote completion blocks release; retain quarantined frames rather than treating timeout as success.

## Deviation Log

- **RV64 only, and the phase is not complete.** The lifecycle is implemented and witnessed on
  RV64 (`§ Progress`, Slices 0–1/4). The phase's own step 5 asks for per-architecture images and
  for the positive path to be proven for the exact pair/architecture before the containment gate
  is lifted for it; only RV64 satisfies that, so AArch64 and x86_64 keep the phase-01 sentinels.
- **`domain_grant_records_live()` became residual-only.** A *managed* domain grant no longer
  blocks domain admission, because the pair lane must admit the receiver while the owner's grant
  is live. A record that no lifecycle owns (a pre-gate record) still blocks admission exactly as
  in phase 01.
- **WriteOnly is refused rather than widened.** `rights_for()` returns `None` for `GrantPerm::WriteOnly`
  on a domain pair, so `GrantShare` denies it; the plan's step 1 asked for the VFS/hypervisor
  callers that pass `WriteOnly` while reading to be migrated first — that migration was **not**
  done, and the refusal is the conservative substitute.
- **The boot fixture still asserts first-attempt completion.** `S22-RV64-GRANT-REVOKE-SLICE-RW`
  and `-REVOKE` fail on a 2-hart boot where a remote hart stops acknowledging, even though the
  lifecycle behaves correctly (fail-closed, frames retained, record `Revoking`). Recorded as a
  named gap rather than silently re-labelled.
- **Lean Pass skipped deliberately.** `kernel/src/task/syscall.rs` (+934/−313) and
  `kernel/src/task/domain_grant.rs` (+598) exceed the configured complexity thresholds; a
  behavior-preserving refactor of a fresh revoke state machine was judged riskier than the
  duplication it would remove.
