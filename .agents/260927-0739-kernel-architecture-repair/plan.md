---
title: "Kernel architecture repair — grants, domains, snapshot and memory"
description: "Close verified grant/domain isolation gaps, restore correctness, and qualify boot/runtime behavior without changing the public ABI."
status: in-progress
priority: P1
effort: "phased; no fixed estimate until target runtime matrix is available"
branch: main
tags: [bugfix, critical, tech-debt]
blockedBy: []
blocks: []
created: 2026-09-27
---

# Kernel architecture repair plan

## Scope and disposition

Repair **all** findings of the 2026-09-27 audit: domain grant revoke/permission/map and page-table lifetime; non-RV64 switch completion; x86 PCID; snapshot integrity/layout/quiescence; GrantAlloc owner mapping; boot contiguous heap; single-region allocator waste; RV64 RT sender wake. This plan changes no `libs/api`/`libs/types` ABI, new kernel service, or Tier-1 SAS fast path. Spec 15 and Spec 22 are acceptance contracts, not evidence that code already satisfies them. `scout-report.md` records code citations and precedent footprints.

**Immediate safety posture:** refuse unsafe domain grants until phase 03 passes, refuse AArch64/x86_64 Tier-2 admission until phase 02 proves switch ordering on one CPU, and return snapshot unavailable until phase 07 proves save→reboot→restore. RV64 copied IPC, Tier-1 SAS grants and Tier-3 stay available. Do not misrepresent a disabled feature as a completed fix. Rollback: boot a known-safe image with Tier-2 admission **disabled**, reboot (not live downgrade); never reuse pre-fix snapshots. Fleet admission already off (`kernel/src/main.rs:1016-1045`), but dev default includes `native-domains` (`kernel/Cargo.toml:69`). Validate image-specific posture.

## Phases

| # | Deliverable | Dependency | Gate |
|---|---|---|---|
| 01 | [Contain risky paths and capture regressions](./phase-01-containment-baselines.md) | — | Deny unsafe domain grants, non-RV64 Tier 2 and snapshot; no SAS regression |
| 02 | [Correct domain PTE reclaim and architecture switch](./phase-02-domain-root-lifetime.md) | 01 | Shootdown before reuse, PCID fallback, safe-root ack |
| 03 | [Rebuild explicit domain grant lifecycle](./phase-03-domain-grant-lifecycle.md) | 02 | Rights, transactional mapping, revoke/death/races pass |
| 04 | [Guarantee heap contiguity](./phase-04-boot-heap.md) | 01 | Fragmented-map heap remains valid/fails safely |
| 05 | [Manage all usable RAM regions](./phase-05-multiregion-frames.md) | 04 | No reserved frames used; quota/metrics correct |
| 06 | [Restore immediate RT wake behavior](./phase-06-rt-wake.md) | 01 | Two-hart target IPI and latency bounded |
| 07 | [Repair and qualify warm snapshot](./phase-07-warm-snapshot.md) | 03, 05, 06 | Exact storage replay, integrity, quiescence and restart |
| 08 | [Cross-architecture qualification and documentation](./phase-08-qualification.md) | 02–07 | Fresh full matrix; no premature production claim |

## Status

