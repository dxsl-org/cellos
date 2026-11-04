---
phase: 0
title: "Governance: ADR amendment, portfolio, lock handoff, ABI items"
status: completed
priority: P1
effort: "0.5d + owner review"
dependencies: []
tier: thinking
checkpoint: true
---

# Phase 00: Governance

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [ADR-0022](../../docs/decisions/0022-intel-x86-64-c2c-only-direction.md) §Hardware item 2, §Paused scope
- [ADR-0029](../../docs/decisions/0029-platform-image-families-and-runtime-detection.md) §2 (DMA policy by profile)
- [Portfolio](../plan-portfolio.md) lines 162-179; [lock inventory](../cell-contract-0024-locks.md)
- [HCL](../../docs/hardware-compatibility-list.md) R1-R9

## Overview

ADR-0022 says "keep one NIC/storage path; do not broaden device support". This plan broadens it, so no code phase may start until the owner signs an amendment, the plan is registered, and file ownership is handed off.

## Key Insights

- The owner chose on 2026-11-03: Intel only (AMD parked), Intel VMD and server RAID handled as HCL constraints, targets KVM + VMware + Intel PC/server.
- The C2C link to record: KVM/VMware guests give cheap, repeatable C2C nodes, and two-node C2C tests no longer wait on hardware purchases. Broader Intel device support also widens the choice of qualification machine.
- One policy decision is still open: how a VM without a vIOMMU gets DMA isolation.

## Requirements

- New `docs/decisions/0031-x86-platform-breadth-kvm-vmware-intel.md` that amends ADR-0022, with an "Amendment 2026-11-xx" pointer added to ADR-0022.
- The ADR records these decisions:
  1. Targets: KVM (q35, default virtio devices), VMware (Workstation/ESXi virtual hardware), Intel physical PC/server.
  2. Out of scope: AMD, cloud NICs, GPU drivers, mlx5/bnxt/ice, Intel VMD driver, MegaRAID/mpt3sas/smartpqi.
  3. **D-VM (DMA policy in guests) — owner chose 2026-11-03 (validation).** Keep `x86_64-pc` = `Required`. Add an `x86_64-vm` profile with `Optional` DMA isolation, selectable only by explicit `cellos.profile=x86_64-vm` with the CPUID hypervisor bit set (same rule as the q35 profile, enforced by `kernel/src/board.rs:279`). Production guests run `x86_64-pc` with a vIOMMU (KVM `intel-iommu` + virtio `iommu_platform=on`; VMware "Virtualize IOMMU").
  4. HCL changes: R2 becomes "reliable clocksource", R1 becomes "console via SPCR/COM or framebuffer + keyboard". Both changes take effect only after phases 02 and 10 ship. R4 adds "VMD disabled; RAID controllers in HBA/AHCI mode or NVMe boot".
- ABI items for Law-1 CP1 (three append-only syscalls plus one semantic change):
  - `QueryPcieFunction = 434`: full BAR table plus capability bytes for an owned BDF.
  - `AllocPcieIrq = 435`: MSI/MSI-X vectors bound to `WaitIrq` irq numbers.
  - `SystemPower = 436`: reboot/poweroff, Supervisor/Init capability only.
  - Semantic change, no wire change: `FindPcieDevice = 418` and `FindPcieDeviceByVendor = 424` return the first matching device **not owned by a live driver cell**, instead of the first match (today an owned first match returns 0: `kernel/src/task/syscall.rs:2465,8722-8743`). Implemented in phase 04a.
  - Allowlist bit sharing proposal: the bitmap is full, so these reuse the PCIe-driver bit and are gated by capability at dispatch.
