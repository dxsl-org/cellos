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
| 02 | Domain root and TLB correctness | in progress — slices 1–3 and the x86 runtime gate done; non-RV64 admission still refused | `phase-02-domain-root-lifetime.md` § Progress — tag leases (`h1-asid-lease-mvFocf`), unmap invalidation order (`h1-unmap-order-HDEPLP`), incoming completion hook (`S22-AARCH64-ROOT-SWITCH … safe_root_consumed=true`, AArch64 lane exit 0, vfs 96/96), tag-targeted acknowledged frame release (host 145/145), x86 PCID gated on a correct INVPCID probe (TCG `PCID disabled`, KVM `PCID enabled … invpcid=true`, `hal-x86` 12/12). Still closed: AArch64/x86_64 Tier-2 admission, the x86 `INVPCID` instruction path, and the AArch64 non-global private-leaf requirement recorded in the phase's Finding |
| 06 | RT sender wake | completed 2026-09-27 (decision-level) | `phase-06-rt-wake.md` § Progress — `S22-RV64-RT-WAKE` red with the wake call reverted (`h2-rt-wake-xiIlSU`: `pended=false`) and with both hunks reverted (`h2-rt-wake-3qm6mA`), green after (`h2-rt-wake-jDuHmU`); latency P99 stays hardware-gated |
| 04 | Boot heap contiguity | completed 2026-09-27 | `phase-04-boot-heap.md` § Progress — host lane 113/113 (fragmented map red before the fix at frame 50), RV64 + AArch64 + RV64-production boots green; x86_64 lane unavailable |
| 05 | Multi-region frames | completed 2026-09-27 (algorithm; capacity claim board-gated) | `phase-05-multiregion-frames.md` § Progress — host lane 118/118 (multi-range plan red before the fix), RV64 + AArch64 + RV64-production boots green |
| 07 | Warm snapshot | in progress — format/state-machine half done, feature still disabled | `phase-07-warm-snapshot.md` § Progress — internal format v2 with an explicit address inventory and one canonical checksum, `EMPTY→WRITING→COMMITTED→CONSUMING→CONSUMED`, fake-block corruption/reset matrix: host lane 145/145 (26 new snapshot tests), red witness in a throwaway transcription of the legacy pair; quiescence, closure, freshness and the board witness remain, and `QUALIFICATION_ENABLED` is untouched |
| 03 | Grant lifecycle | in progress — RV64 lifecycle implemented and green; 2-hart pair, downgrade assertion and deferred-ack tolerance open | `phase-03-domain-grant-lifecycle.md` § Progress — `scripts/qemu-native-domain-test.sh --harts 1 --case admission,asid-lease,unmap-order,grant-revoke,grant-gate,grant-pair` exit 0, 6/6 PASS (`h1-grant-pair-6kWaA7`): owner `ALLOC/REGISTER/MAPPED/REG-MAPPED: OK`, receiver `SLICE-RO/SLICE-RW: OK`, `RO-WRITE`/`REVOKE-FAULT`/`UNREGISTER-FAULT`/`EXIT-FAULT: FAULT-EXPECTED` attributed by address, `FRAME-REUSE: REFUSED`; every non-capable shape keeps the phase-01 sentinels byte-for-byte (AArch64/x86_64, retired roots, unsupported rights). Named gaps: 2-hart pair shootdown, deferred-ack tolerance in the boot fixture, same-recipient RW→RO assertion, DMA/VFS interaction, non-RV64 lifecycle |
| 08 | Cross-architecture qualification | pending | — |

## Remaining work and its prerequisites

Recorded so the next session starts from evidence rather than from the phase titles.
Last synced 2026-09-28.

