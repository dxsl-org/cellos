---
phase: 3
title: "Extract QEMU ARM Virt SoC Facts"
status: completed
priority: P1
effort: "7h"
dependencies: [1]
tier: thinking
---

# Phase 3: Extract QEMU ARM Virt SoC Facts

## Overview

Create a data-only QEMU ARM virt profile and route PL011, GIC, VirtIO, RTC, PCIe, paging, and resource consumers through it.

## Requirements

- Add `hal/soc/arm-virt` with immutable layout/IRQ facts and validation tests.
- Keep PL011/GIC/PCIe/VirtIO mechanisms single-copy.
- Use the QEMU AArch64 board descriptor for fallback memory and enabled drivers.
- Preserve register offsets, GIC programming, mappings, and IRQ numbers.

## Architecture

`hal/soc/arm-virt` owns platform facts; `boards/qemu/virt-aarch64` owns product boot/wiring/driver selection; ARM HAL and kernel own mechanisms.

## Assumptions

None — the current QEMU virt constants were inventoried directly.

## Related Files

- Create: `hal/soc/arm-virt/`
- Modify: workspace/Cargo wiring, ARM HAL PL011/GIC, kernel paging/platform/resource/VirtIO/PCIe consumers

## Implementation Steps

1. Model validated QEMU ARM layout and IRQ topology.
2. Replace hardcoded consumer bases/ranges/IRQs with profile aliases.
3. Route AArch64 fallback/platform defaults through the board descriptor.

## Success Criteria

- [x] ARM-virt host tests and AArch64 kernel/HAL checks pass.
- [x] Scoped QEMU ARM hardware literals exist only in the SoC profile, board fallback assets, tests, or guest-emulation contracts.

## Security Considerations

Keep GIC/ECAM kernel-only and preserve current user-MMIO grants exactly.

## Risk Notes

Mapping drift can fault early boot. Revert consumers and remove the profile as one phase rollback.

## Deviation Log

QEMU AArch64 reached the `ViCell >` shell. The existing test script still waits
for `Cellos >`, so the runtime was observed but the script gate remains stale.
