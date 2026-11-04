---
phase: 11
title: "xHCI: BIOS handoff, hubs, multiple controllers"
status: pending
priority: P2
effort: "4d"
dependencies: ["4b"]
tier: medium
checkpoint: false
---

# Phase 11: xHCI for physical machines

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `cells/drivers/xhci/src/{main.rs,controller.rs}` (scope note `controller.rs:3-14`; boot-keyboard match `:926-936`)
- `.agents/261004-1957-x86-pc-lane/phase-03-xhci-hid.md`, `phase-03b-usb-hid-producer.md`; ADR-0020 (staged USB HID isolation)
- xHCI 1.2 §4.22.1 (USB Legacy Support capability), USB 2.0 §11 (hub class)

## Overview

Make USB keyboards work on real Intel PCs and on BMC virtual consoles: take the controller from BIOS SMM, enumerate devices behind hubs, and drive every xHCI controller present.

## Key Insights

- Without the USBLEGSUP handoff, BIOS SMM can keep driving the controller and corrupt OS state (grep found no handoff code).
- iDRAC/iLO virtual keyboards and many desktop ports sit behind a hub [assumption]. Today the driver enumerates root-port devices only.
- Xeon and newer client chipsets can expose more than one xHCI function. Discovery currently returns the first.

## Requirements

- Functional:
  - BIOS handoff: walk xECP, set the OS-owned semaphore, wait ≤1 s for BIOS-owned to clear (force on timeout and log), disable SMI enables in USBLEGCTLSTS.
  - Hub class: GET_HUB_DESCRIPTOR, per-port power/reset/status (USB 2.0 hubs; USB 3 hubs only for pass-through enumeration of their USB 2 companion), route string + root hub port + TT info in the slot context, status-change endpoint polled through the interrupt-IN path.
  - Device depth ≤ 2 hubs; keyboards anywhere in that tree.
  - Multi-controller: one cell instance per controller. Each instance calls `FindPcieDevice(0C/03/30)` and gets the next controller not owned by a live driver (phase 04a semantic change); the instance that gets "not found" exits cleanly.
  - MSI-X via phase 04a (already from 04b).
  - HID input still flows through the USB HID producer role (opcode 423).
- Non-functional: boot unaffected when no USB device is present.

## Architecture

`cells/drivers/xhci/src/{legacy.rs, hub.rs, topology.rs}`. `controller.rs` keeps the ring/slot machinery; `topology.rs` tracks `(route string, slot)` per device.

## Assumptions

- **Claim:** BMC virtual keyboards (iDRAC/iLO) enumerate behind an internal hub. **Confidence:** low. **How to verify:** `lsusb -t` on the phase 20 server.
- **Claim:** QEMU `usb-hub` is a USB 2.0 hub that works on `qemu-xhci`. **Confidence:** high. **How to verify:** lane.

## Related Code Files

- Create: `cells/drivers/xhci/src/{legacy,hub,topology}.rs`
- Modify: `cells/drivers/xhci/src/{main,controller}.rs`, `tests/integration/src/lib.rs` (constructors with hub/second controller), `tests/integration/tests/xhci-x86.rs`

## Implementation Steps

1. USBLEGSUP handoff (QEMU has no BIOS owner, so unit-test the state machine against a fake register file).
2. Hub enumeration; lane: `-device qemu-xhci,id=x -device usb-hub,bus=x.0,port=1 -device usb-kbd,bus=x.0,port=1.2` → key echoed.
3. Two-level hub lane.
4. Multi-controller; lane: two `qemu-xhci` devices, keyboard on the second.

## Todo List

- [ ] Legacy handoff + unit test
- [ ] Hub class
- [ ] Two-level hub lane
- [ ] Multi-controller

## Success Criteria

- [ ] `xhci-x86` gains three cases (behind hub, behind two hubs, second controller), each echoing the injected key in the shell; the existing 4 cases stay green.
- [ ] Handoff unit test: BIOS-owned cleared → ok; never cleared → forced + logged.

## Risk Assessment

- Real-controller quirks (Intel PCH port routing, USB3 link training) only surface physically, in phase 20.

## Security Considerations

ADR-0020 staged HID isolation is unchanged; hub support adds devices but not new authority.

## Next Steps

Phase 20.

## Deviation Log

None.
