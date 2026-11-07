---
phase: 17
title: "Broadcom BCM5719/BCM5720 (tg3 family)"
status: "blocked: hardware (driver code and host tests complete; capture pending physical machine)"
priority: P2
effort: "6d + physical capture"
dependencies: ["4b"]
tier: thinking
checkpoint: false
---

# Phase 17: Broadcom BCM5720 (tg3)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [NIC cell contract](reference-nic-cell-contract.md)
- Broadcom NetXtreme BCM5719/5720 programmer's reference guide (public PRG); FreeBSD `bge(4)` (BSD) as a behaviour reference

## Overview

The BCM5720 (`14e4:165F`) and BCM5719 (`14e4:1657`) are the onboard LOMs of Dell PowerEdge and HPE ProLiant servers. New `cells/drivers/bcm57xx` cell.

## Key Insights

- These NICs share ports with the BMC (NC-SI) and contain the APE management processor. Resetting without the APE lock and driver-state handshake can drop iDRAC/iLO connectivity, which is unacceptable on a server.
- Bootcode and firmware run on-chip. The driver must wait for the firmware mailbox handshake after reset.
- There is no QEMU model.

## Requirements

- Functional:
  - Identity `165F`, `1657` as `Target`; the captured ID becomes `Driveable`.
  - APE lock protocol for PHY/GRC/memory access; driver-state notifications (start/unload) to APE.
  - Reset with the firmware handshake; MAC from NVRAM or the MAC registers.
  - Standard Rx producer ring + return ring, Tx ring, status block, MSI/MSI-X.
  - On cell exit: APE unload notification before quiesce.

## Assumptions

- **Claim:** the public BCM5719/5720 PRG documents the APE lock and driver-state protocol. **Confidence:** medium. **How to verify:** obtain the PRG revision; otherwise use FreeBSD `bge(4)` + attribution.
- **Claim:** a server with BCM5720 is available for capture (Dell R-series). **Confidence:** low. **How to verify:** procurement decision.

## Related Code Files

- Create: `cells/drivers/bcm57xx/**`
- Modify: `kernel/src/task/drivers/pcie_ecam.rs` (+ tests), `boards/**`, ISO scripts, CI build list, `NOTICE` (if reuse)

## Implementation Steps

1. Identity, regs, APE lock state machine with fake-register host tests (lock timeouts, unload on every exit path).
2. Reset + firmware handshake, MAC, PHY link.
3. Rings + status block + MSI.
4. Physical capture: includes BMC reachability before, during and after Cellos boot and a cell restart.

## Todo List

- [X] Identity + APE
- [X] Reset/link
- [X] Data path
- [ ] Physical capture incl. BMC (pending physical server capture per contract)

## Success Criteria

- [X] Host tests: APE lock acquire/release, unload notification on every exit path.
- [ ] Physical capture per contract plus continuous BMC ping with no loss above 5 s across boot and a driver-cell restart; otherwise `blocked: hardware`.

## Risk Assessment

- Losing BMC access on a remote server. Mitigation: first capture only with local console access; a failure is recorded in HCL §6.

## Security Considerations

Do not touch APE memory beyond the documented driver-state words.

## Next Steps

Phase 20 (server).

## Deviation Log

None.
