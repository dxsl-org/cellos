---
title: "Tier 3b finish + G5 Lite-profile foundations"
description: "Track A finishes the Tier 3b Linux VM (x86 bring-up, full-Ubuntu wide guest, residual hardening); Track B specs the G5 Lite CoW-golden foundations design-first with ARM64 + x86 arch parity, gated on a real-HW virt testbed."
status: pending
priority: P2
effort: Track A ~9.5-13K LOC / ~110-160 eng-days (coding, part real-HW-gated) · Track B ~0 LOC + 2 SPIKEs / ~18-26 eng-days (design/spec, now-able)
branch: fix/ci-followups-srv-lua-qemu
tags: [tier3b, hypervisor, g5, cow-golden, x86-svm, ept-npt, virtio, security, design]
created: 2026-07-22
---

# Tier 3b finish + G5 Lite foundations

Two tracks. **Track A = CODING** (finish the Tier 3b Linux VM; ARM64 EL2 VMM shipped, x86 greenfield). **Track B = DESIGN/SPEC, now-able** (G5 Lite CoW-golden foundations, now with ARM64 **and** x86 EPT/NPT parity per user scope decision; NO coding until a real-HW VMX/SVM/EL2 testbed exists). Every phase labelled **now-able** (design) or **coding** (needs real-HW testbed for final validation).

Reconciles (does not duplicate): `.agents/260712-0952` (P04-08 fold into Track A), `.agents/260711-1917` (x86 SVM, 0% implemented — Track A P01 continues it), `.agents/260613-2134` (ARM64 EL2 — DONE, the substrate).

## Phases

| # | Phase | Track | Label | Effort | Law 1 / K-Boundary | Depends |
|---|-------|-------|-------|--------|--------------------|---------|
| 01 | [x86 VMM bring-up (SVM-first, PVH, no-LAPIC MVP) + TCG-VMRUN spike](phase-01-x86-vmm-bringup.md) | A | **coding** (world-switch + device-model = real-HW/KVM-gated; only PVH ELF-note parse host-unit-testable) | XL (~5-6K) | **Law 1** (ViVmExit x86 variants, VERSION 1→2) | — |
| 02 | [Wide-guest VMM substrate + minimal-glibc boot (T1)](phase-02-wide-guest-substrate.md) | A | **coding** (ARM64 TCG) | L (~2-3K) | no (manifest write-cap) | — |
| 02b | [Full Ubuntu + systemd + apt-persist image pipeline (T2)](phase-02b-full-ubuntu-image.md) | A | **coding** (ARM64 TCG) | XL (~1.5-2.5K + image infra) | no | 02 |
| 03 | [Residual hardening — C1 IRQ cap (IN PROGRESS elsewhere) + virtqueue fuzz + bounds verify](phase-03-residual-hardening.md) | A | **coding** (ARM64 TCG) | L (~1-1.5K) | no | — |
| 04 | [VMM one-core + feature-flag preset design (CoW = arch-specific, NOT shared core)](phase-04-profile-flag-matrix-design.md) | B | **now-able** (design) | M (0 LOC) | no | 03 |
| 05 | [CoW-golden clone spec — OWNS provenance/refcount model + SAS multi-region guard rework (ARM64)](phase-05-cow-golden-clone-spec.md) | B | **now-able** (design) | XL (0 LOC) | flags Law 1 (new syscall) | 04 |
| 05b | [x86 EPT/NPT CoW parity spec (write-violation decode, RO-golden pages)](phase-05b-x86-cow-parity-spec.md) | B | **now-able** (design) | L (0 LOC) | flags Law 1 | 05, 01(world-switch) |
| 06 | [Reset-to-golden + VMID lifecycle + S2 TLB-invalidation + zero-on-free + atomic reset (ARM64)](phase-06-reset-to-golden-spec.md) | B | **now-able** (design) | L (0 LOC) | flags kernel-priv mechanism | 05 |
| 06b | [x86 INVEPT/INVVPID + VPID lifecycle parity spec](phase-06b-x86-invept-vpid-spec.md) | B | **now-able** (design) | M (0 LOC) | flags kernel-priv mechanism | 06, 05b |
| 07 | [vCPU + device-state save/restore split + ARM64 snapshot SPIKE + validated restore](phase-07-vcpu-device-state-split.md) | B | **now-able** (design + SPIKE) | L (0 LOC + spike) | flags Law 1 | 05 |
| 08 | [SECURITY: golden-frame poisoning + lifecycle across ALL teardown paths + restart survival](phase-08-golden-frame-security.md) | B | **now-able** (design + fault-injection test spec) | L (0 LOC design) | flags kernel-priv mechanism | 05, 06 |