| # | Phase | Status | Evidence |
|---|---|---|---|
| 01 | Containment baselines | completed 2026-09-27 | `phase-01-containment-baselines.md` § Evidence — failing-before witness red (`h1-grant-gate-6j4WCL`) → green (`h1-grant-gate-MbNs8s`), 7/7 RV64 regression cases, AArch64 denial lane 2/2, `launch-profile` snapshot contract 1/1, off-feature build clean |
| 02 | Domain root and TLB correctness | **complete for test images on all three architectures** — slices 1–7 done; every production image stays refused by policy | `phase-02-domain-root-lifetime.md` § Progress — tag leases (`h1-asid-lease-mvFocf`), unmap invalidation order (`h1-unmap-order-HDEPLP`), incoming completion hook (`S22-AARCH64-ROOT-SWITCH … safe_root_consumed=true`, AArch64 lane exit 0, vfs 96/96), tag-targeted acknowledged frame release (host 145/145), x86 PCID gated on a correct INVPCID probe (TCG `PCID disabled`, KVM `PCID enabled … invpcid=true`, `hal-x86` 12/12). Still closed: AArch64/x86_64 Tier-2 admission, the x86 `INVPCID` instruction path, and and the AArch64 non-global-leaf slice; **a real Tier-2 domain entry now runs on one AArch64 PE in the test-hooks image** (`S22-AARCH64-DOMAIN-LIVE: PASS asid=1 root=0x418cd000 ttbr0=0x10000418cd000 domain=13 generation=14`, teardown `releases=1 quarantined=0`, contained fault, vfs 96/96), with production admission pinned by a compile-time assert and still refused. The synchronous-ack stall class is removed by the deferred-release reaper (`S22-RV64-DEFERRED-RELEASE: PASS`; the 2-hart suite passed twice with zero truncations). Still unproven: the ASID-scoping behaviour (QEMU 8.2.2 does not scope `aside1is`) and *production* runtime denial of a real domain artifact |
| 06 | RT sender wake | completed 2026-09-27 (decision-level) | `phase-06-rt-wake.md` § Progress — `S22-RV64-RT-WAKE` red with the wake call reverted (`h2-rt-wake-xiIlSU`: `pended=false`) and with both hunks reverted (`h2-rt-wake-3qm6mA`), green after (`h2-rt-wake-jDuHmU`); latency P99 stays hardware-gated |
| 04 | Boot heap contiguity | completed 2026-09-27 | `phase-04-boot-heap.md` § Progress — host lane 113/113 (fragmented map red before the fix at frame 50), RV64 + AArch64 + RV64-production boots green; x86_64 lane unavailable |
| 05 | Multi-region frames | completed 2026-09-27 (algorithm; capacity claim board-gated) | `phase-05-multiregion-frames.md` § Progress — host lane 118/118 (multi-range plan red before the fix), RV64 + AArch64 + RV64-production boots green |
| 07 | Warm snapshot | in progress — format/state-machine, mutable-image inventory, a real RV64 park hook, freeze-stable staging on a **reserved scratch workspace** (308 frames, no allocation while frozen) and authenticated freshness done; the board witness remains, feature still disabled | `phase-07-warm-snapshot.md` § Progress — internal format v2 with an explicit address inventory and one canonical checksum, `EMPTY→WRITING→COMMITTED→CONSUMING→CONSUMED`, fake-block corruption/reset matrix: host lane 145/145 (26 new snapshot tests), red witness in a throwaway transcription of the legacy pair; all-hart quiescence is implemented as a protocol with a fail-closed park-hook seam (`kernel/src/task/quiesce.rs`, refused before any wait while the hook is absent) and host tests: 155 passed in total. Closure, coherent capture staging, authenticated freshness, the real park hook and the board witness remain, and `QUALIFICATION_ENABLED` is untouched |
| 03 | Grant lifecycle | in progress — RV64 and AArch64 test images, kernel fixture plus the two-cell pair on both (AArch64 via an in-band id handoff); DMA/VFS interaction remains | `phase-03-domain-grant-lifecycle.md` § Progress — `scripts/qemu-native-domain-test.sh --harts 1 --case admission,asid-lease,unmap-order,grant-revoke,grant-gate,grant-pair` exit 0, 6/6 PASS (`h1-grant-pair-6kWaA7`): owner `ALLOC/REGISTER/MAPPED/REG-MAPPED: OK`, receiver `SLICE-RO/SLICE-RW: OK`, `RO-WRITE`/`REVOKE-FAULT`/`UNREGISTER-FAULT`/`EXIT-FAULT: FAULT-EXPECTED` attributed by address, `FRAME-REUSE: REFUSED`; every non-capable shape keeps the phase-01 sentinels byte-for-byte (AArch64/x86_64, retired roots, unsupported rights). Lane gaps closed 2026-09-28: deferred-ack tolerance is asserted in invariant form with red-proved mutants (`--assert-log`), same-recipient RW→RO downgrade is proven by an address-classified store fault (`faults=1:2:1:1`), and `--harts 2 --case grant-pair` passes non-skipping. Still open: DMA/VFS interaction, non-RV64 lifecycle, and the phase-02 stall that can truncate a 2-hart revoke boot |
| 08 | Cross-architecture qualification | in progress — matrix recorded in `phase-08-qualification.md` § Progress (24 rows; AArch64 production suite 11/11 incl. a `-smp 2` row that requires the second hart online, the IPI answered and a task dispatched to it; the two-hart test-hooks lane is 14 of 14 since the SGI/preemption split) | Host, RV64 (incl. the two integration lanes, now runnable on Linux via `scripts/gen-disk-ci.sh`), AArch64 and x86 lanes all green on the current tree; hardware-gated rows named rather than approximated; x86-only integration lanes and the physical-board rows remain |

