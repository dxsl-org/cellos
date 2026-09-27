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
| 02 | Domain root and TLB correctness | in progress — slices 1–2 done, AArch64 lane repaired | `phase-02-domain-root-lifetime.md` § Progress — tag leases (`h1-asid-lease-mvFocf`) and unmap invalidation order (`h1-unmap-order-HDEPLP`) each red before / green after; AArch64 test-hooks lane builds and boots again; non-RV64 switch, invalidation ack and x86 PCID remain |
| 06 | RT sender wake | completed 2026-09-27 (decision-level) | `phase-06-rt-wake.md` § Progress — `S22-RV64-RT-WAKE` red with the wake call reverted (`h2-rt-wake-xiIlSU`: `pended=false`) and with both hunks reverted (`h2-rt-wake-3qm6mA`), green after (`h2-rt-wake-jDuHmU`); latency P99 stays hardware-gated |
| 04 | Boot heap contiguity | completed 2026-09-27 | `phase-04-boot-heap.md` § Progress — host lane 113/113 (fragmented map red before the fix at frame 50), RV64 + AArch64 + RV64-production boots green; x86_64 lane unavailable |
| 05 | Multi-region frames | completed 2026-09-27 (algorithm; capacity claim board-gated) | `phase-05-multiregion-frames.md` § Progress — host lane 118/118 (multi-range plan red before the fix), RV64 + AArch64 + RV64-production boots green |
| 03, 07, 08 | | pending | — |

## Remaining work and its prerequisites

Recorded so the next session starts from evidence rather than from the phase titles.

| Item | Prerequisite that does not exist yet |
|---|---|
| 02 slice 3 — non-RV64 ordered switch + invalidation ack | (a) AArch64/x86 `Context::switch` is a **single assembly routine** (`hal/arch/arm/src/aarch64/context.rs:129` `__switch_el1`, same shape on x86): saving the outgoing context before activating the incoming root means splitting it into save/load halves, which changes every switch on the target. (b) ~~No AArch64 domain fixture exists~~ **Resolved 2026-09-27**: `domain_switch_tests` now runs on AArch64 and the test-hooks lane asserts its markers, so plan derivation, identity publication and the pin→plan window are witnessed there; what remains unwitnessed is a real switch between two private roots. (c) `flush_range` issues the RFENCE and panics on transport failure but collects no per-hart completion, so "await invalidation" is still "the firmware call returned"; the generation-tagged ack is what phase 03's revoke path needs. |
| 02 slice 4 — x86 PCID/INVPCID runtime gate | **Done and witnessed 2026-09-27** (`73b558c51`, `X86_ACCEL`/`X86_EXPECT_PCID` in the lane). Off: `X86_EXPECT_PCID=0` → shell prompt + `PCID disabled (CPUID pcid=false invpcid=false, CR4.PCIDE=0, CR3=0x59000)`. On: `sg kvm -c 'X86_ACCEL=kvm X86_CPU_MODEL=host X86_EXPECT_PCID=1 …'` → `PCID enabled (CPUID pcid=true invpcid=true, CR4.PCIDE=1, CR3=0x59000)`, i.e. `CR4.PCIDE` set by the kernel and CR3's low 12 bits zero for the kernel root, as the policy requires. Two environment facts worth keeping: TCG cannot emulate PCID (QEMU clears it with a warning), and group membership for `/dev/kvm` only applies to *new* sessions — `sg kvm -c` works in the current one. **Not executed**: the `INVPCID` path itself (no x86 domain can be admitted while the switch-ordering gate is closed); it becomes live with slice 3's reopening. **New finding, unrelated**: the KVM boot faults later in the Tier-3 VMM bring-up (`#GP` at `0xffffffff80087415` after the paging report), so the x86 lane stays TCG-based and the KVM run witnesses only up to that point. |
| 03 — grant lifecycle | Blocked by slice 3 per the plan's own dependency: the revoke path needs a synchronous invalidation acknowledgement and a proven safe-root transition before frames may be released. |
| 07 — warm snapshot | Blocked by 03 (settled accounting/layout) and by the phase-01 qualification gate (feature `snapshot-qualified`; `kernel/src/snapshot.rs:415-431` invalidates a stale header and disables capture/restore). The **functional** witness is available here: `scripts/qemu-boot-test.sh` attaches a raw `virtio-blk` disk and `snapshot.rs` drives `block::read_sector`/`write_sector` at `PART_SNAPSHOT_BASE_LBA`, so save→reset→restore→resume can be exercised in QEMU. What stays hardware-gated is the *claim*: device-backed monotonic freshness, MMC behaviour and measured RT latency need the physical board. |
| 08 — cross-architecture qualification | Depends on 02–07; its matrix also needs the x86 lane and the physical-board witnesses. |

- **The x86 KVM lane now boots** (`a2951b38a`). Under KVM the boot died in `vmxon` with
  CPUID advertising VMX and `IA32_FEATURE_CONTROL` reading locked-with-VMXON-enabled: a
  hypervisor may grant the bits and still refuse nested root operation, and VMXON *faults*
  rather than reporting failure, so an optional capability was taking the whole boot down.
  The attempt is now gated on `hypervisor_present()` (capability stays closed with a visible
  reason; bare metal unchanged), `enter_root` reads `IA32_FEATURE_CONTROL` back after locking
  it, and the preconditions are printed on the early console before the attempt. Both x86
  modes now reach the shell: TCG `PCID disabled`, KVM `PCID enabled (CPUID pcid=true
  invpcid=true, CR4.PCIDE=1, CR3=0x59000)`. Not witnessed: root operation on bare metal.

Two smaller items are recorded but deliberately not "fixed while passing":

- `scripts/build-aarch64-test-hooks-ci.sh`'s `admission-core` marker: the selftest runs after the
  earlier fixtures fill the 64-slot cell-quota table, so its "admitted ELF must succeed" step has
  no slot. Test-harness fix (make the selftest independent of ambient quota, or drain what the
  quota fixtures fill), proved not caused by this session's changes.
- `FALLBACK_MEMORY_MAP` (x86, `kernel/src/boot.rs`): the plan's phase-05 step 3 requires the
  fallback to prove it excludes the live kernel image; today it marks that range usable. Needs
  the x86 lane.

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