## Dependency Graph (re-baselined)

```
Track A (coding — mostly file-disjoint):
  01 (x86: SPIKE gate ─► world-switch real-HW/KVM lane)
  02 (wide substrate, T1) ─► 02b (full Ubuntu, T2)
  03 (harden; C1 fix in progress via separate implementor)

Track B (design, now-able; gated on 03 baseline):
  03 ─► 04 (profile flags; records CoW is arch-specific) ─► 05 (CoW-golden ARM64; OWNS provenance/refcount + SAS multi-region guard)
                                                             │
        01 world-switch ──────────────► 05b (x86 EPT/NPT CoW parity)
                                                             │
   05 ─┬─► 06 (reset + VMID + S2 TLB + zero-on-free + atomic) ─► 06b (x86 INVEPT/INVVPID + VPID)  [also needs 05b]
       ├─► 07 (vCPU/device snapshot + SPIKE; restore via P03 validator)
       └─► 08 (SECURITY: poisoning + all-teardown-path refcount + restart survival)  [also needs 06]
   05 ──ABI-ordering edge──► 01  (S2PermFault variant appends at disc 12+, AFTER P01's x86 variants at 8-11)
```

- **Provenance/refcount model has a SINGLE owner: P05.** P06 (reset teardown), P07 (restore), P08 (all-path lifecycle + security) are pure consumers ("uses model defined in P05 §Provenance"). Security constraints are fixed WITH the mechanism in P05, not bolted on after.
- **CoW is an arch-specific mechanism, NOT shared core** (P04 records this). ARM64 spec = P05/P06; x86 EPT/NPT parity = P05b/P06b, gated on P01's world-switch (real-HW).
- **Track A critical path (software compat):** 02 → 02b (full Ubuntu apt-persist). x86 (01) is on its own real-HW lane.
- **Track B critical path:** 04 → 05 (the hub) → {06, 07, 08}; x86 parity 05b/06b trails P01.

## Law 1 / Kernel-Boundary flags (explicit)
- **P01 — Law 1 (2× user confirmation):** x86 `ViVmExit` variants (PortIn/PortOut/Hlt/Msr) at discriminants 8-11 + `VERSION` 1→2 (`libs/api/src/abi/hypervisor.rs:19`). No new syscalls (220-227 arch-generic).
- **P05 / P05b / P07 — Law 1 flags (design surfaces, commits nothing):** CoW clone + snapshot likely need new syscalls (`sys_clone_vm_from_golden`, `sys_snapshot_vcpu`/`sys_restore_vcpu`) and a `ViVmExit::S2PermFault`/`EptViolation` variant. Design presents the ABI delta for approval; nothing lands. **S2PermFault appends at disc 12+ AFTER P01's x86 variants (append-only freeze).**
- **P06 / P06b / P08 — Kernel Boundary:** S2 TLB-invalidation (`tlbi ipas2e1`/`vmalls12e1is`), x86 `INVEPT`/`INVVPID`, VMID/VPID recycle, golden-frame RO-in-identity-map are EL2/ring-0 privileged mechanisms → correctly kernel-side. Each is NEW; justify against the 4-question test, keep minimal, policy stays cell-side.
- **General:** device backends already live in the hypervisor cell (GOOD). P04 must NOT propose moving any into the kernel for speed.

