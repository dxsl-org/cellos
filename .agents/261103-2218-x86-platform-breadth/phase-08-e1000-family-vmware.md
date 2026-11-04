---
phase: 8
title: "e1000 82545EM and new e1000e cell (82574L)"
status: pending
priority: P2
effort: "3d"
dependencies: ["4b"]
tier: medium
checkpoint: false
---

# Phase 08: e1000 82545EM and e1000e 82574L

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `cells/drivers/e1000/src/controller.rs`; exact-ID gate `kernel/src/task/drivers/pcie_ecam.rs:27-60,1174-1291`
- `cells/drivers/igb/src/identity.rs` (exact-ID pattern to copy)
- Intel 82574 datasheet (register map, MSI-X, EERD, MDIC)

## Overview

These are the NICs VMware guests get by default: "e1000" = 82545EM `8086:100F` and "e1000e" = 82574L `8086:10D3`. The same 82574L is also QEMU's `e1000e`. The new e1000e cell is the base that phase 13 extends to I219.

## Key Insights

- 82545EM uses the same register model as 82540EM, so this is an identity-table change plus the QEMU `e1000-82545em` lane.
- 82574L keeps legacy descriptors, but differs in MSI-X (IVAR), the EERD layout, PHY init and `CTRL_EXT` bits. A separate cell keeps the 82540EM cell simple, and the PCH-LAN (I219) logic in phase 13 belongs with 82574L-era code.
- The kernel exact-ID gate (`ethernet_id_has_driver`) must list each new ID with its owning cell, or discovery races return.

## Requirements

- Functional:
  - e1000 cell: admits `8086:100E` and `8086:100F`. Logs the SKU.
  - New `cells/drivers/e1000e`: identity table (`8086:10D3` now), reset, MAC from RAL/RAH (fallback EERD word 0-2), PHY autoneg via MDIC, legacy Tx/Rx rings via `GrantDma`, MSI-X (3 vectors: rx, tx, other) via phase 04a, link-status-change handling, net-bridge registration like igb.
  - `pcie_ecam.rs`: `ethernet_id_has_driver` + per-cell exact-ID query tables updated; tests updated.
  - Image composition + CI for the new cell.
- Non-functional: same DHCP latency envelope as the igb lane.

## Architecture

`cells/drivers/e1000e/src/{main.rs, identity.rs, regs.rs, controller.rs, phy.rs, rings.rs, irq.rs}`, with files kept under 200 lines. `identity.rs` returns `MacKind::{I82574, PchLan(gen)}` so phase 13 adds variants without touching the core paths.

## Assumptions

- **Claim:** QEMU's `e1000-82545em` device ID is `8086:100F`. **Confidence:** high. **How to verify:** `qemu-system-x86_64 -device e1000-82545em,help` + guest log.
- **Claim:** QEMU `e1000e` supports MSI-X with 5 vectors. **Confidence:** medium. **How to verify:** 434 output in the lane log.

## Related Code Files

- Create: `cells/drivers/e1000e/**`, `tests/integration/tests/e1000e-x86.rs`
- Modify: `cells/drivers/e1000/src/controller.rs` (identity), `kernel/src/task/drivers/pcie_ecam.rs`, `tests/integration/src/lib.rs` (NIC model parameter), `scripts/qemu-x86_64-test.sh` (`X86_NIC_MODEL=e1000-82545em|e1000e`), `boards/**` (`DriverId::EthernetE1000e`), ISO scripts, CI job

## Implementation Steps

1. 82545EM identity + lane `X86_NIC_MODEL=e1000-82545em` DHCP.
2. e1000e skeleton: identity, reset, MAC, link; lane: link up.
3. Rings + MSI-X; lane: DHCP lease; VT-d variant with `intel-iommu`.
4. Kernel exact-ID table + tests; CI wiring.

## Todo List

- [ ] 82545EM
- [ ] e1000e link
- [ ] e1000e data path + MSI-X
- [ ] Exact-ID gate + CI

## Success Criteria

- [ ] `nic-x86` passes with `e1000`, `e1000-82545em` and `e1000e`, each with and without VT-d.
- [ ] `pcie_ecam` unit tests: `100F` → e1000 cell, `10D3` → e1000e cell, `10C9` → igb; an unknown Intel ID is refused by name.

## Risk Assessment

- QEMU `e1000e` diverges from the real 82574L in PHY details. VMware is a second, independent implementation and is checked in phase 09.

## Security Considerations

DMA only through `GrantDma`; the exact-ID gate prevents another cell from claiming the device.

## Next Steps

Phase 09 VMware matrix; phase 13 extends the cell to I219.

## Deviation Log

None.
