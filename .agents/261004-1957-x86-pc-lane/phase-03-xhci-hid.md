---
phase: 3
title: "xHCI + HID family"
status: completed
priority: P2
effort: M (driver cell + HID decode reuse)
dependencies: [1]
tier: thinking
ceiling: qemu
---

# Phase 03 — xHCI + HID family

## Evidence (2026-10-05, `qemu` ceiling)

- New cell `cells/drivers/xhci/` (1472 lines: `main.rs`, `controller.rs`,
  `regs.rs`, `dma.rs`, `input.rs`) plus a shared HID crate `cells/drivers/hid/`
  (`driver-hid`) that `dwc2-usb` now re-exports — decode is single-copy, and the
  ARM checks confirm the BCM lane still builds.
- Standard-path rebuild (`build-x86_64-cells.ps1` → kernel `board-x86-pc` → ISO)
  and `xhci-x86` **4/4**, re-verified by me after the implementer's run:
  `controller init ok caplen=0x40 slots=64 ports=8 intrs=16` → `port 5 reset
  speed=3` → `slot 1 addressed` → `enumerated device vid=0x0627 pid=0x0001` →
  `HID boot keyboard interface=0 endpoint=0x81 mps=8 interval=7` → `USB HID
  keyboard ready` → `key down code=0x10` / `key up code=0x10`.
- Regressions on the same image: `x86_64-boot` 9/9, `ahci-x86` 5/5,
  `nvme-x86` 3/3, `pcie-multibus-x86` 2/2, `driver-registration-contract` 3/3,
  `cellos-boards` 13/13, `cellos-kernel` 187/187, HAL boundaries; ARM:
  `driver-dwc2-usb` and the `board-rpi3` kernel check build.
- Evidence log: `evidence/phase-03-xhci-hid.log`.

## Review (2026-10-05, independent reviewer) — fixed here

The reviewer traced the Enable Slot timeout to one root cause and found five more
defects; all fixed: (1) **operational-register base** — every
`USBCMD`/`USBSTS`/`CRCR`/`DCBAAP`/`CONFIG` access omitted `CAPLENGTH`, so the
controller never received the command-ring base or Run/Stop while the polls read
capability fields and "succeeded"; (2) **slot ID** missing from control bits
31:24 of the slot-scoped commands; (3) **DCI** for an IN endpoint is `2n + 1`
(the keyboard's EP1 IN is 3, not 2); (4) **HID interface descriptor** — class /
subclass / protocol are bytes 5/6/7, and the code compared byte 5 against the
boot subclass, so the keyboard was always rejected; (5) **scratchpad count** —
HCSPARAMS2's high/low fields were swapped; (6) **NIC role** — using
`sys_register_nic_driver()` as an input-producer proof either steals the network
route or is refused, so the call is removed here and the proper role is 03b.

## Scope note

Key **delivery** to the input service is phase 03b (a distinct kernel-verified
USB HID producer role), on the owner's decision, because the existing producer
gate is built on the singleton NIC role. This phase stops at "decoded in the
cell", which is what its oracle asserts.

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

- New Driver Cell `cells/drivers/xhci/` (Tier 1; DMA/MMIO under the documented
  Law-4 unsafe exception like the NVMe/e1000 cells, with `// SAFETY:` comments and
  the F1 unsafe ratchet green):
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
