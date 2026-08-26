---
phase: 3
title: "G1 Hardware Peripheral Controllers"
status: completed
priority: P1
effort: "8d"
dependencies: [2]
tier: thinking
---

# Phase 03: G1 Hardware Peripheral Controllers

## Context Links

- `docs/specs/04-hardware.md:65-75`; `docs/specs/13-peripherals.md:4-5`; `docs/specs/13-peripherals.md:16-25`; `docs/specs/13-peripherals.md:160-177`.
- `boards/src/descriptor.rs:20-45`; `hal/traits/gpio/src/lib.rs:20-46`; `hal/traits/i2c/src/lib.rs:28-45`; `hal/traits/spi/src/lib.rs:12-49`.
- Existing fallback/prototype crates: `cells/drivers/i2c-gpio`, `cells/drivers/spi-gpio`, `cells/drivers/pwm-gpio`, `cells/drivers/adc-sim`, `cells/drivers/can-loopback`.

## Overview

Promote G1 by adding real shared controller drivers in order: GPIO IRQ/ownership, hardware I2C controller, hardware SPI controller. The safe slice is already implemented for BCM BSC1/SPI0 and exact MMIO authority; bit-bang/sim/loopback crates remain fallback/test baselines, not promoted stage evidence.

## Requirements

- Functional: GPIO IRQ/ownership, BCM BSC I2C for RPi line, BCM SPI for RPi line, DesignWare I2C/SPI only where verified compatibles exist.
- Non-functional: bounded polling, no async borrowed buffers, no board-specific driver clone.

## Architecture

Data flow: board compatibles/pinmux -> SoC MMIO/IRQ policy -> `DriverId` selection -> `MmioRegion`/IRQ lease -> HAL trait impl -> Driver Cell IPC/service API -> robot/sensor smoke -> serial/log output.

## Related Code Files

- Implemented: `libs/api/src/abi/{manifest_flags.rs,manifest.rs,manifest_macro.rs,manifest_tests.rs}`.
- Implemented: `kernel/src/{resource_registry.rs,resource_registry_tests.rs,policy.rs}`.
- Implemented: `kernel/src/task/{cap.rs,drivers.rs}` and `kernel/src/task/drivers/bcm_pinmux.rs`.
- Implemented: `cells/drivers/{i2c-bcm,spi-bcm}/`.
- Keep fallback/test only: `cells/drivers/{i2c-gpio,spi-gpio,pwm-gpio,adc-sim,can-loopback}/`.
- Reference only: `hal/traits/{gpio,i2c,spi}/`, `kernel/src/task/drivers/{gpio_irq,irq_dispatch,uart}.rs`.

## Implementation Steps

1. Add missing `DriverId` entries for I2C/SPI controller families; keep PWM/ADC/CAN/NPU out until their phase.
2. Close GPIO IRQ/ownership first, including BCM and SiFive paths where board data selects them.
3. Implement BCM BSC I2C for RPi-line boards with bounded transfer, repeated-start, NACK, and bus recovery gates.
4. Implement BCM SPI for RPi-line boards with mode, chip-select, FIFO, and DMA-disabled safe baseline.
5. Add DesignWare I2C/SPI only after DT/compatible evidence; do not infer JH7110 support from name alone.
6. Keep bit-bang I2C/SPI/PWM, ADC sim, and CAN loopback as tests/fallback; do not count them as real-controller promotion.

## Todo List

- [x] Descriptor entries and board selection for real I2C/SPI controllers.
- [x] GPIO edge IRQ positive/negative tests and physical RPi3 wiring proof.
- [x] BCM BSC I2C repeated-start, NACK, timeout, and recovery tests.
- [x] BCM SPI mode/bounds/chip-select lifetime tests.
- [ ] DesignWare compatibility proof before enabling DW controller crates.

## Success Criteria

- [x] RPi-line vertical slice: real I2C read or explicit NACK -> GPIO actuator -> output marker.
- [x] QEMU ARM virt PL061 still passes synthetic GPIO IRQ path.
- [x] Driver Cells exit cleanly when selected controller MMIO/IRQ is absent.
- [x] RPi3 current physical result is recorded as PASS/FAIL/BLOCKED before claiming RPi3 evidence.

