---
phase: 13
title: "Intel I219 (PCH LAN) in the e1000e cell"
status: pending
priority: P1
effort: "4d + physical capture"
dependencies: [8, 12]
tier: thinking
checkpoint: false
---

# Phase 13: Intel I219 (e1000e PCH)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [NIC cell contract](reference-nic-cell-contract.md)
- Phase 08 `cells/drivers/e1000e` (`MacKind::PchLan`)
- Intel I219 datasheet + PCH datasheet (LAN section), Intel Ethernet Controller I219 spec update
- `.agents/TODO.md:41` (Dell mini PC is the first-machine candidate)

## Overview

The I219 sits on nearly every Intel business desktop, mini PC and NUC, so it is the first physical NIC. It goes into the e1000e cell as the PCH-LAN MAC kind.

## Key Insights

- The I219 is a PHY attached to a MAC inside the PCH. MAC registers resemble the 82574L's, but PHY access needs the SW/FW/ME semaphore (EXTCNF_CTRL.SWFLAG) and the ME firmware status (FWSM).
- Newer generations (Tiger Lake onwards) can boot with the PHY in Ultra Low Power mode. The driver must take the PHY out of ULP before MDIC access works.
- The MAC address is normally programmed into RAL0/RAH0 by firmware. The NVM lives in the shared SPI flash (accessed through a flash BAR or a BAR0 window, depending on the PCH generation).
- There is no QEMU model, so only host tests plus a physical capture apply.

## Requirements

- Functional:
  - `MacKind::PchLan(Gen)` for the generations in the identity table.
  - Initial `Driveable` set = the exact ID(s) of the selected first machine. Other I219 IDs stay `Target` until captured.
  - Reset sequence with the SW flag; ULP exit; K1/PLL workarounds that the datasheet spec update marks mandatory for the admitted generations (listed in `phy.rs` with section references).
  - MAC from RAL0/RAH0 when valid; NVM read as a fallback.
  - PHY link via MDIC with semaphore; autoneg 1G.
  - Rx/Tx legacy descriptors (shared with 82574L); MSI (PCH LAN has no MSI-X on most generations [assumption]).
  - ME coexistence: no write to manageability registers. Releasing the SW flag on every exit path is mandatory.

## Assumptions

- **Claim:** the first-machine candidate has an I219 variant. **Confidence:** low. **How to verify:** `lspci -nn` on the machine (phase 20 checklist item 6).
- **Claim:** PCH LAN exposes MSI but not MSI-X. **Confidence:** medium. **How to verify:** 434 capability list on the machine.
- **Claim:** the I219 device ID list in the Intel spec update is complete for the generations targeted. **Confidence:** medium. **How to verify:** cross-check the datasheet revision used, recorded in `identity.rs` doc comments.

## Related Code Files

- Modify: `cells/drivers/e1000e/src/{identity,controller,phy,regs}.rs`, `kernel/src/task/drivers/pcie_ecam.rs` (exact IDs + tests)
- Create: `cells/drivers/e1000e/src/pch.rs` (flash/NVM, ULP, semaphore)

## Implementation Steps

1. Identity entries for the target generation(s), plus host tests for classification.
2. Semaphore + ULP state machine against a fake register file (host tests: acquire/release on every error path).
3. Reset + MAC + PHY link; then rings + MSI.
4. Physical capture per the contract; promote the ID.

## Todo List

- [ ] Identity + tests
- [ ] Semaphore/ULP + tests
- [ ] Link + data path
- [ ] Physical capture

## Success Criteria

- [ ] Host tests: semaphore release on every exit path; ULP exit sequence order; identity classification.
- [ ] 82574L lanes from phase 08 still green (shared cell).
- [ ] Physical: bind, link, DHCP, 10-minute ping, VT-d active before first DMA — on the exact machine. Otherwise `blocked: hardware`.

## Risk Assessment

- ME/AMT interaction on vPro machines can hold the PHY. Record the vPro state in the HCL row; refuse with a named log when the SW flag cannot be acquired.

## Security Considerations

Do not touch manageability (ME) filter registers; that traffic belongs to the platform, not to Cellos.

## Next Steps

Phase 20.

## Deviation Log

None.
