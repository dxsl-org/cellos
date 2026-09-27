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
| 02–08 | | pending | — |

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