## Remaining work and its prerequisites

Recorded so the next session starts from evidence rather than from the phase titles.
Last synced 2026-09-28 (evening).

| Item | State / prerequisite that does not exist yet |
|---|---|
| 02 — test-image domain entry, all three architectures | **Done.** RV64 (always), AArch64 and x86_64 (test images only, production const-asserted closed). Each lane witnesses admission, a live root register read from inside the domain's own kernel context (`S22-AARCH64-DOMAIN-LIVE`, `S22-X86-DOMAIN-LIVE`), a contained fault, teardown with `quarantined=0`, and (x86) shell recovery. |
| 02 — ASID-scoping behaviour of `tlbi aside1is` | **UNPROVEN in emulation.** The in-tree witness (`S22-AARCH64-ASID-INVALIDATION`) reports `UNPROVEN` because QEMU 8.2.2 retires unrelated ASIDs — proved by the fixture's own control. Needs hardware or an ASID-faithful emulator; the non-global-leaf composition itself is witnessed at the encoder (`S22-AARCH64-LEAF-NONG`). |
| 02 — non-RV64 SMP (sized, not started) | Confirmed by inspection: `kernel/src/task/smp.rs:384` makes `start_secondaries()` a no-op off RV64; there is **no PSCI client** anywhere under `hal/arch/arm` (only a comment about a "spin-table or PSCI-based wake loop", `hal/arch/arm/src/aarch64/boot.rs:159`); and `hart_local`'s `current_hart()`/`current_hart_id()` still hard-code slot 0 off RV64 (`kernel/src/task/hart_local.rs:293-356`). Slices, in dependency order: **(a)** per-CPU hart identity (MPIDR_EL1 on AArch64) behind a per-CPU data block, replacing the slot-0 lookup; **(b)** secondary bring-up (PSCI `CPU_ON` over HVC or the QEMU `virt` spin-table) where the secondary runs on its own stack with the shared root already active and parks in a bounded loop — QEMU `virt` supports both, so this slice is witnessable in the AArch64 lane (`-smp 2`); **(c)** IPI (GIC SGI) plus the epoch/remote-ack machinery mirroring RV64's, whose absence is why `enable_for_boot` refuses multi-CPU today; **(d)** only then may `switch_ordering_qualified` admit multi-CPU on AArch64, and non-RV64 Tier-2 stays single-CPU until it does. |
| 03 — pair-level evidence on AArch64, the two-hart stream, DMA/VFS interaction | The two-cell pair runs on AArch64 as well, over the in-band id handoff (owner `ALLOC/REGISTER/MAPPED/REG-MAPPED`, receiver `SLICE-RO/SLICE-RW`, and the four address-classified faults), deterministic at one hart and **14 of 14** at `QEMU_SMP=2` since the SGI/preemption split (`d2dbf8f00`, cause and readings in `phase-08-qualification.md` § Progress). A revocation is asynchronous, so both fixtures retry their store until it traps (`e918e468c`). DMA pins and the VFS lease stay unreachable for domain receivers (`GrantDma` denies a private-root caller by design). |
| 07 — what capture still cannot prove | The inventory covers the mutable kernel-image span, the park hook is real, the capture stages on its own reserved 308-frame workspace (nothing allocates while frozen), and a replayed/older/tampered image is refused by an authenticated epoch. What remains: allocator metadata inside those frames is *rebuilt* rather than restored, the device epoch has no real monotonic source (a board MMC/eMMC), and the save→reset→restore→resume witness does not exist. `QUALIFICATION_ENABLED` still keeps every shipping image from capturing. |
| 08 — matrix and hardware rows | Every lane runnable here is green on the current tree, including the AArch64 two-hart lane (`QEMU_SMP=2`, 14 of 14 after `d2dbf8f00`; it defaults to one hart, where it is deterministic), and tabulated in `phase-08-qualification.md` § Progress. The `tests/integration` lanes need a `disk_v3.img` carrying its cell-table bootstrap section (this checkout has none), and MMC/remote-TLB/measured-latency rows are hardware-gated and named as such. |
| Housekeeping | `scripts/unsafe-allowlist.toml` gained F1 entries for the Tier-2 grant pair (approver field asserts human approval) — review or revert deliberately. The RV64 lanes still share `kernel/src/embedded-test-hooks` and `target/native-domain-test`, so two concurrent invocations collide; the AArch64 and x86 lanes now have their own embedded directories. |

