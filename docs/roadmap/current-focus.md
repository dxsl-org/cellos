# Current Focus

**Last updated**: 2026-08-19

## Active Stage

G1 Robot & Embedded remains the active product stage. The practical focus is
keeping board/HAL/kernel contracts accurate while closing real hardware evidence
without treating QEMU or compile-only checks as board qualification.

## Recent State

- RPi3 post-HAL-split smoke work has landed in `main`.
- HAL to kernel Rust ABI signatures are centralized in
  `hal/traits/arch/src/kernel_abi.rs`.
- Root `boards/` is the owner for board descriptors and fallback assets.
- SoC immutable facts live under `hal/soc/*`.
- Shared drivers remain single-copy in kernel integration paths or
  `cells/drivers/*`; boards do not fork UART, SDHCI, GIC/PLIC, PCIe, or
  DesignWare-style mechanisms.

## Current Documentation Corrections

- MicroPython is historical, not an active workspace runtime.
- Cargo workspace count is generated/discovered data; avoid hardcoding old
  counts except in generated metrics.
- `docs/TODO.md` is no longer project documentation. Personal task tracking
  belongs in `.agents/`.

## Next Useful Work

1. Close remaining hardware-gated board evidence with PASS/FAIL/BLOCKED logs.
2. Decide whether production signing enforcement should become a G1 release
   gate, because default dev builds still admit unsigned cells.
3. Continue reducing kernel-resident legacy driver/orchestration code only when
   a slice has explicit runtime evidence and rollback notes.
4. Keep HAL/board boundary checks in CI whenever board descriptors, SoC facts,
   or HAL ABI hook declarations change.
5. Use [hardware-tracks.md](hardware-tracks.md) and
   [runtime-and-platform-tracks.md](runtime-and-platform-tracks.md) for lane
   status instead of re-reading the archive.
