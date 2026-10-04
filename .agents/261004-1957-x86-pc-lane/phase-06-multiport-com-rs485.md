---
phase: 6
title: "Multi-port COM / RS232-485"
status: pending
priority: P3
effort: M
dependencies: [1]
tier: thinking
ceiling: qemu
---

# Phase 06 — Multi-port COM / RS232-485

## Target

Roadmap gate **X86-PC-5**. `cells/drivers/serial` is PL011 (ARM) and x86 exposes
only the COM1 console (`kernel/src/main.rs:157`, `hal/soc/x86/src/lib.rs`), so
the RS232/RS485 ports that make industrial PCs attractive for field integration
cannot be used by any cell.

## Change

- Generalize the kernel 16550 driver from one fixed COM1 to **N ports declared by
  the board descriptor** (`PortIoDevice` list: base + IRQ per port), keeping
  COM1 as the console and keeping the polled-RX fallback intact.
- Decide the user-space surface: a `serial` Driver Cell for x86 mirroring the
  PL011 cell's role (open/read/write/configure per port) rather than exposing
  kernel-only console writes. Record the ABI choice in the phase evidence; do not
  invent a second console stack.
- RS485 direction control: descriptor-declared DE/RE mechanism (GPIO line or
  RTS-driven auto-direction), with the timing requirement documented per board.
- Fail closed for a descriptor that declares a port whose probe does not match a
  16550 register set.

## QEMU-first gate

- q35 with several emulated 16550 ports (e.g. `-device isa-serial,index=0,...`
  variant set) → assert COM2..COMn enumerate, are openable by the cell, and
  echo a marker; assert COM1 console behaviour is unchanged (including the
  polled path with the IRQ gate closed).
- **RS485 is not claimed in this phase.** QEMU has no RS485 DE/RE model, so the
  direction-control timing cannot be gated here; it belongs to phase 07's
  hardware evidence and must be stated as unclaimed until then.

## Acceptance

- On QEMU: N>1 ports usable from a cell, console unchanged, `x86_64-boot` 7/7.
- The descriptor is the only place port facts live; no per-board code.
- RS485 explicitly documented as unclaimed and moved to the hardware phase.

## Out of scope

- RS485 direction timing belongs to phase 07's hardware capture (recorded there,
  not claimed): QEMU has no DE/RE model, so it cannot be gated in this phase.
- DMA-capable serial cards, USB-serial (requires phase 03 plus a usb-serial
  class driver), SIO GPIO/watchdog features, hardware flow control beyond what is
  needed to gate the ports.

## Risk assessment

- **Undo:** revert to single-port COM1 by restoring the fixed driver call sites;
  the console must remain functional at every step of the phase.
- **Not undoable:** a console regression during bring-up — COM1 is the only x86
  debug path, so the phase gate includes the closed-IRQ polled fallback
  (`kernel/src/main.rs:160,711-716`).
