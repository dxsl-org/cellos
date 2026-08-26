**VERDICT:** PASS — CLEAR; the RPi3-only pinmux fix routes the external SD pins to Arasan before reset/probe without changing pulls or non-RPi targets.

[POSITIVE] kernel/src/task/drivers/mmc/pinmux_rpi3.rs:1 — GPIO base `0x3F20_0000` matches the mapped BCM2837 peripheral window and existing mini-UART GPIO precedent.
[POSITIVE] kernel/src/task/drivers/mmc/pinmux_rpi3.rs:6 — GPFSEL address math uses `GPIO_BASE + (pin / 10) * 4`, yielding GPFSEL3 for GPIO34-39, GPFSEL4 for GPIO48-49, and GPFSEL5 for GPIO50-53.
[POSITIVE] kernel/src/task/drivers/mmc/pinmux_rpi3.rs:12 — bit shifts use `(pin % 10) * 3`, so the RMW masks only each target pin's three-bit function field.
[POSITIVE] kernel/src/task/drivers/mmc/pinmux_rpi3.rs:15 — function select RMW preserves unrelated pins in the same GPFSEL registers; UART GPIO14/15 live in GPFSEL1 and are not touched.
[POSITIVE] kernel/src/task/drivers/mmc/pinmux_rpi3.rs:26 — GPIO34-39 are set to input before GPIO48-53 are routed, disconnecting the Wi-Fi SDIO path from Arasan first.
[POSITIVE] kernel/src/task/drivers/mmc/pinmux_rpi3.rs:29 — GPIO48-53 are set to ALT3 (`0b111`), matching the Arasan external-SD route required by the RPi3 `mmc` overlay behavior.
[POSITIVE] kernel/src/task/drivers/mmc/pinmux_rpi3.rs:10 — volatile read/write is limited to boot-time MMIO; no GPPUD/GPPUDCLK writes are present, so firmware SD pull resistors are preserved.
[POSITIVE] kernel/src/task/drivers/mmc.rs:3 — the quirk module is compiled only for `all(target_arch = "aarch64", feature = "board-rpi3")`, isolating RPi4, VF2, QEMU virt, RV, and x86 builds.
[POSITIVE] kernel/src/task/drivers/mmc.rs:117 — the pinmux call runs after the `SDHCI_BASE == 0` no-op guard and before `EmmcBlock::probe(SDHCI_BASE)`, so reset and CMD traffic use the intended pins.
[POSITIVE] kernel/src/memory/paging.rs:263 — RPi3 maps `0x3F00_0000..0x4000_0000` before driver init, covering both GPIO `0x3F20_0000` and Arasan SDHCI `0x3F30_0000`.
[POSITIVE] tools/rpi3-netboot/test-netboot-scripts.ps1:146 — static guard now checks the pin ranges, ALT3 constant, and absence of `GPPUD`, preventing regression of the narrow routing contract.

Verification: `git diff --check -- kernel/src/task/drivers/mmc/pinmux_rpi3.rs kernel/src/task/drivers/mmc.rs tools/rpi3-netboot/test-netboot-scripts.ps1` PASS; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rpi3-netboot\test-netboot-scripts.ps1` PASS; `cargo check -p cellos-kernel --features board-rpi3 --target aarch64-unknown-none-softfloat` PASS with pre-existing warnings.
