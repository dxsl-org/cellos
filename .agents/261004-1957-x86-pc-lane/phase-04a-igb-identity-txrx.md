---
phase: 4
sub: a
title: "igb part A — identity, registration, Tx/Rx"
status: completed
priority: P2
effort: L (sub-phase of a real NIC family; spec 04 §7 estimates ~5-8K LOC for the family)
dependencies: [1]
tier: thinking
ceiling: qemu
---

# Phase 04a — igb part A: identity, registration, Tx/Rx

## Evidence (2026-10-05, `qemu` ceiling)

- New cell `cells/drivers/igb/` (`main.rs`, `controller.rs`, `dispatch.rs`,
  `dma_layout.rs`, `identity.rs`): asks for its controller by exact vendor:device,
  reads the MAC over EERD, restarts PHY autoneg, waits for link, brings up TX/RX
  descriptor rings, registers through the existing NIC path only after a device was
  found, and serves frames over the shared NIC IPC. No DHCP logic in the cell
  (04b's gate).
- **Root cause of the earlier partial state, named and fixed**: `FindPcieDevice`
  (418) matched only `(class, subclass, prog_if)`, and Ethernet `02:00:00` is one
  triple for both NIC cells — `/bin/e1000` claimed the igb controller first,
  declined by ID and exited, and `/bin/igb` could only win it inside a retry window
  after the sibling's reap, which pushed registration past the net service's first
  DHCP attempt. The lane passed only when QEMU tracing slowed the guest. Fixed at
  the layer it belongs to: append-only opcode **424 `FindPcieDeviceByVendor`**
  (a0 = vendor, a1 = device, a2 = out_ptr; gated on `PcieDriverCap`; shares
  allowlist bit 50) lets each NIC cell name its own device, so the sibling race is
  gone rather than hidden. `PcieDeviceInfo` grew 40 → 48 bytes
  (`vendor_id`/`device_id`) for the identity check; the owner approved both.
- Gate: `igb-x86` passes **untraced** (3/3 by the implementer, 3 more by me) with
  `controller bound 8086:10c9 82576 (QEMU model) link_up=true` →
  `[net-bridge] first e1000 TX len=304 accepted=true` →
  `[net-bridge] first e1000 RX len=590` → `[net] DHCP acquired — IP configured` →
  `10.0.2.15`, and zero `[e1000]` lines in the transcripts. All eight x86 lanes
  green on the rebuilt production ISO: `igb-x86` 1/1, `xhci-x86` 4/4, `ahci-x86`
  5/5, `x86_64-boot` 9/9, `nvme-x86` 3/3, `pcie-multibus-x86` 2/2,
  `driver-registration-contract` 3/3, `nic-x86` 2/2 (e1000 + its e1000e
  fail-closed assertion). Also green: `cellos-kernel` 189/189, `cellos-boards`
  13/13, `api` 103/103, HAL boundaries, F1/F5, both ARM checks, and both runner
  gates (`X86_NIC_MODEL=igb` present, `X86_NIC_MODEL=e1000e` still refused).
- Evidence log: `evidence/phase-04a-igb-identify.log`.

## Review (2026-10-05) — and what it changed

The reviewer confirmed the sibling race is fixed at the root and the ABI is the
free append-only slot with no added authority, then found that the cell claimed
more SKUs than it can drive. Narrowed accordingly (owner-approved):

- **Claimed now: `10C9` (QEMU's 82576 model) and `1533` (i210 copper with external
  flash)** — the IDs whose NVM access and media path this cell implements. The
  kernel's Ethernet gate and the cell's query list were narrowed together, and the
  kernel unit test now asserts the unimplemented IDs stay refused.
- **Deferred, with the prerequisite recorded rather than the claim widened**:
  `157B`/`157C` (i210 flashless) and `1539` (i211) need an **iNVM** read path
  (EERD/Shadow RAM is not it); `1536`/`1537`/`1538` (fibre/SerDes/SGMII) need
  **media-specific link setup** instead of the copper BMCR restart. The datasheet
  ID list stays in `identity.rs` as the target list.
- **Recorded as a plan risk, not fixed here**: on a host with one supported e1000
  *and* one supported igb, both cells register (singleton, last-wins) while the net
  service caches its first provider TID, so the selected interface is
  scheduling-dependent — the same class as the storage-provider risk, and it needs
  owner-side arbitration (Platform/net) rather than a driver workaround.
- Harness fix found while verifying: `xhci-x86`/`ahci-x86` preferred a
  `build/x86-pc-lane/*.iso` if present, which goes stale against the cell set and
  the image composition (a pre-`/bin/input` lane ISO produced a false red). Both
  now use the production ISO unless `VICELL_XHCI_ISO`/`VICELL_AHCI_ISO` pins one.


## Target

Roadmap gate **X86-PC-3**, first half. Validated decision (2026-10-04): **igb
(i210/i211) is the first family**, because it has public datasheets, an upstream
reference (`igb`), a QEMU model (verified: `igb` = `8086:10c9`, 82576-class), and
it is the chip class used by multi-port industrial boards. `e1000e`/I219 stays a
recorded follow-up decision, not a silent omission.

## Change

- New Driver Cell `cells/drivers/igb/`: PCI ID table seeded with the **verified
  QEMU ID** `8086:10c9` plus the real i210/i211 IDs **read from the datasheet in
  this phase** (never inferred from the model); every other ID `NotSupported`
  with a log line naming vendor:device.
- Scope: BAR MMIO via `request_mmio`, reset, MAC address from NVM/EEPROM,
  TX/RX descriptor rings, unicast/multicast filter setup, link status, bounded RX
  polling. Every DMA buffer through `DmaBuf::authorize`.
- Registration through the existing `nic` path so the net service consumes it
  exactly as it consumes e1000 — no second network surface.
- Change `kernel/src/task/drivers/pcie_ecam.rs:894` from "82540EM only" to
  "families that have a driver cell", keeping the rejection for every ID that
  still has none (the e1000e assertion in `nic-x86` must keep passing).
- Descriptor wiring: `DriverId` variant (phase 01 declares it) + catalog row;
  extend `scripts/build-x86_64-cells.ps1` packaging as 02a does.

## QEMU-first gate

- Extend `scripts/qemu-x86_64-test.sh`: `X86_NIC_MODEL` gains `igb`
  (`-device igb,netdev=net0`), keeping `e1000`/`e1000e`.
- New `tests/integration/tests/igb-x86.rs` extending `nic-x86.rs`: identity must
  not be rejected (the pre-change baseline line
  `[e1000] unsupported Ethernet 8086:10c9; driver gate closed` must be gone),
  driver registration, first bridge Tx with `accepted=true`, first Rx.
- Regression: `nic-x86` 2/2 (including the e1000e fail-closed assertion),
  `nvme-x86` 3/3, `x86_64-boot` 7/7.

## Acceptance

- On QEMU: registration + first Tx + first Rx pass for `8086:10c9`; unsupported
  IDs still fail closed; the e1000e rejection test still passes.
- The datasheet-derived ID list is recorded in the phase evidence, separate from
  the model-derived one.
- No NIC interrupt redesign; the existing net path is reused.

## Out of scope

- DHCP and VT-d gating (04b), `e1000e`/I219, `igc` i225/i226, Realtek, i40e,
  multi-queue, SR-IOV, WoL.

## Risk assessment

- **Undo:** remove the cell + runner option + test and restore the `:894` gate;
  e1000 remains the shipped NIC path.
- **Containment:** `pcie_ecam.rs:894` is shared with the e1000e fail-closed test —
  that test is part of this sub-phase's gate.