## Success criteria (functional)
- **Track A:** build + boot + run in QEMU. P02 = minimal-glibc boots to shell (ARM64 TCG). P02b = **full Ubuntu + systemd boots; `apt install` persists across reboot** (ARM64 TCG). P03 = fuzz harness clean + C1 cap holds under IRQ-spam. P01 = only the PVH ELF-note parser is host-unit-testable; the SVM-VMRUN spike must pass BEFORE the ~5-6K LOC estimate is trusted; world-switch + device model marked **real-HW-only validation**.
- **Track B:** each spec is executable by `/hc-cook` without re-deriving mechanism (fault decode, data flow, ABI delta, provenance model, test matrix, rollback). P08 specifies fault-injection tests with measurable pass conditions. No runtime validation claimed (design-only). P07's snapshot consistency contract requires an ARM64 SPIKE, not armchair spec.

## Positioning guard (corrected — do NOT overclaim)
Cold-boot ~150ms is an **UNMEASURED target**, not "parity plausible" — there are zero measurements and a contrary data point (FAT loader quadratic re-seek, `loader_image.rs:130-134`). Gate the number behind a measured ARM64 cold-boot baseline on a KVM-accel/real-HW lane. Headline sub-10ms REQUIRES P05/P06/P07 snapshot/CoW work. G5 value = dual-purpose (first-party fleet instant-restart + agent-sandbox latency), NOT an untrusted-multi-tenant-hosting moat.

## Red Team Review (2026-07-22 — 4 reviewers, 4 Critical + 9 Major + 2 Minor, all code-grounded, all ACCEPTED)

