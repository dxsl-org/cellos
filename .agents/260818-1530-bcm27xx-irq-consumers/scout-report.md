# Scout Report

## Relevant Files

- `hal/arch/arm/src/aarch64/bcm2835_legacy_irq.rs`: GPIO pending-bank identification still embeds bits 17 and 18.
- `hal/arch/arm/src/aarch64/timer.rs`: RPi3 CNTP routing still embeds local-source bit 1.
- `kernel/src/main.rs`: RPi3 diagnostic still embeds two controller addresses and two IRQ masks.

## Boundary

Numbers, controller bases, and routed-source masks are SoC facts. Register offsets, bank selection, MMIO access, diagnostic formatting, timer period, and acknowledgement behavior remain consumer mechanisms.

## Precedents

- `5a5342e8`: introduced the checked BCM27xx IRQ topology and compatible public aliases.
- `546f4de5`: routed ARM HAL controller bases through the SoC profile.
- `5db526b2`: centralized BCM2837 MMIO policy with host tests.

## Prior Failures

No matching `.agents/failure-history.jsonl` or incident report exists.

## Blast Radius

Only the three listed consumer files and living docs. Public HAL constants and kernel/HAL APIs remain unchanged.

## Deferred Debt

Timer-frequency policy and UART debug MMIO in `kernel/src/task.rs` are intentionally out of scope.
