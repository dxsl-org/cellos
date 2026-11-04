---
phase: 10
title: "Console: SPCR, COM2/MMIO 16550, framebuffer text console"
status: pending
priority: P1
effort: "3d"
dependencies: [1, 3]
tier: medium
checkpoint: false
---

# Phase 10: Console beyond COM1

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `hal/arch/x86/src/x86_64/uart_16550.rs:5,31,181-240,274`; `hal/soc/x86/src/lib.rs:168-271` (`STANDARD_COM_PORTS`, `X86_EARLY_FACTS`)
- `kernel/src/task/drivers/console_drv.rs:150-235` (PS/2 input), `kernel/src/main.rs:1461-1500` (framebuffer mapping)
- HCL R1; Microsoft SPCR spec; ADR-0029 two-stage selection (pre-ACPI facts are shared constants)

## Overview

Most current PCs have no COM1, and server serial-over-LAN often sits on COM2 or an MMIO UART. The console follows SPCR (or an explicit command-line choice), and a framebuffer text console with PS/2/USB keyboard input works when no UART exists.

## Key Insights

- Pre-ACPI output stays COM1 best-effort (ADR-0029 early facts). The console switch happens after ACPI parse, so the early-facts contract is unchanged.
- USB keyboard input already reaches the shell through `/bin/input` (pc-lane phase 03b). Only output is missing.
- The kernel console must not depend on the display cell, because logs are needed before cells run.

## Requirements

- Functional:
  - `uart_16550` parameterised: IO-port or MMIO base, register stride (1/4 bytes), IRQ/GSI; default unchanged.
  - Console selection after ACPI, first match wins:
    1. `cellos.console=` (`ttyS0..3`, `mmio@0x…`, `fb`);
    2. SPCR (16550 types, IO or MMIO, GSI);
    3. COM1 if probed present;
    4. framebuffer.
    The selection is logged on the old and the new console.
  - Framebuffer text console: Limine framebuffer, built-in 8×16 bitmap font (public-domain), scrolling, the same log/shell output stream as the UART. It hands the framebuffer over to the display cell when that cell activates (existing `reserve/activate_display_framebuffer`).
  - Console input merges the selected UART RX, PS/2 and USB HID (existing paths).
  - HCL R1 reworded: "a console: 16550 at a SPCR/COM address (or BMC SOL), or GOP framebuffer + PS/2/USB keyboard".
- Non-functional: framebuffer console output must not slow boot by more than 10% (log volume) — measured.

## Architecture

`kernel/src/task/drivers/console_select.rs` (policy) + `kernel/src/task/drivers/fb_console.rs` (renderer) + the existing `console_drv.rs` sink list. Font in `kernel/src/task/drivers/fb_font.rs` (generated table).

## Assumptions

- **Claim:** QEMU `-acpitable file=` can inject an SPCR table on q35. **Confidence:** medium. **How to verify:** boot with a crafted SPCR and read `[acpi] gates: spcr=1`.
- **Claim:** console output is a single sink list that can take a second sink. **Confidence:** medium. **How to verify:** read `console_drv.rs` and the `log` backend in `kernel/src/main.rs`.

## Related Code Files

- Create: `kernel/src/task/drivers/{console_select,fb_console,fb_font}.rs`, `tests/fixtures/acpi/spcr-com2.bin`, `tests/integration/tests/console-x86.rs`
- Modify: `hal/arch/x86/src/x86_64/uart_16550.rs`, `kernel/src/task/drivers/console_drv.rs`, `kernel/src/main.rs`, `kernel/src/boot/limine.rs` (cmdline option), `docs/hardware-compatibility-list.md` R1, `boards/pc/x86_64-pc/README.md`

## Implementation Steps

1. Parameterise the UART (IO/MMIO, stride); unit tests for register addressing.
2. Selection policy + cmdline; lane: `-serial null -device isa-serial,iobase=0x2f8,irq=3,chardev=…` + SPCR fixture → shell on COM2, with RX IRQ.
3. Framebuffer console renderer; lane: `-serial none`, then QMP `send-key` types a command that writes a marker file to the NVMe disk; a second boot with serial verifies the file. This proves framebuffer + PS/2 input end to end.
4. Handover to the display cell: `desktop-x86` still passes.
5. HCL R1 + README.

## Todo List

- [ ] UART parameterisation
- [ ] Selection policy + SPCR lane
- [ ] Framebuffer console + headless-serial lane
- [ ] Display handover regression
- [ ] HCL R1

## Success Criteria

- [ ] `console-x86` lanes pass: COM2 via SPCR (output + RX input), framebuffer + PS/2 (marker file round trip).
- [ ] `serial-x86`, `x86_64-boot`, `desktop-x86`, `xhci-x86` unchanged.

## Risk Assessment

- Some firmware SPCR entries describe a UART the BMC owns at a non-standard clock (baud divisor). The SPCR baud field is used, and a mismatch shows up in phase 20 per machine.

## Security Considerations

The console is a privileged input path, and input sources are unchanged except for which UART is selected. `cellos.console=` is an operator choice on the unmeasured command line, the same trust as `cellos.profile=` (ADR-0029 note).

## Next Steps

Phase 20 uses SOL/framebuffer per machine.

## Deviation Log

None.
