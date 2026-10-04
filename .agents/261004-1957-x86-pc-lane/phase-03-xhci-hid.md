---
phase: 3
title: "xHCI + HID family"
status: pending
priority: P2
effort: M (driver cell + HID decode reuse)
dependencies: [1]
tier: thinking
ceiling: qemu
---

# Phase 03 — xHCI + HID family

## Target

Roadmap gate **X86-PC-2**. x86 has no USB host controller driver at all;
`cells/drivers/dwc2-usb` is BCM2837-only. This gap also blocks USB-serial debug,
so COM1 stays the only debug channel until this lands.

**Scope decision required.** USB xHCI was deliberately frozen out of the shared
driver program by decision —
`.agents/260819-1416-port-common-drivers-g1-g2-g3/phase-01-evidence-and-provenance-gate.md`
item 5, `reports/driver-source-license-bom.md:32`. This phase is that scope
decision being revisited; record it in the phase evidence and in the driver
plan's ledger rather than silently starting work.

## Change

- New Driver Cell `cells/drivers/xhci/` (Tier 1, `#![forbid(unsafe_code)]`):
  PCI class `0x0C`/`0x03` (USB controller, xHCI prog-if `0x30`), capability
  register parsing, BAR MMIO via `request_mmio`, HCRST reset, command ring and
  event ring, port reset/enable, slot/endpoint contexts, transfer rings,
  control transfers (GET_DESCRIPTOR), and interrupt-IN polling.
- HID: **do not fork** the decode path. `cells/drivers/dwc2-usb/src/hid/`
  (`report`, `keymap`, `decode`, `mods`) already decodes HID boot-protocol
  reports; if those modules are controller-agnostic, factor them into a shared
  crate and have both controllers depend on it, per the single-copy rule
  (`docs/code-standards.md`, board/shared-driver ownership).
- Input: deliver key events through the same input path the BCM USB driver uses
  so the shell sees characters without a second input stack. UART input stays
  untouched as the fallback.
- Descriptor wiring (verified pre-Build): add a `DriverId` variant in
  `boards/src/descriptor.rs` plus a `boards/src/catalog_tests.rs` row; no
  per-board code.
- Fail-closed: unsupported controller revision, missing interrupters, or a
  device whose descriptors do not match the supported classes must log a named
  reason and register nothing.

## QEMU-first gate

- New `QemuRunner` constructor `boot_x86_bios_with_xhci(iso)`:
  `-device qemu-xhci,id=xhci` plus `-device usb-kbd,bus=xhci.0`, with a QMP
  monitor so keystrokes can be injected (`send-key`), following the
  `boot_with_pointer` monitor precedent.
- New `tests/integration/tests/xhci-x86.rs`: controller init marker, device
  enumeration marker (VID/PID of the QEMU keyboard), one injected key observed
  by the guest input path, and a shell echo of that character.
- Record the QEMU model caveat: `qemu-xhci` is NEC uPD720200-class; real
  controller revisions are not validated here (`A-03`).

## Acceptance

- On QEMU: enumeration + HID report + shell echo pass; `x86_64-boot` stays 7/7
  and COM1 input is unaffected.
- No controller-specific code is copied from the BCM driver; shared HID decode
  is single-copy.
- The frozen-out scope decision is recorded, not implied.

## Out of scope

- USB storage, USB-serial class drivers, hubs beyond what enumeration needs,
  isochronous transfers, USB3 SuperSpeed tuning, power management.
- Any claim about a physical machine's USB ports.

## Risk assessment

- **Undo:** revert the cell, the shared-crate move, and the test. The shared HID
  factor must be revertible in one commit without touching BCM behaviour.
- **Containment:** the input path is exercised by RPi3 lanes; any change there
  must keep those green (their evidence is exact-device and cannot be re-run
  cheaply).
