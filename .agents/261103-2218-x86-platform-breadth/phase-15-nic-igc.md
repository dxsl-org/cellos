---
phase: 15
title: "Intel I225/I226 (igc)"
status: "blocked: hardware (driver code and host tests complete; capture pending physical machine)"
priority: P2
effort: "4d + physical capture"
dependencies: ["4b"]
tier: medium
checkpoint: false
---

# Phase 15: Intel I225/I226 (igc)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [NIC cell contract](reference-nic-cell-contract.md)
- Intel Ethernet Controller I225/I226 datasheets; `cells/drivers/igb` as the structural model

## Overview

The I225/I226 2.5GbE controllers are standard on recent Intel boards and mini PCs. New `cells/drivers/igc` cell.

## Key Insights

- The register layout descends from igb, but the family is distinct: advanced descriptors only, internal 2.5G PHY via MDIC with GPY-specific setup, and NVM either flash-backed or iNVM.
- Early I225 steppings (I225-V B1/B2) had link-stability errata at 2.5G. Admit only the steppings captured; others are `Target`.
- There is no QEMU model.

## Requirements

- Functional:
  - Identity: I225-LM `15F2`, I225-V `15F3`, I226-LM `125B`, I226-V `125C` as `Target`; the captured ID(s) become `Driveable`. Revision ID recorded.
  - Reset (CTRL.DEV_RST), SW_FW_SYNC semaphore, PHY autoneg up to 2.5G, MAC from RAL/RAH or NVM.
  - Advanced Rx/Tx descriptors (one queue each), MSI-X.

## Assumptions

- **Claim:** the device IDs above match the datasheets. **Confidence:** medium. **How to verify:** datasheet ID tables, recorded in `identity.rs`.
- **Claim:** a machine with I226-V is obtainable (common in mini PCs). **Confidence:** medium. **How to verify:** procurement decision in phase 20.

## Related Code Files

- Create: `cells/drivers/igc/**`
- Modify: `kernel/src/task/drivers/pcie_ecam.rs` (+ tests), `boards/**` (`DriverId::EthernetIgc`), ISO scripts, CI build list

## Implementation Steps

1. Identity + regs + host tests.
2. Reset/semaphore/PHY with fake-register tests.
3. Rings + MSI-X; compile + package.
4. Physical capture.

## Todo List

- [X] Identity + regs
- [X] Reset/PHY
- [X] Data path
- [ ] Physical capture (pending physical machine capture per contract)

## Success Criteria

- [X] Host tests green; cell packaged in the x86 image.
- [ ] Physical capture per contract; otherwise `blocked: hardware`.

## Risk Assessment

- Stepping errata. Mitigation: exact-revision admission.

## Security Considerations

As in the contract.

## Next Steps

Phase 20.

## Deviation Log

None.
