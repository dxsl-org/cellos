## Summary
- Centralize HAL<->kernel Rust ABI hook signatures in hal-arch-trait.
- Replace arch-local extern Rust declarations with shared imports.
- Add kernel-side compile-time signature assertions for exported hooks.
- Update HAL boundary guard and docs for the single-source ABI contract.

## Linked Issues
No linked issues.

## Pre-Landing Review
Pre-Landing Review: 2 issues (0 critical, 2 informational)

- [hal/traits/arch/src/kernel_abi.rs:55] Frame assertions cover size but not individual assembly offsets.
  Fix: add offset assertions before future frame-layout work.
- [riscv32 compile] RV32 remains blocked by pre-existing mainline compile failures.
  Fix: track separately before RV32 hardware work.

## Test Results
- [x] cargo fmt --all --check
- [x] bash scripts/check-hal-boundaries.sh
- [x] bash scripts/check-board-configs.sh (includes expected fail-closed conflicting-board checks)
- [x] cargo check -p cellos-kernel --target x86_64-unknown-none
- [x] cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf
- [x] cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3
- [ ] cargo check -p cellos-kernel --target riscv32imac-unknown-none-elf remains baseline-failing on branch and origin/main: hal::paging, AtomicU64, and u32/usize errors.

## Changes
23 files changed, 290 insertions(+), 214 deletions(-)

## Ship Mode
- Mode: official
- Target: main
