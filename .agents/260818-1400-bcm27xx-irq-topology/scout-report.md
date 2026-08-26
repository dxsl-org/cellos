# Scout Report — BCM27xx IRQ Topology

- OBSERVED: `bcm2835_legacy_irq` owns legacy IRQ numbers 1/29/49/50 as literals or shift operands.
- OBSERVED: `bcm2836_irq` owns Core0 source masks for timer NS, timer HP, and GPU pass-through.
- OBSERVED: trap and UART paths consume public ARM HAL constants, so aliases preserve their contract.
- OBSERVED: `hal-arm/board-rpi3` already activates `hal-soc-bcm27xx`; no new dependency direction is required.
- BOUNDARY: register offsets and interrupt mechanisms remain in ARM HAL; only immutable topology moves.