- Portfolio entry (table row: C2C link, acceptance scenario, evidence ceiling).
- Lock handoff record covering the S1 file set: `kernel/src/acpi*`, `kernel/src/main.rs` (x86 init only), `hal/arch/x86/**`, `kernel/src/task/drivers/{pcie_ecam,iommu_x86,irq_wait}.rs`, `kernel/src/task/{syscall,telemetry}.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `boards/**`, `hal/soc/x86/**`, `cells/drivers/{nvme,ahci,igb,e1000,xhci,virtio-*}`, CI x86 job. Later slices get their own handoffs.

## Architecture

Docs-only phase. Execution order is ADR → portfolio → handoff → CP1 for the ABI items. Phase 04a cannot start without CP1.

## Assumptions

- **Verified 2026-11-04:** syscall numbers 434-436 are free; the highest assigned is `QueryMachineFacts = 433` (`libs/api/src/abi/syscall.rs:672`).
- **Claim:** CELL-CONTRACT-0024 still covers these files. **Confidence:** high. **How to verify:** read `.agents/cell-contract-0024-locks.md`.

## Related Code Files

- Create: `docs/decisions/0031-x86-platform-breadth-kvm-vmware-intel.md`, `.agents/261103-2218-x86-platform-breadth/lock-handoff-approval.md`
- Modify: `docs/decisions/0022-intel-x86-64-c2c-only-direction.md` (amendment pointer), `.agents/plan-portfolio.md`, `docs/hardware-compatibility-list.md` (R4 VMD/RAID wording only), `docs/abi-boundary-records.md` (pending items)

## Implementation Steps

1. Draft ADR-0031 using the ADR-0029 structure. Write D-VM as the recommended option with the rejected alternative ("vIOMMU-only, no VM profile"), plus the security consequence: inside an `x86_64-vm` guest, cells are not DMA-isolated from each other.
2. Add the amendment pointer to ADR-0022.
3. Add the portfolio row under a new "Registered — x86 platform breadth" section.
4. Write `lock-handoff-approval.md` with the S1 file set and its activation condition.
5. Add the three ABI items to `docs/abi-boundary-records.md` as `proposed`, with wire layouts copied from phase 04a and phase 06.
6. Edit HCL R4 wording (VMD off, RAID → HBA/AHCI/NVMe). Leave R1/R2 for phases 02 and 10.
7. Run `node scripts/check-skill-cross-refs.js` if any kit refs were touched (not expected).

## Todo List

- [x] ADR-0031 drafted and owner-approved
- [x] Portfolio row
- [x] S1 lock handoff approved
- [x] CP1 for 434/435/436 + 418/424 semantics approved

## Success Criteria

- [x] ADR-0031 carries an approval line from the owner, quoted verbatim the same way as the ADR-0029 record (`cp1-approval.md`, ADR-0031 status line).
- [x] The portfolio lists this plan with its C2C link, acceptance scenario and evidence ceiling.
- [x] The handoff record names the exact file set and activation condition (CP1 §7; effective 2026-11-04, WIP lane free).
- [x] Law-1 CP1 decision recorded for the three syscalls and the 418/424 semantic change (approved, D-1…D-7).

## Risk Assessment

- D-VM was approved by the owner on 2026-11-03 (validation). It still has to be written into ADR-0031.
- A CP1 rejection blocks 04a and everything after it. Stop and re-plan.

## Security Considerations

D-VM weakens isolation between cells inside a guest. It is acceptable only as an explicit operator choice, logged at boot, and recorded in the machine pin.

## Next Steps

Phase 01, after the handoff becomes effective.

## Deviation Log

- Deviation: the durable-charge journal v2 review was dropped from this phase.
  Why: `cpu_accounting` is `#[cfg(cellos_g1_cpu)]` and in-memory only (see phase 02 Deviation Log).
  Impact: phase 00 no longer requests a durable-charge review; the S1 handoff no longer names `cpu_accounting.rs`.
  Revert: re-add the review if the owner extends phase 02 to the G1 lane.
- Deviation: the handoff request is CP1 §7, and the approval is `cp1-approval.md` (one record for ADR + CP1 + handoff) instead of a separate `lock-handoff-approval.md`.
  Why: this follows the C2C slice d/e precedent, which binds CP1 and handoff in one approval record.
  Impact: none on scope; the lock inventory and portfolio link to `cp1-approval.md`.
  Revert: split the record if a later audit requires it.
- Surprise: ADR-0031 as approved does not state the D-VM consequence ("inside an `x86_64-vm` guest, cells are not DMA-isolated from each other") as explicitly as step 1 asked. It does say `Optional`.
  Why: drafting omission found while closing the checklist.
  Impact: the approved ADR text is bound by digest and left unchanged. Phase 05 must state the consequence in `boards/vm/x86_64-vm/README.md` and in the boot log line.
  Revert: n/a.