## Test Matrix

- Unit: HAL traits, controller register model, fallback bit-bang state machines.
- Integration: QEMU ARM virt synthetic GPIO, RPi compile, no-controller fail-closed.
- E2E: RPi3 physical UART + GPIO + real I2C/SPI when wired; VF2/RPi4 only after board/DTB evidence.

## Risk Assessment

| Risk | LxI | Mitigation |
|---|---|---|
| Fallback drivers misreported as promoted | HxH | capability matrix separates bit-bang/sim/loopback from real-controller gates. |
| GPIO shared by I2C/SPI fallback deadlocks | MxH | explicit bus lease/release and timeout paths. |
| Unverified DW-compatible target selected | MxH | require DT/manual evidence before enabling DW I2C/SPI for a board. |
| IRQ storm or missed ack | MxH | top-half ack bounds and bottom-half event drain tests. |

## Security Considerations

Peripherals must not widen MMIO grant ranges beyond exact SoC policy; driver death must release ownership.

## Backward Compatibility

Keep existing demo output markers unless tests are updated in the same slice.

## File Ownership

Owns G1 GPIO/I2C/SPI controller files and descriptor entries; does not touch PCIe/NVMe/e1000 or PWM/ADC/CAN promotion.

## Rollback

Revert controller crates and board selection deltas; leave reserved DriverIds if ABI published. Irreversible part: physical hardware side effects on connected devices, mitigated by low-power actuator/sensor test rig.

## Assumptions

- User-confirmed decision: RPi3 BCM BSC/SPI is the first real controller lane after the current RPi3 smoke gate. Verify exact MMIO/IRQ/pinmux facts against the RPi3 DTB/manual before implementation.
- Claim: JH7110 provides DW I2C/SPI-compatible controllers. Confidence: low. How to verify: VF2 DTB/manual; do not implement before evidence.

## Deviation Log

- 2026-08-19: User supplied the second explicit ABI confirmation required by
  Law 1, unblocking `libs/api` manifest/capability extensions for dedicated
  I2C/SPI MMIO authority instead of reusing the GPIO flag/class.
- Safe slice implemented polling-only BCM BSC1/SPI0 controller cores, exact SoC
  windows, dedicated runtime capability classes, and RPi3 pinmux activation.
  GPIO IRQ ownership, DW-compatible controllers, and physical promotion remain
  gated.
- The pre-existing BCM2837 GPIO grant was narrowed from `0x10000` to `0x1000`;
  the former range overlapped SPI0 and violated the exact-MMIO-authority rule.

## Verification Evidence

- `cargo fmt --all`
- `cargo test -p api -p types --target x86_64-unknown-linux-gnu`
- `cargo test -p driver-i2c-bcm -p driver-spi-bcm --no-default-features --target x86_64-unknown-linux-gnu`
- `cargo check -p sensor-demo -p spi-demo --target aarch64-unknown-none-softfloat`
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3`
- `bash scripts/check-hal-boundaries.sh`
- `bash scripts/check-board-configs.sh`
- `git diff --check`
- `reports/evidence/phase03-rpi3-bcm-hardware-gates-20260820.md` — RPi3 GPIO17→GPIO27, BCM BSC1 NACK, and BCM SPI0 loopback PASS on real hardware.
- `tests/integration/tests/aarch64-boot.rs:161-164` — targeted QEMU AArch64 PL061/PL011/pinned-worker gate PASS.

## Post-Review Fixes

- BCM BSC combined reads now wait for empty-FIFO `TXW` before queueing the write
  bytes and arming `READ|ST`; the regression test asserts that ordering.
- A failed BSC transaction is logged and falls through to the GPIO fallback
  instead of silently reporting only synthetic hardware readings.
- Dedicated I2C/SPI manifest classes and exact RPi3 BSC1/SPI0 allowlists remain
  separate from GPIO authority; the GPIO grant no longer overlaps SPI0.

## Next Steps

Phase 04 handles boot/storage/input/display baseline independently of robot peripheral APIs.
