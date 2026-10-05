---
phase: 6
title: "Multi-port COM / RS232-485"
status: completed
priority: P3
effort: M
dependencies: [1]
tier: thinking
ceiling: qemu
---

# Phase 06 — Multi-port COM / RS232-485

## Evidence (2026-10-05, `qemu` ceiling)

- **Port facts live in the board profile.** `X86PlatformProfile` gained
  `serial_ports: &'static [PortIoDevice]` (console first) and
  `rs485: &'static [Rs485Port]`, with validation that the console *is*
  `serial_ports[0]`, that bases are distinct, that IRQs are ISA, that an RS485
  entry names a declared port, and that its turnaround is non-zero. Both x86
  profiles declare the four standard COM ports; no per-board code decides
  anything.
- **The kernel mechanism carries N ports and fails closed per port.**
  `hal/arch/x86` `configure_all` registers every declared port, probing all but
  the console with two independent tests (scratch register and divisor-latch
  read-back, each restored), and only then programs the port. A declared port the
  machine does not have is refused by name
  (`[x86-gate] serial port 1 base=0x2f8 irq=3 absent (16550 probe failed)`) rather
  than assumed. The console keeps its existing path — `configure`/`init`/
  `init_input_irq` are unchanged, including the OUT2 bit that gates IOAPIC
  delivery — and the extra ports are polled, so no new IDT vector is needed.
- **The user-space surface is a cell over kernel-owned ports** (owner decision,
  recorded here because it differs from "the cell drives the registers"): x86
  cells run at CPL3 with IOPL cleared and there is no port-I/O grant, so a cell
  cannot execute `in`/`out` at all. `/bin/serial` therefore drives the *port
  abstraction* the kernel owns, which satisfies the acceptance — N>1 ports usable
  from a cell — without inventing a hardware-privilege class. Direct port-I/O
  grants (a `sys_request_port_io` plus a per-cell TSS I/O permission bitmap) are
  recorded as the follow-up that would let a cell own the register set, with the
  reason it is not free: arbitrary port I/O is a much wider authority than the
  probed-port surface.
- **ABI (owner-approved):** opcodes **425 `SerialPortInfo`**, **426 `SerialWrite`**,
  **427 `SerialRead`**, **428 `SerialConfigure`**, sharing allowlist bit 50 with
  `RegisterBlockDriver`/`FindPcieDevice`/`FindPcieDeviceByVendor` (the `u64`
  allowlist is full: bits 0–62 are syscalls and bit 63 is the VFS-mutate
  declaration), plus `SerialPortInfo` (4 bytes, `repr(C)`) and the
  **`serial_port`** capability — which required policy blob **v4** (one more cap
  byte) in both `scripts/sign-policy.py` and `kernel/src/policy.rs`. The
  capability is installed through the same `∩ ceiling` intersection as every
  other cap, appears in `boot_ceiling`/`launch_profile`/`with_path_caps`/
  sign-policy, is audited on grant (`PrivilegedCapGranted` bit 3), and a
  boot-ceiling self-test asserts `/bin/serial` holds `serial_port` **only** — no
  PCIe, no MMIO, no DMA.
- Gates: new `serial-x86` **1/1** — q35 with three extra `isa-serial` devices,
  each behind its own `-chardev socket`, so the test reads what the cell
  transmitted and injects a byte for it to receive: `usable=4 declared=4`, the
  COM2 marker arrives on the chardev, the injected byte comes back echoed
  (`[serial] port 1 rx byte=0x5a echoed`), and **COM1 is unchanged** (the shell
  prompt still appears). Regressions: `igb-x86` 2/2, `nic-x86` 2/2, `nvme-x86`
  3/3, `x86_64-boot` 9/9, `pcie-multibus-x86` 2/2, `ahci-x86` 5/5, `xhci-x86`
  4/4, `iommu-dmar-x86` 3/3, `driver-registration-contract` 3/3,
  `cellos-kernel` 193/193, `cellos-boards` 13/13, `hal-soc-x86` 5/5, HAL
  boundaries, F1/F5, sign-policy round-trip.
- Evidence log: `evidence/phase-06-multiport-serial.log`.

## RS485 — declared, not claimed

The profile can express a transceiver (`Rs485Direction::RtsAuto` or
`Gpio { offset }` plus `turnaround_ns`), and the validator rejects a declaration
that names an undeclared port or omits the turnaround. **No machine declares
one**: QEMU has no DE/RE model, so the direction-control timing the declaration
implies cannot be gated in this phase. The hardware capture that can gate it is
phase 07, where the industrial machine's real DE/RE facts belong.

## Not claimed

- No hardware: no physical port has been driven, and the 6-port industrial
  machine's non-standard addresses need their own profile (its HCL record).
- Per-port interrupt RX: the extra ports are polled. Each would need its own IDT
  vector plus an IIR-based demultiplexer, which is a separate decision.
- USB-serial, DMA-capable serial cards, SIO GPIO/watchdog features, and hardware
  flow control beyond what the ports need.


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
