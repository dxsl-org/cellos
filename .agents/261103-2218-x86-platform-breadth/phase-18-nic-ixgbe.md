---
phase: 18
title: "Intel 82599/X540/X550 10GbE (ixgbe family)"
status: "blocked: hardware (driver code and host tests complete; capture pending physical machine)"
priority: P3
effort: "6d + physical capture"
dependencies: ["4b"]
tier: thinking
checkpoint: false
---

# Phase 18: Intel 82599/X540/X550 (ixgbe)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [NIC cell contract](reference-nic-cell-contract.md)
- Intel 82599, X540, X550 datasheets; Intel's BSD-licensed shared code (`ixgbe_*.c` in FreeBSD `ix(4)`)

## Overview

The most widely deployed 10GbE family in existing servers: 82599 SFP+ (`8086:10FB`), X540-T (`1528`), X550-T (`1563`). New `cells/drivers/ixgbe` cell.

## Key Insights

- 82599 SFP+ needs SFP module identification (I2C to the module EEPROM). Firmware may refuse unqualified modules, so record the module part number in evidence.
- X540/X550 are 10GBASE-T with an internal PHY and firmware-managed link (X550 uses the FW/host interface).
- Intel's shared code is BSD-licensed in FreeBSD and may be ported with attribution. Linux code may not be copied.

## Requirements

- Functional:
  - Identity for the three MAC types as `Target`; captured IDs become `Driveable`.
  - Per-MAC-type reset and link setup; SFP identification for 82599.
  - Advanced descriptors (one queue each), MSI-X.
  - MAC from RAL/RAH or NVM.

## Assumptions

- **Claim:** the device IDs above are correct. **Confidence:** medium. **How to verify:** datasheet ID tables.
- **Claim:** the FreeBSD `ix(4)` shared code carries a BSD-3 header. **Confidence:** high. **How to verify:** file headers.

## Related Code Files

- Create: `cells/drivers/ixgbe/**`
- Modify: `kernel/src/task/drivers/pcie_ecam.rs` (+ tests), `boards/**`, ISO scripts, CI build list, `NOTICE`

## Implementation Steps

1. Identity + MAC-type dispatch + host tests.
2. 82599 path incl. SFP I2C.
3. X540/X550 path.
4. Physical capture per admitted MAC type.

## Todo List

- [X] Identity
- [X] 82599 + SFP
- [X] X540/X550
- [ ] Physical captures (pending physical machine capture per contract)

## Success Criteria

- [X] Host tests green.
- [ ] Physical capture per contract for each `Driveable` ID; otherwise `blocked: hardware`.

## Risk Assessment

- SFP module compatibility. Mitigation: record module part numbers; refuse unknown modules by name.

## Security Considerations

As in the contract.

## Next Steps

Phase 20 (server, optional row).

## Deviation Log

None.
