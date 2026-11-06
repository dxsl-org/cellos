---
phase: 8
title: "e1000 82545EM and new e1000e cell (82574L)"
status: completed (QEMU TCG + local KVM ceiling; VMware rows in phase 09)
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

- [x] 82545EM
- [x] e1000e link
- [x] e1000e data path + MSI-X
- [x] Exact-ID gate + CI

## Success Criteria

- [x] `nic-x86` passes with `e1000`, `e1000-82545em` and `e1000e`, each with and without VT-d (TCG and KVM, [evidence](reports/phase-08-evidence.md)).
- [x] `pcie_ecam` unit tests: `100F` → e1000 cell, `10D3` → e1000e cell, `10C9` → igb; an unknown Intel ID is refused by name.

## Risk Assessment

- QEMU `e1000e` diverges from the real 82574L in PHY details. VMware is a second, independent implementation and is checked in phase 09.

## Security Considerations

DMA only through `GrantDma`; the exact-ID gate prevents another cell from claiming the device.

## Next Steps

Phase 09 VMware matrix; phase 13 extends the cell to I219.

## Deviation Log

- **D1 Decision (gate table owner column).** `ETHERNET_DRIVER_IDS` became `(vendor, device, cell)` with `ethernet_driver_cell()`, so the tests assert "100F → /bin/e1000" literally; `ethernet_id_has_driver` semantics and `ETHERNET_LEGACY_CLASS_IDS = [100E]` unchanged. Reversible: drop the column.
- **D2 Deviation (file split).** `cells/drivers/e1000e/src/` also has `reset.rs` (quiesce/reset/station address) and the test-only `fake.rs` (device-shaped `EERD`/`MDIC`/`CTRL` register file), inside the `cells/drivers/e1000e/**` (C) row, to keep every file < 200 lines. `MacKind::PchLan(PchGen)` uses a generation enum; I219 `Target` IDs come from the PCI ID repository and must be re-checked against the I219 datasheet in phase 13.
- **D3 Surprise (EIAC auto-mask).** First run: `irq_wakes` absent, link reached by park expiries only. With `EIAC` set and `CTRL_EXT.IAME` clear, sending the MSI-X message also clears the cause in `IMS` (QEMU `e1000e_msix_notify_one`, read as reference only). Fix: `EIAC = 0`; each park first clears `ICR` and re-arms `IMS`, then re-checks. Result: `irq_wakes=1` (TCG `park_expiries=3`, KVM `park_expiries=0`).
- **D4 Surprise (platform cell gate).** `cells/services/platform/src/scan.rs:222-228` prints `[platform] unsupported Ethernet identity — registration skipped` for every Ethernet ID except `8086:100E` (pre-existing; igb lanes print it too). Binding is unaffected (the kernel ECAM scan publishes BARs). Out of the §2 set, not edited; the script and `nic-x86` match the kernel/cell refusal text `driver gate closed` instead of `unsupported Ethernet`.
- **D5 Decision (runners).** `lib.rs` NIC bodies are parameterized (`boot_x86_bios_with_nic_model`); no `-nic none` there because the `-netdev` already suppresses q35's default NIC (request §1.4). `qemu-x86_64-test.sh` explicit models get `-nic none` + restricted netdev. `/bin/e1000` exits silently when absent so `igb-x86`'s `!contains("[e1000]")` stays valid.
- **D6 Deviation — approved by the integrator 2026-11-06 (xHCI lane `-nic none`).** H-S3-7 (a) made `/bin/e1000e` bind q35's default NIC in the xHCI lane, whose oracle is "no NIC device, so no cell holds the NIC role" (`xhci-x86.rs:243-248`). Fix: `-nic none` in `boot_x86_xhci_inner` (`tests/integration/src/lib.rs`) — the owner-approved H-S3-7 (b) fallback, applied to that runner only.
- **D7 Deviation — CI line approved by the integrator 2026-11-06.** `cargo test -p driver-e1000e` added to the host-test step of `.github/workflows/ci.yml` (next to `driver-nvme -p app-init`); no other `ci.yml` change beyond the `nic-x86` step. Still not done (out of set, left as is): stale comments naming `/bin/e1000` as the 418 user at `kernel/src/task/syscall.rs:8834-8835` and `libs/api/src/abi/syscall.rs:529-534` (pinned files); `boards/qemu/q35-x86_64/board.rs` not listing `EthernetE1000e` (`DriverId` has no kernel consumer).
