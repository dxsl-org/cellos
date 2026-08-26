---
phase: 1
title: "Add RPi3 SD Pinmux Quirk"
status: completed
priority: P1
effort: 3h
dependencies: []
tier: thinking
---

# Phase 1: Add RPi3 SD Pinmux Quirk

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Create the smallest board-rpi3-only GPIO function-select helper needed to move the external SD pins from the firmware/U-Boot state to Arasan ALT3.

## Requirements

- Functional: set GPIO34-39 function select to input (`000`) and GPIO48-53 to ALT3 (`111`) using 32-bit GPFSEL read-modify-write.
- Functional: do not write GPPUD/GPPUDCLK or change pulls.
- Non-functional: compile out on every target except `all(target_arch = "aarch64", feature = "board-rpi3")`.

## Architecture

Data enters as a one-time boot call, transforms only GPFSEL3/4/5 bits, exits with pins electrically routed for the existing SDHCI controller. The implementation should mirror the existing RPi3 GPFSEL RMW style in `hal/arch/arm/src/aarch64/uart_bcm_mini.rs:44` while remaining local to the kernel MMC board quirk.

## Assumptions

- **Claim:** BCM2837 ALT3 encoding for GPIO48-53 is `0b111` and GPIO input is `0b000`.
  **Confidence:** medium
  **How to verify:** check BCM2835/BCM2837 ARM peripherals table or official Raspberry Pi DT/overlay before implementation.
- **Claim:** Preserving pulls means no access to GPPUD/GPPUDCLK registers.
  **Confidence:** high
  **How to verify:** grep implementation for `GPPUD` and `GPPUDCLK`.

## Related Files

- Create: `kernel/src/task/drivers/mmc/pinmux_rpi3.rs`
- Modify: `kernel/src/task/drivers/mmc.rs`

## Implementation Steps

1. Add `pinmux_rpi3.rs` behind `#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]`.
2. Define GPIO base `0x3F20_0000` and GPFSEL offsets only; do not define pull registers.
3. Add one private helper for `set_pin_function(pin, func)` with volatile read/write and `// SAFETY:` comments.
4. Add public `prepare_external_sd_for_arasan()` that sets GPIO34-39 input, then GPIO48-53 ALT3.
5. Keep the file under 200 lines and avoid generic GPIO abstractions.

## Success Criteria

- [x] Implementation contains no `GPPUD`, `GPPUDCLK`, mailbox, DT, or SDHOST symbols.
- [x] Only `board-rpi3` builds include the quirk.
- [x] Register math covers pins 34,35,36,37,38,39,48,49,50,51,52,53 exactly.

## Security Considerations

Kernel unsafe MMIO is allowed only for hardware I/O per `docs/code-standards.md:65`; every unsafe access must document boot-time single-core/MMIO preconditions.

## Risk Notes

- Risk: wrong ALT value or off-by-one register math leaves SD disconnected. Mitigation: table-driven constants plus reviewer arithmetic check.
- Rollback: delete this file and remove the module declaration. Cannot undo a GPFSEL write during the same failed boot except by rebooting firmware.

## Deviation Log

- Decision: preserve firmware-configured pulls and change only GPFSEL bits.
- Evidence: the real board progressed from CMD8 failure through SD identification after the quirk was deployed.
