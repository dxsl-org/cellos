# Scout Report — BCM27xx MMIO Policy

- OBSERVED: `hal/soc/bcm27xx` already owns BCM2837 peripheral/GPIO/AUX bases but not mapping spans or grant widths.
- OBSERVED: `kernel/src/memory/paging.rs` repeats peripheral and local-controller bounds while applying intentionally different flags.
- OBSERVED: `kernel/src/resource_registry.rs` and `kernel/src/task/drivers/gpio_irq.rs` repeat the GPIO base introduced together by `6036f2dda`.
- OBSERVED: paging literals originated in `9b4aeead9` and USER access was added by `2830f767b`; permission semantics must not change.
- PRIOR: `19728a70` established `hal/soc/bcm27xx` as the data-only SoC-facts boundary.
- BOUNDARY: IRQ/timer drivers remain in `hal/arch/arm`; board wiring remains in root `boards/`.
