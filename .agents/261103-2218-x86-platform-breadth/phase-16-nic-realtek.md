---
phase: 16
title: "Realtek RTL8111/8168 and RTL8125 (r8169 family)"
status: "blocked: hardware (driver code and host tests complete; capture pending physical machine)"
priority: P2
effort: "5d + physical capture"
dependencies: ["4b"]
tier: thinking
checkpoint: false
---

# Phase 16: Realtek RTL8111/8168, RTL8125

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [NIC cell contract](reference-nic-cell-contract.md)
- Realtek RTL8111/8168 and RTL8125 public datasheets (where available); FreeBSD `re(4)` (BSD) as a behaviour reference

## Overview

Realtek is the most common NIC on consumer boards and cheap mini PCs, including Intel-CPU ones. New `cells/drivers/r8169` cell.

## Key Insights

- One PCI ID (`10ec:8168`) covers many MAC/PHY versions, identified by the XID bits in `TxConfig`. Behaviour differs per XID (PHY init sequences, ePHY patches).
- Some versions need Realtek firmware patches (`rtl_nic/*.fw`) for stable link or EEE. Admit only XIDs that link without a blob. Loading firmware is out of scope unless its license is recorded (contract §Licensing).
- RTL8125 (`10ec:8125`, 2.5G) has a different register set from the 8168 and is handled as its own MAC kind.
- There is no QEMU model (`rtl8139` is a different chip).

## Requirements

- Functional:
  - Identity by PCI ID plus XID; `Driveable` only for captured XIDs.
  - Reset, MAC from IDR0-5, PHY via the MII access register.
  - Descriptor rings (one Tx, one Rx), MSI/MSI-X.
  - 1G autoneg (2.5G for 8125).
  - No firmware loading; an XID that needs firmware is refused with a named log.

## Assumptions

- **Claim:** at least one common RTL8111H/8125B XID links without a firmware blob. **Confidence:** medium. **How to verify:** physical capture.
- **Claim:** FreeBSD `re(4)` license (BSD-2) permits behaviour reference and table reuse with attribution. **Confidence:** high. **How to verify:** file header in the FreeBSD source; record in `NOTICE` if any table is reused.

## Related Code Files

- Create: `cells/drivers/r8169/**`
- Modify: `kernel/src/task/drivers/pcie_ecam.rs` (+ tests), `boards/**`, ISO scripts, CI build list, `NOTICE` (if reuse)

## Implementation Steps

1. Identity + XID decoding + host tests.
2. 8168 path: reset, MAC, PHY, rings, MSI.
3. 8125 path.
4. Physical capture for each admitted XID.

## Todo List

- [X] Identity/XID
- [X] 8168 path
- [X] 8125 path
- [ ] Physical captures (pending physical machine capture per contract)

## Success Criteria

- [X] Host tests: XID → MAC kind table; descriptor encoding.
- [ ] Physical capture per contract for each `Driveable` XID; otherwise `blocked: hardware`.

## Risk Assessment

- High variance between XIDs. Mitigation: per-XID admission; never wildcard on `10ec:8168`.

## Security Considerations

As in the contract.

## Next Steps

Phase 20.

## Deviation Log

None.
