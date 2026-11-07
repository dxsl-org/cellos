---
phase: 12
title: "VT-d: multiple DRHD units, RMRR, interrupt remapping"
status: completed
priority: P1
effort: "4d"
dependencies: ["4a"]
tier: thinking
checkpoint: true
---

# Phase 12: VT-d multi-unit, RMRR, interrupt remapping

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `kernel/src/task/drivers/iommu_x86.rs` (1149 lines; singleton `VTD_REG_BASE` `:60-66,546`, root/context tables `:75-86`, domains `:98`, CM handling `:704`)
- Phase 01 `AcpiInfo.drhds/rmrrs`; phase 03 `irq_destination` hook; phase 04a vector allocator
- Intel VT-d spec §3 (translation), §5 (interrupt remapping), §8.3-8.4 (DRHD scopes, RMRR)

## Overview

On Intel servers and many client chipsets, devices sit behind different remapping units, and some (USB, iGPU) have firmware-reserved regions. This phase drives every unit, maps RMRRs, and adds interrupt remapping. IR also enables x2APIC IDs above 255 and blocks MSIs forged by cells. Closes S4.

## Key Insights

- A Xeon IIO stack per socket has its own DRHD; only one unit carries INCLUDE_PCI_ALL. Programming that unit for a device scoped to another unit leaves the device untranslated or blocked.
- Client chips: one DRHD dedicated to the iGPU plus one INCLUDE_PCI_ALL. Without the ADR-0033 GPU driver Cell, Cellos does not drive the iGPU, so its unit can stay with translation off. When that Cell is in the image, the iGPU's unit must translate before the Cell starts, and its stolen-memory RMRR stays mapped (ADR-0033 §4; iGPU plan CP1 draft D-G11, noted 2026-11-04).
- Interrupt remapping needs its own IRTE table per unit. With IR on, MSI address/data are remappable-format handles, and the phase 04a MSI-X carve-out becomes a second layer.

## Requirements

- Functional:
  - `VtdUnit` instances (≤16) from DMAR. Device → unit lookup by scope: endpoint, bridge subtree, then the segment's INCLUDE_PCI_ALL unit.
  - Root/context tables and the invalidation queue (or register-based, as today) per unit.
  - Domain = per cell per unit; `GrantDma` maps into the unit that owns the BDF.
  - A unit whose scope covers no device Cellos drives stays disabled and is logged.
  - RMRR: before enabling translation on a unit, identity-map each RMRR range in the context of every scoped device. When such a device is granted to a cell, the identity mapping stays (logged `rmrr kept for bdf`).
  - IR:
    - IRTA table per unit (≤4096 IRTEs), EIM when x2APIC is active.
    - Phase 04a programs remappable MSI.
    - IOAPIC RTEs use remappable format.
    - Compatibility-format interrupts blocked (`GCMD.CFI` stays 0).
    - Enabled when DMAR `INTR_REMAP` is set and all units support IR.
    - When IR is on, CPUs with APIC ID above 255 come online (phase 03 rule lifted).
  - Fail-closed: under `Required`, a device behind a unit that failed init is refused (named log).
- Non-functional: single-unit QEMU lanes unchanged in behaviour.

## Architecture

Split `iommu_x86.rs` into `iommu_x86/{mod.rs, unit.rs, context.rs, domain.rs, invalidate.rs, rmrr.rs, ir.rs}`, each under 200-300 lines. `mod.rs` owns the unit registry and BDF→unit lookup.

## Assumptions

- **Claim:** QEMU `intel-iommu,intremap=on` requires `-accel kvm,kernel-irqchip=split` (the accelerator property in QEMU 8.2). **Confidence:** high. **How to verify:** lane start error otherwise.
- **Claim:** QEMU emulates only one DRHD. **Confidence:** high. **How to verify:** `[acpi] gates: drhd=1`. Multi-unit is proved by host unit tests with a fake register file.

## Related Code Files

- Create: `kernel/src/task/drivers/iommu_x86/**` (replacing `iommu_x86.rs`)
- Modify: `kernel/src/task/drivers/pcie_msi.rs` (remappable format), `hal/arch/x86/src/x86_64/ioapic.rs`, `apic/x2apic.rs` hook, `tests/integration/tests/iommu-dmar-x86.rs`, `tests/integration/src/lib.rs` (`boot_x86_bios_with_vtd_ir`), `docs/hardware-compatibility-list.md` (R8: interrupt remapping required)

## Implementation Steps

1. Module split with no behaviour change; `iommu-dmar-x86` + `igb-x86` VT-d green.
2. Unit registry + scope lookup; fake-register host tests with 3 units (INCLUDE_PCI_ALL, endpoint-scoped, bridge-scoped).
3. RMRR identity maps; host test: RMRR range present in the device's context after domain switch.
4. IR tables, remappable MSI and RTE; lane: `-accel kvm,kernel-irqchip=split` + `intel-iommu,intremap=on,eim=on,caching-mode=on` with nvme, igb, virtio-net (`iommu_platform=on`), `-smp 4`.
5. Negative test: a forged compatibility-format MSI write from a test hook is blocked (`IR fault` logged).

## Todo List

- [X] Module split
- [X] Multi-unit registry
- [X] RMRR
- [X] Interrupt remapping
- [X] Lanes + negative test

## Success Criteria

- [X] Host tests pass: 3-unit scope lookup, RMRR identity mapping, IRTE encoding (xAPIC and x2APIC destinations).
- [X] KVM IR lane: `ir=on`, drivers use remapped MSI, DHCP + NVMe persistence pass at `-smp 4`.
- [X] Existing VT-d lanes unchanged.
- [X] Review: independent `haily-reviewer` security pass on unit selection, RMRR and IR.

## Risk Assessment

- Real firmware DMAR bugs (overlapping scopes, RMRR outside RAM). These are reported per machine in phase 20, and the kernel refuses rather than guesses.

## Security Considerations

This is the DMA and interrupt isolation boundary for physical machines. Required-profile semantics must stay fail-closed at every new branch.

Under the `Required` profile, interrupt remapping becomes mandatory once this phase ships: a physical machine whose DMAR lacks `INTR_REMAP` (or a unit without IR support) is an HCL R8 failure. Without IR, a cell can make its device DMA into the MSI window and spoof interrupts (phase 04a note). HCL R8 is reworded in this phase.

## Next Steps

Phase 13 and phase 20.

## Deviation Log

None.
