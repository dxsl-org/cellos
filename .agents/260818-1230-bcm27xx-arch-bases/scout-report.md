# Scout Report — BCM27xx Arch Bases

- OBSERVED: `uart_bcm_mini.rs` repeats AUX and GPIO bases already held by `BCM2837.mmio`.
- OBSERVED: legacy IRQ and system timer modules repeat two controller bases not yet represented in the profile.
- OBSERVED: `bcm2836_irq.rs` repeats the existing local-controller base.
- OBSERVED: RPi3 timer diagnostics use raw mini-UART, system-timer, and IRQ-pending addresses.
- OBSERVED: `hal/core` already propagates `board-rpi3` to `hal-arm`; `hal-arm` can activate a target-scoped optional SoC-data dependency without a cycle.
- PRECEDENT: `9b4aeead` introduced the mechanisms and `a84b9fc3` hardened physical-board bring-up; neither ownership nor register behavior moves in this slice.
