---
phase: 4
sub: a
title: "igb part A — identity, registration, Tx/Rx"
status: pending
priority: P2
effort: L (sub-phase of a real NIC family; spec 04 §7 estimates ~5-8K LOC for the family)
dependencies: [1]
tier: thinking
ceiling: qemu
---

# Phase 04a — igb part A: identity, registration, Tx/Rx

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
