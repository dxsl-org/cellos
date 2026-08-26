---
phase: 2
title: "Consume x86 board and SoC policy"
status: completed
priority: P1
effort: "3h"
dependencies: [1]
tier: thinking
---

# Phase 2: Consume x86 Board and SoC Policy

## Overview

Select and validate the descriptor during x86 boot, then parameterize the existing architecture mechanisms from the SoC profile.

## Requirements

- Functional: configure the shared x86 16550 mechanism before first output.
- Functional: use the SoC firmware window when admitting legacy ACPI records.
- Functional: retain zero/default-closed ACPI device gates.
- Non-functional: preserve the existing x86 gate order and log semantics.

## Architecture

`kernel::board` maps the selected descriptor to `hal_soc_x86::GENERIC_PC`. The kernel configures the architecture UART mechanism and consumes the profile for firmware admission; ACPI continues to provide runtime MMIO values.

## Assumptions

- **Claim:** The post-CR3 direct COM1 assembly probe can use the configured UART API without losing diagnostic value.
  **Confidence:** medium
  **How to verify:** run the BIOS QEMU gate and confirm the post-paging marker plus shell prompt.

## Related Files

- Modify: `kernel/Cargo.toml`
- Modify: `kernel/src/board.rs`
- Modify: `kernel/src/main.rs`
- Modify: `hal/arch/x86/src/x86_64/uart_16550.rs`
- Modify: `kernel/src/task/drivers/uart.rs`

## Implementation Steps

1. Add x86 target dependencies and validated board/SoC selectors.
2. Make the x86 UART mechanism consume a one-time port/IRQ configuration.
3. Remove duplicate kernel COM1 I/O constants and use the configured mechanism.
4. Replace hard-coded legacy firmware admission bounds with the selected profile.

## Success Criteria

- [x] x86 kernel check/build passes with no new warnings.
- [x] `kernel/src/main.rs` and x86 UART integration no longer own COM1/IRQ4 or firmware-window facts.
- [x] Missing/invalid ACPI still leaves timer and PCIe gates closed.

## Security Considerations

Configuration must happen once before use; invalid zero/overflowing ranges fail closed.

## Risk Notes

Early UART is a boot-critical path. Revert the parameterization as one slice if the BIOS witness regresses.

## Deviation Log

None.
