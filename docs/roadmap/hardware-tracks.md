# Hardware Tracks

**Last updated**: 2026-08-19

This page collects the hardware qualification lanes that matter for roadmap
reading. For the architecture split between board descriptors, SoC facts, and
shared drivers, see [system-architecture.md](../system-architecture.md).

## Board and SoC Ownership

- Root `boards/` owns board descriptors and audited fallback assets.
- `hal/soc/*` owns immutable SoC facts.
- Shared drivers stay single-copy in `cells/drivers/` or the relevant kernel
  integration path; boards do not fork UART, SDHCI, GIC/PLIC, PCIe, or similar
  mechanism code.

## Current Qualification Lanes

- RPi3 physical smoke is merged and should be treated as real hardware evidence.
- VF2, Pioneer, and RPi4 remain physical-only qualification lanes unless a log
  explicitly records PASS/FAIL/BLOCKED evidence.
- QEMU and compile-only checks are regression evidence, not board qualification.

## Placeholder-Only Board Entries

- `q35-x86_32`
- `virt-riscv32`
- `virt-aarch32`

These entries exist for documentation and future expansion, not as active
hardware claims.

## Shared-Lane Rule

If a hardware change affects a common peripheral mechanism, update the shared
driver or HAL layer once rather than cloning the behavior per board.
