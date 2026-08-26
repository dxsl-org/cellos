---
phase: 4
title: "Make Shared SDHCI Policy Data-Driven"
status: completed
priority: P1
effort: "7h"
dependencies: [1, 2]
tier: thinking
---

# Phase 4: Make Shared SDHCI Policy Data-Driven

## Overview

Remove board-feature branches from the shared SDHCI mechanism and express controller layout/access quirks as SoC data plus board wiring.

## Requirements

- Add BCM2711 and JH7110 SDHCI facts without copying the driver.
- Pass one runtime `SdhciAccessPolicy` into `SdhciController`.
- Select controller presence/base via the board/SoC pair.
- Keep RPi3 pin routing board-owned and applied by a reusable BCM GPIO pinmux mechanism.

## Architecture

The controller object owns generic reads/writes; policy selects access width/spacing; board wiring selects pins.

## Assumptions

- **Claim:** RPi4 support is currently limited to the eMMC2 base and requires no existing runtime proof.
  **Confidence:** high
  **How to verify:** grep `board-rpi4` consumers and docs.

## Related Files

- Modify: `hal/soc/bcm27xx`, `hal/soc/riscv`, `kernel/src/task/drivers/mmc*`
- Modify: board wiring descriptors and Cargo dependencies

## Implementation Steps

1. Extend SoC profiles with optional SDHCI controller facts.
2. Make controller access policy a field, eliminating per-board cfg fields/methods.
3. Replace compile-time base constants with selected configuration.
4. Generalize BCM pinmux application from descriptor wiring.

## Success Criteria

- [x] One SDHCI implementation compiles for default, RPi3, RPi4, and VF2.
- [x] No SDHCI mechanism branch tests a board feature.

## Security Considerations

Absent controllers must fail closed and never probe address zero.

## Risk Notes

Access-width and spacing regressions can corrupt SD traffic. Revert runtime policy plumbing as a unit; physical claims remain unchanged.

## Deviation Log

BCM2711 integration also required an RPi4 platform path. Its GPIO, UART, and
SDHCI user grants are disjoint, GIC remains kernel-only, and PCIe is not
advertised until a real BCM2711 host-controller path exists.
