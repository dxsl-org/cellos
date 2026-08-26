# Scout Report — RPi3 Board Descriptor

- OBSERVED: `BoardDescriptor` currently requires UART, PLIC, CLINT, and RTC even though RPi3 lacks the latter three.
- OBSERVED: RPi3 fallback memory is duplicated in `kernel/src/boot.rs`; platform UART/absence data is embedded in `kernel/src/platform.rs`.
- OBSERVED: `hal/soc/bcm27xx` already owns BCM2837 controller layout and SDHCI constraints, so the new descriptor must not duplicate mechanism or policy ownership.
- OBSERVED: `kernel/src/board.rs` selects only the QEMU RV64 descriptor today.
- OBSERVED: `c0096ade` established root `boards/` as the board-data boundary; `9372d870` and the current BCM27xx slice established `hal/soc` as SoC-policy data.
- PRIOR: RPi3 physical behavior is not revalidated by this refactor and remains outside the evidence boundary.
