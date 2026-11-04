---
phase: 14
title: "Intel I350 in the igb cell"
status: pending
priority: P2
effort: "1.5d + physical capture"
dependencies: ["4b"]
tier: medium
checkpoint: false
---

# Phase 14: Intel I350 (igb)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [NIC cell contract](reference-nic-cell-contract.md)
- `cells/drivers/igb/src/identity.rs:6-23,35-75`, `cells/drivers/igb/src/main.rs`
- Intel Ethernet Controller I350 datasheet

## Overview

The I350 (`8086:1521` copper, 4-port) is the most common onboard and add-in 1GbE NIC in Intel servers. It belongs to the igb family already driven for 82576 (QEMU) and i210.

## Key Insights

- The register model is close to 82576. Differences: EEE defaults, DMA coalescing, the MDICNFG-selected internal PHY per port, and per-port software/firmware semaphores (SW_FW_SYNC).
- Multi-port: each port is a separate PCI function with the same device ID. Thanks to the phase 04a skip-owned lookup, a second igb instance can claim port 1; C2C needs only one port, so the image launches one instance by default.
- The QEMU igb (82576) lane remains the regression gate for shared code.

## Requirements

- Functional:
  - `IgbSku::I350`, `Driveable` IDs = the captured function ID(s), starting with `1521`.
  - SW_FW_SYNC semaphore for PHY/NVM access per port.
  - EEE disabled at init (no tuning in scope); DMA coalescing off.
  - MAC from NVM per port (the existing EEPROM path with the port offset).
  - MSI-X from phase 04b.

## Assumptions

- **Claim:** `8086:1521` is I350 copper. **Confidence:** high. **How to verify:** datasheet device ID table.
- **Claim:** igb shared code can branch on SKU without a new cell. **Confidence:** high. **How to verify:** `identity.rs` already returns `IgbSku`.

## Related Code Files

- Modify: `cells/drivers/igb/src/{identity,main,controller,dispatch}.rs`, `kernel/src/task/drivers/pcie_ecam.rs` (+ tests)

## Implementation Steps

1. Identity + host tests.
2. Semaphore + EEE/DMAC init with host tests against a fake register file.
3. QEMU igb lanes (± VT-d) stay green.
4. Physical capture on an I350 (server or add-in card in the desktop candidate).

## Todo List

- [ ] Identity
- [ ] Semaphore/init
- [ ] Regression lanes
- [ ] Physical capture

## Success Criteria

- [ ] `igb-x86` (2/2) green; host tests green.
- [ ] Physical capture per contract on an I350; otherwise `blocked: hardware`.

## Risk Assessment

- On some servers the BMC shares port 0 through NC-SI. Reset must not drop the BMC link: capture BMC reachability during the physical test.

## Security Considerations

As in the contract.

## Next Steps

Phase 20 (server).

## Deviation Log

None.
