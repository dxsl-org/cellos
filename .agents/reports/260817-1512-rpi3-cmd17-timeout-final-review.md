**VERDICT:** PASS_WITH_RISK — implementation is correct, but the static guard does not enforce the required timeout-write placement.

[MED] tools/rpi3-netboot/test-netboot-scripts.ps1:149 — guard only checks that `write8(SDHCI_TIMEOUT_CONTROL, TIMEOUT_MAX)` exists, so moving it after `BLOCK_SIZE`/`BLOCK_COUNT`/`TRANSFER_MODE` would still pass. fix: make the regex assert timeout write precedes the three setup writes in `setup_data_transfer`.
[POSITIVE] kernel/src/task/drivers/mmc/regs.rs:18 — `SDHCI_TIMEOUT_CONTROL` uses offset `0x2E`, the timeout byte in the `0x2C..0x2F` control word.
[POSITIVE] kernel/src/task/drivers/mmc/regs.rs:57 — `TIMEOUT_MAX = 0x0E` selects the longest standard SDHCI data-timeout exponent without using the reserved `0x0F` value.
[POSITIVE] kernel/src/task/drivers/mmc/sdhci.rs:113 — on RPi3, the byte write to offset `0x2E` is promoted through the existing 32-bit read-modify-write helper, preserving clock-control and soft-reset neighbor bytes.
[POSITIVE] kernel/src/task/drivers/mmc/sdhci.rs:119 — the RPi3 RMW path still goes through `write32`, so BCM2835 write spacing and `last_write_ticks` accounting remain applied.
[POSITIVE] kernel/src/task/drivers/mmc/sdhci.rs:424 — all data-transfer callers route through one setup helper, covering EXT_CSD, SD CMD17/CMD24, and eMMC CMD17/CMD24.
[POSITIVE] kernel/src/task/drivers/mmc/sdhci.rs:432 — timeout is programmed before block size, block count, and transfer mode, matching the intended pre-data-command placement.

Verification: `git diff --check -- kernel/src/task/drivers/mmc/regs.rs kernel/src/task/drivers/mmc/sdhci.rs tools/rpi3-netboot/test-netboot-scripts.ps1` PASS; `cargo check -p cellos-kernel --features board-rpi3 --target aarch64-unknown-none-softfloat` PASS with pre-existing warnings; `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat` PASS with pre-existing warnings; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rpi3-netboot\test-netboot-scripts.ps1` PASS.
