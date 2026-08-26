# Scout Report — BCM27xx SoC Facts

- OBSERVED: `kernel/src/platform.rs` embeds the BCM2837 mini-UART IO address and peripheral-base log text in the RPi3 path.
- OBSERVED: `kernel/src/task/drivers/mmc.rs` embeds the BCM2837 Arasan base while `pinmux_rpi3.rs` embeds the GPIO controller base.
- OBSERVED: `sdhci.rs` keeps shared register access but embeds the BCM2835 six-microsecond write-spacing policy.
- OBSERVED: root `boards/` currently has only a QEMU RV64 descriptor; its mandatory PLIC/CLINT/RTC fields make an RPi3 descriptor a separate schema slice.
- OBSERVED: `.agents/260817-hal-soc-bcm27xx-slice/plan.md` says phase 1 completed although no `hal/soc/bcm27xx` exists; this plan supersedes that stale status.
- PRIOR: RPi3 physical behavior remains hardware-gated; compile and QEMU evidence must not be promoted to hardware evidence.

## Precedent

- `9372d870 refactor(hal): add RISC-V SoC profiles` established the data-only `hal/soc/<family>` pattern.
- `c0096ade refactor(hal): add board descriptor layer` established that board identity, boot, fallback memory, wiring, and enabled-driver data stay in root `boards/`.