## Dependency / release policy

Phases 02 and 04 can run independently after 01; 06 is independent after 01. Phase 03 needs safe PTE reclamation and switch completion first; snapshot must use settled memory accounting/layout from 05, final RT sender wake behavior from 06, and a safe all-hart quiescence protocol from 02–03. Serialize scheduler/SMP edits in phases 06/07; test sender-consume wake racing freeze. Complete and verify each phase on its own image before merging or enabling its path. A failed high-risk gate returns to phase-01 fail-closed posture, **not** SAS fallback; software rollback cannot undo frames/data already exposed or a corrupt snapshot already restored. A fresh disk/cold boot and security incident review are required if that happened.

## Verification levels

Host model/unit tests validate edge cases. Fresh RV64 QEMU 1/2-hart and AArch64/x86 **single-CPU** QEMU exercise actual trap, domain and fault paths. Non-RV64 SMP is a named qualification blocker until per-CPU `hart_local`, IPI and remote shootdown are implemented (`kernel/src/task/hart_local.rs:293-356`). PCID-off and precise cross-hart shootdown need explicit witnesses. Board MMC/remote-TLB and hard RT latency are **hardware-gated**; QEMU output alone cannot promote production or physical claims. Record build tuple, SHA, board, CPU features, test log and pass/fail; include all applicable Spec 22 negative cases. No code changes have been made in this planning task.

## Assumptions

- [UNVERIFIED] A safe stop-the-world snapshot transport and recovery path can be made on a board with MMC without changing public ABI. Verify device and reset sequence before enabling `Snapshot`.
- [UNVERIFIED] Existing default development image actually exercises multi-arch Tier 2 grant syscalls; prove on fresh target-specific images, not based on build flags.
- [UNVERIFIED] Capacity and physical RT targets have a qualified exact-device lab. Missing hardware keeps those acceptance claims blocked, not silently passed.

## Red Team Review

Four independent hostile lenses found and the plan incorporated: grant-denial ABI sentinel and real two-Cell harness; non-RV64 pre-save root switch and absent multi-CPU hart identity; shared-ASID live-tag rollover; same-recipient rights downgrade; snapshot mutable kernel roots, capture-side mutation, durable commit/consume and rollback freshness; malformed-map fallback overlapping x86 kernel; snapshot/RT phase dependency. Risk remaining: physical MMC and remote-TLB evidence is hardware-gated, so affected profiles stay disabled. No finding was marked fixed in implementation.

## Validation Log

- Code-confirmed: `GrantAlloc`/`GrantRegister` wrapper accepts every nonzero return (`libs/ostd/src/syscall.rs:1766-1775,1960-1967`); source denies must return `0`. Domain root activation precedes non-RV64 `Context::switch` (`kernel/src/task.rs:1358-1365`); their hart-local lookup uses slot zero (`task/hart_local.rs:293-356`). ASID wraps at 16-bit (`memory/address_space.rs:837-855`); snapshot writer/reader disagree on CRC and sparse layout (`kernel/src/snapshot.rs:104-166,268-324`); x86 fallback marks live kernel range usable (`kernel/src/boot.rs:699-720`). Files, links, dependency ordering and per-phase acceptance/rollback were checked locally; no implementation behavior was run for this plan.
- Unverified: physical MMC save/restore, x86 PCID-on/off runnable image, remote multi-PE TLB completion, device-backed monotonic snapshot freshness, measured RT latency. Their **claim/profile gates remain closed** until exact runtime witnesses exist. No owner decision is needed to preserve the existing ABI and conservative fail-closed posture; any discovered ABI change triggers Law-1 checkpoints before work.