| ID | Sev | Finding (file:line) | Resolution |
|----|-----|---------------------|------------|
| C1 | **Crit** | CoW substrate does NOT exist as scout claimed. `map()` SAS guard (`stage2.rs:274-279`) checks ONE contiguous region and is SKIPPED when `guest_ram_pages==0`. A clone maps borrowed-golden (another VM's region) + scattered overlay = two disjoint regions the single-region model can't express → `SasViolation` on golden map, or guard bypass = SAS escape. 4/4 reviewers. | **P05 re-architects the guard**: per-table **multi-region HPA allowlist** = golden range(s) [RO-only] ∪ this clone's overlay carve [RW]; `writable=false` may target golden, `writable=true` only owned overlay, else `SasViolation`; `guest_ram_pages==0` never means "no check". Scout corrected: "RO descriptors expressible; isolation guard must be re-architected." THE CoW substrate blocker. |
| C2 | **Crit** | Golden lifecycle beyond clone-Drop. (a) `reap_vms_for_task` (`registry.rs:531`) frees ALL frames, no refcount → kill golden owner while clones live = UAF. (b) never-die restart: registry keyed by owner_tid; restart→new tid → frees golden + all clones = instant-restart wipes the baseline. | **P08**: golden refcount gates ALL teardown paths (reap/kill/Drop/reset); golden ownership survives owner-cell restart (kernel-held refcounted registry decoupled from transient hypervisor-cell tid + re-attach path). Tests: kill golden owner w/ live clone → frames stay allocated; restart cell → golden survives. |
| C3 | **Crit** | VMID monotonic, wraps u16, never recycled (`registry.rs:58` `AtomicU16::new(1)`, `:105` `fetch_add`). Fast clone fleet → wrap → reuse live VMID → stale S2 TLB matches across VMs → cross-VM r/w. | **P06**: VMID free-list/generation-counter; on teardown `tlbi vmalls12e1is` for that VMID before return; refuse reuse while stale TLB could persist. Hard dependency of the P06 TLB primitive. x86 VPID analog → P06b. |
| C4 | **Crit** | Track B was silently ARM64-only. | **RESOLVED by user scope decision #1** — x86 EPT/NPT parity budgeted as P05b/P06b (properly phased, not just mentioned). |
| M1 | Major | Provenance model ownerless (P05/P06/P08 each point at another) + P08 sequenced AFTER the phases it must constrain. | **P05 is sole owner** of the provenance/refcount type; hoisted into P05; P06/P07/P08 are consumers. Security fixed with mechanism. |
| M2 | Major | Freed frames never zeroed (`frame.rs:120,142` bitmap-only; frame-identity keeps contents) → reset frees overlay unzeroed → next tenant reads prior secret. | **P06**: zero overlay frames on free (or zero-on-carve for all guest-RAM handouts); "no re-zero" speed claim applies ONLY to RO-golden re-point, never to frames leaving VM ownership. |
| M3 | Major | CoW fault handler undefined on allocator exhaustion → panic/hang; no per-clone overlay quota. | **P05**: exhaustion policy (graceful clone-fail vs guest fault) + per-clone overlay watermark; test allocator-None injection → bounded non-panic. |
| M4 | Major | Reset-to-golden not atomic — mid-loop kill frees some overlay before `clear()` → double-free / half-golden corrupt guest. | **P06**: transactional reset (build new mapping, tlbi, then free overlay after atomic swap); crash-consistency; kill-injection test at each step. |
| M5 | Major | P07 restore bypasses P03 virtqueue `cur<q_size` clamp + no cross-surface rollback (kernel vCPU blob + cell DeviceSnapshot). | **P07**: route restore through the P03 MemBackend validator; validate-all (esp. kernel vCPU blob) BEFORE mutating cell device state; define abort/rollback. |
| M6 | Major | P01 "device-model TCG-testable" unsound — backends only run via world-switch exits; TCG nested-SVM VMRUN fidelity never exercised (0 LOC). | **P01**: honest split (only PVH ELF-note parse host-unit-testable); early **SPIKE** proving TCG can VMRUN + deliver one PIO exit BEFORE committing the LOC estimate. |
| M7 | Major | Cold-boot ~150ms unmeasured; contrary data (FAT quadratic re-seek `loader_image.rs:130-134`). | Positioning corrected to "unmeasured target"; gated behind a measured ARM64 baseline on KVM/real-HW lane. Roadmap §G5 corrected. |
| M8 | Major | P07 "fiddly 20%" understates — `vcpu_regs` (`registry.rs:354-385`) copies only 32×u64 (GPRs + `g_elr_el2`); MISSING SPSR/PSTATE, SCTLR/TTBR0-1/TCR/MAIR/VBAR, SP_EL0-1, TPIDR, CNTV/CNTP, all vGIC (GICH LR/VMCR) ≈ 1/10 of the register surface. | **P07**: pulled out of the design estimate; consistency contract needs a snapshot/restore SPIKE on the shipped ARM64 guest; staged deliverables. |
| M9 | Major | Full-Ubuntu wide guest categorically bigger than "minimal glibc". | **RESOLVED by user scope decision #2** — split P02 (substrate + minimal-glibc T1) / P02b (full Ubuntu + systemd + apt-persist T2); 512 MiB-1 GB contiguous RAM carve verified against `allocate_guest_ram` (`frame.rs:339`) or spec non-contiguous mapping. |
| m1 | Minor | CoW fault handler lock order unspecified. | **P05**: single global order FRAME_ALLOCATOR → registry_lock (matches reaper deferred-free); handler drops registry_lock before allocating overlay. |
| m2 | Minor | `S2PermFault` variant discriminant ordering vs P01. | **P05**: appends at disc 12+ AFTER P01's x86 variants (8-11); explicit P05→P01 ABI-ordering edge added to graph. |

*Adjudication: nothing rejected, no DEFER. Two reviewers rated the SAS-guard Critical, one rated x86-divergence Critical — both escalated. User accepted all + two scope expansions (x86 parity, full Ubuntu).*

## Sources
- Scout: [scout-report.md](scout-report.md) (C1 correction applied) · G5 memory `project-g5-dual-profile-vm` · roadmap §Stage G5
- Prior plans: `.agents/260712-0952-*`, `.agents/260711-1917-*`, `.agents/260613-2134-*`
- Code: `kernel/src/memory/stage2.rs`, `frame.rs`, `kernel/src/hypervisor/registry.rs`, `cells/services/hypervisor/src/`, `libs/api/src/abi/hypervisor.rs`
