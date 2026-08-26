# Phase 03-04 Execution Sync

Status: plan artifacts updated to match the approved implementation slice; Phase 03 and Phase 04 remain `in_progress`.

## Scope update

- Branch recorded as `feat/common-drivers-g1-g2-g3`.
- Scope shifted from roadmap-only language to approved implementation execution with separate promotion gates.

## Phase 03 evidence

- The safe slice is implemented for dedicated I2C/SPI capability bits, exact MMIO allowlists, BCM BSC1/SPI0 controller cores, and RPi3 pinmux wiring.
- Verification ledger already covers `cargo fmt --all`, `cargo test -p api -p types --target x86_64-unknown-linux-gnu`, `cargo test -p driver-i2c-bcm -p driver-spi-bcm --no-default-features --target x86_64-unknown-linux-gnu`, `cargo check -p sensor-demo -p spi-demo --target aarch64-unknown-none-softfloat`, `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3`, `bash scripts/check-hal-boundaries.sh`, `bash scripts/check-board-configs.sh`, and `git diff --check`.
- Post-review fixes recorded in the phase file: dedicated capability flags, exact RPi3 windows, and narrowed GPIO grant.

## Phase 04 evidence

- Evidence scripts were added for boot marker checking: `scripts/assert-boot-markers.sh`, `scripts/qemu-boot-test.sh`, and `scripts/qemu-aarch64-test.sh`.
- Live QEMU promotion evidence is now recorded for RV64 and AArch64, including the optional GPU/NIC omission gate. Logs are retained under `reports/evidence/phase04-rv64-baseline.log`, `reports/evidence/phase04-aarch64-baseline.log`, `reports/evidence/phase04-rv64-without-optional.log`, and `reports/evidence/phase04-aarch64-without-optional.log`.
- The current-head RPi3 payload is built, wrapped, and staged for TFTP, but the physical board run is still blocked by the disconnected host Ethernet link and unreachable `192.168.42.2`.

## Remaining gates

- Phase 03 GPIO IRQ and physical RPi3 wiring proof.
- Phase 04 physical RPi3 storage/input boot evidence.
