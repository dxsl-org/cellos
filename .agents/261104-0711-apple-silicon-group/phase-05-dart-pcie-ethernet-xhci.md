---
phase: 5
title: "DART IOMMU, Apple PCIe, Ethernet and xHCI"
status: "blocked: hardware (DART 16 KiB page table builder and host tests complete; live S2-hv boot pending physical Mac mini M1 hardware per contract)"
priority: P2
effort: "12d + hardware"
dependencies: ["4"]
tier: thinking
checkpoint: true
---

# Phase 05: DART, Apple PCIe, Ethernet, xHCI (AS-5)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- References (class P/D): OpenBSD `apldart.c`, `aplpcie.c`, `bge(4)`; Linux `t8103.dtsi`, `t8103-j274.dts` (Ethernet on root port 02, MAC filled by the loader)
- x86 breadth [phase 17](../261103-2218-x86-platform-breadth/phase-17-nic-bcm5720.md) (`cells/drivers/bcm57xx`, Intel-owned)

## Overview

DART behind the IOMMU trait, the Apple PCIe root complex (per-port link-up, ECAM, MSI through AIC), then the Mac mini's wired Ethernet and USB-A xHCI as existing or shared driver Cells.

## Key Insights

- DART: 16 KiB pages, a per-stream TTBR, bypass support that `Required` must refuse, and an error interrupt that reports the faulting address.
- Each on-board PCIe device sits behind its own DART stream, so per-device isolation is natural.
- The Ethernet MAC comes from the FDT (`local-mac-address`, filled by m1n1), not from NVRAM.
- The NIC is in the same Broadcom family as x86 phase 17. One cell, two device-ID sets; Apple adds its ID after phase 17 lands.

## Requirements

- DART: map/unmap with 16 KiB granularity, TLB flush, fault interrupt logged with stream ID and address; bypass never enabled.
- Apple PCIe: port bring-up from FDT facts, ECAM enumeration through the existing PCIe path, MSI.
- Ethernet: exact captured PCI ID admitted (`Driveable` only after capture); DHCP.
- xHCI: existing cell built for aarch64, DMA through DART.

## Related Code Files

- Create: `hal/arch/arm/src/aarch64/backend/apple_t8103/{dart,pcie}.rs` (or `hal/soc/apple`), DART host tests
- Modify: `kernel/src/task/drivers/pcie_ecam.rs` (FDT root-complex source), `cells/drivers/{bcm57xx,xhci}` (aarch64 build, Apple IDs), `boards/apple/macmini-m1/**`

## Implementation Steps

1. DART page-table builder with host tests (16 KiB, stream mapping, flush, bypass refusal).
2. PCIe port bring-up under the hypervisor; enumerate the captured IDs.
3. xHCI enumerate a USB keyboard.
4. Ethernet DHCP (after x86 phase 17's cell exists).
5. DART fault test: a deliberate out-of-map DMA is blocked and reported.

## Todo List

- [x] DART + host tests (`AppleDartPt` in `kernel/src/task/drivers/iommu_pt.rs`: 16 KiB pages, L1/L2 tables, bypass refusal)
- [ ] PCIe enumeration (S2-hv pending hardware)
- [ ] xHCI (S2-hv pending hardware)
- [ ] Ethernet (S2-hv pending hardware)
- [ ] DART fault test (S2-hv pending hardware)

## Success Criteria

- [ ] S2-hv: DHCP Tx/Rx over the on-board Ethernet, USB keyboard input at the shell, a DART fault captured with no memory corruption.
- [x] Host tests cover DART mapping, padding and the bypass refusal (`dart_16k_mapping_and_bypass_refusal`).

## Risk Assessment

- PCIe link training needs PMGR clocks and GPIO resets in the right order. The hypervisor MMIO trace of a stock boot (phase 01) is the reference; no Linux driver reading.

## Security Considerations

The DART is the DMA isolation boundary for every PCIe device on this machine; with it absent or failing, the profile refuses the device (fail closed).

## Next Steps

Phase 06.

## Deviation Log

None.
