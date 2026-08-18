# Raspberry Pi 3 Model B

This directory contains audited fallback configuration for the BCM2837-based
Raspberry Pi 3 Model B. VideoCore firmware normally supplies the live device
tree; the checked-in DTS documents the fallback contract and is not claimed as
a generated or compiled boot artifact.

Board data owns identity, firmware/boot expectations, fallback RAM, pinmux
group names, and enabled shared drivers. BCM2837 controller layout and SDHCI
access constraints remain in `hal/soc/bcm27xx`.

```sh
cargo build -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3
```