| Item | State / prerequisite that does not exist yet |
|---|---|
| 02 slice 3 — non-RV64 ordered switch + invalidation ack | **Done except the admission proof.** Ordering (`4f17b915b`, `36ef90516`), the incoming completion hook (`kernel/src/task.rs:918-928`, witnessed on the AArch64 lane as `S22-AARCH64-ROOT-SWITCH: PASS … safe_root_consumed=true`), the generation-tagged acknowledgement (`b783ca63e`) and the tag-targeted acknowledged frame release (host 145/145; `flush_asid_and_await` gates every unmap and `AddressSpace::drop` releases the tag before any frame) are in place. What is still missing is any **non-RV64 Tier-2 admission**: no AArch64/x86_64 image has entered a real domain task, so `switch_ordering_qualified()` stays false off RV64. |
| 02 — AArch64 non-global private leaves (new blocker) | `hal/arch/arm/src/aarch64/paging.rs:182` never sets `PTE_nG`, so private-root leaves are global: ASID-targeted `tlbi aside1is` cannot reach them (hence the `vmalle1is` stopgap in the release path) and a stale entry of one private root stays usable under another ASID. Required before reopening AArch64 admission: a `PageFlags` bit translated to `PTE_nG` for the domain builder's mappings, shared kernel ranges staying global, then re-run the phase-02 witnesses. |
| 02 slice 4 — x86 PCID/INVPCID runtime gate | **Done and witnessed on the current tree.** The probe now reads `CPUID.07H:EBX[10]` (the previous `CPUID.01H:ECX[12]` is FMA and reported INVPCID present almost everywhere); a nonzero PCID requires PCID *and* INVPCID, `CR4.PCIDE` is only set with an untagged CR3, and a firmware-set PCIDE is cleared after selecting tag 0. TCG `X86_EXPECT_PCID=0` → shell + `PCID disabled (CPUID pcid=false invpcid=false, CR4.PCIDE=0, CR3=0x59000 …)`; KVM `X86_EXPECT_PCID=1` → shell + `PCID enabled (CPUID pcid=true invpcid=true, CR4.PCIDE=1, CR3=0x59000)`; `hal-x86` host lane 12/12. **Not executed**: the `INVPCID` instruction path itself (no x86 domain can be admitted while admission is closed). |
| 02 — awaited-flush budget under load | The RV64 2-hart multi-case boot timed out a release (`[tlb] asid invalidation unacknowledged on hart 1 (attempt 25)` → tag retained → `S22-RV64-GRANT-REVOKE: FAIL`, `.logs/native-domain-qemu/h2-user-copy-race-p7nYhU`) while the same case passes alone. Fail-closed and correct, but every unmap now pays the 25 × 200 ms budget. Phase 03's revoke must defer release to a reaper instead of widening the bound. |
| 02 — 2-hart `asid-lease` flake | `--harts 2 --case asid-lease` panicked at the trusted-init publication stage (`ATOMIC_PUBLICATION_AP-15: FAIL` → `cases.rs:149` → `[KERNEL PANIC]`) in 2 of 3 boots (`.logs/native-domain-qemu/h2-asid-lease-xoLVBw`), with 42 older lane logs already containing panics. Attribution open; the reproducing command is recorded in the phase file. |
| 03 — grant lifecycle | **Lane exists, gate still closed.** A real Tier-2 owner/receiver pair (`cells/tests/tier2-grant-owner`, `tier2-grant-receiver`) is driven non-skipping through the public syscall path by `scripts/qemu-native-domain-test.sh --case grant-pair`, and every phase-01 deny sentinel is asserted verbatim (`h1-grant-pair-WpVt7F`). The lifecycle itself (owner mapping, rights-accurate receiver mapping, transactional publish, revoke with acknowledgement, death paths) is the remaining work; the revoke must not block on the 5-second retry budget. |
| 07 — warm snapshot | **Format/state-machine half done, feature disabled.** Internal format v2 with an explicit address inventory, one canonical checksum, `EMPTY→WRITING→COMMITTED→CONSUMING→CONSUMED`, capacity/identity preflight and a test-only fake block device; host lane 145/145 with 26 new tests and a red witness from a throwaway transcription of the legacy pair (`phase-07-warm-snapshot.md` § Progress). Still open: all-hart quiescence before capture, closure completeness (mutable kernel-image roots, allocator metadata, page tables, task records), coherent capture staging, authenticated monotonic freshness, the real save→reset→restore→resume witness on a block-capable board, and the `docs/specs/03-runtime.md` reconciliation. `QUALIFICATION_ENABLED` is untouched. |
| 08 — cross-architecture qualification | Depends on 02–07. The x86 TCG and KVM lanes and the AArch64 one-PE lane are runnable here; the physical-board witnesses (MMC, remote TLB, measured RT latency) and the x86 domain lane are not. |

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
