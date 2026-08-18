#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

fail() {
    printf 'HAL boundary violation: %s\n' "$1" >&2
    exit 1
}

if grep -RInE '\bMmioRegion\b|hal_soc_|hal-soc-' boards --include='board.rs' --include='Cargo.toml'; then
    fail 'board packages must not import SoC MMIO types or crates'
fi

if grep -nE 'pub (uart|plic|clint|rtc|virtio_mmio):' boards/src/descriptor.rs; then
    fail 'BoardDescriptor must not own SoC MMIO or IRQ facts'
fi

if grep -n 'feature = "board-' kernel/src/task/drivers/mmc/sdhci.rs; then
    fail 'the shared SDHCI mechanism must select policy at its integration boundary'
fi

if find boards -type f \( \
    -iname '*uart*' -o -iname '*sdhci*' -o -iname '*i2c*' -o \
    -iname '*spi*' -o -iname '*gic*' -o -iname '*plic*' -o \
    -iname '*pcie*' \
\) -print | grep -q .; then
    fail 'board packages must not contain per-board shared-driver copies'
fi

if grep -RInE '0x(0[cC]00_0000|0200_0000|1000_0000|0010_1000|70_4000_0000)' \
    hal/arch/riscv/src --include='*.rs'; then
    fail 'RISC-V SoC base addresses belong under hal/soc/riscv'
fi

if ! grep -q 'pub fallback_mmio: RiscvFallbackMmio' hal/soc/riscv/src/profile.rs; then
    fail 'RISC-V profiles must own audited fallback MMIO'
fi

if ! grep -q 'pub fn has_driver' boards/src/descriptor.rs; then
    fail 'typed driver selection query is missing'
fi

printf 'PASS: HAL/SoC/board boundaries are intact\n'
