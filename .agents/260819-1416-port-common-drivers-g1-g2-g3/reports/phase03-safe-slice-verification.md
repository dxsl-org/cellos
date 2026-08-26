# Phase 03 BCM safe-slice verification

Status: compile/test complete; runtime and physical promotion blocked by capability and hardware gates.

Implemented:

- BCM2837 BSC1 and SPI0 facts in the SoC profile and RPi3 descriptor/DTS.
- Polling-only BCM BSC I2C core with bounded waits, repeated-start, NACK,
  timeout, cleanup, and transfer-size tests.
- Polling-only BCM SPI0 Mode 0 core with bounded waits, explicit chip-select
  lifetime, cleanup-on-error, and transfer-size tests.
- GPIO MMIO grant narrowed to `0x1000`, eliminating the prior overlap with
  SPI0 at `0x3f204000`.

Passed in WSL:

- `cargo fmt --all --check`
- `cargo test -p driver-i2c-bcm -p driver-spi-bcm --no-default-features --target x86_64-unknown-linux-gnu` (5 I2C + 6 SPI)
- `cargo check -p driver-i2c-bcm -p driver-spi-bcm --target aarch64-unknown-none-softfloat`
- `cargo test -p hal-soc-bcm27xx -p cellos-boards --target x86_64-unknown-linux-gnu` (6 SoC + 12 board)
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3`
- `bash scripts/check-hal-boundaries.sh`
- `bash scripts/check-board-configs.sh` (exit 0; conflicting-board failures are expected negative checks)
- `git diff --check`

Known unrelated host-test limitation:

- `cargo test -p hal-core --no-default-features --features x86_64 --target x86_64-unknown-linux-gnu`
  reaches the linker and fails on the pre-existing duplicate `_start` test-harness conflict.

Remaining gates:

- No I2C/SPI capability bits or runtime resource allowlist was added, so
  `open()` remains fail-closed.
- GPIO edge IRQ ownership, RPi3 sensor/actuator wiring, repeated-start against
  a real device, SPI CS/polarity, and IRQ delivery remain hardware-gated.
- DesignWare controllers remain blocked until a target DT provides verified
  compatible/MMIO/IRQ facts.
