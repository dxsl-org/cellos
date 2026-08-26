# Scout Report

## Findings

- Root `boards/` contains QEMU RV64 and RPi3 only, while kernel features also select VF2, Pioneer, and RPi4; QEMU AArch64 remains an implicit default.
- `kernel/src/boot.rs` duplicates the VF2 and QEMU AArch64 fallback maps instead of consuming descriptors.
- QEMU AArch64 PL011, GIC, VirtIO, RTC, PCIe, paging, and grant facts remain spread across ARM HAL and kernel modules.
- The shared SDHCI driver still uses `board-rpi3` cfg fields/methods and `mmc.rs` embeds RPi4/JH7110 controller bases.
- `enabled_drivers` is untyped string data and does not currently drive initialization.
- Shared driver files are single-copy, but several Driver Cells choose MMIO by architecture literals; those are follow-on consumers of selected platform data, not reasons to copy drivers.

## Precedents

- `c0096ade` introduced the board descriptor layer.
- `19728a70` connected board and SoC policies.
- `5a5342e8`, `0690b9ad`, and `5513e5cd` closed BCM topology and duplicated consumer mechanisms incrementally.

## Blast Radius

Board catalog, early boot/platform selection, AArch64 QEMU layout, shared SDHCI construction, Cargo feature compatibility, and living docs. Public Cell ABI is excluded.

## Prior Failures

No matching failure ledger or incident report exists. Physical-board runtime evidence remains host-gated.
