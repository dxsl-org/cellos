# Test Report — 2026-08-17 — HAL/board split verification

## Test Results Overview
- Total gates checked in this rerun: 10
- Passed: 9 | Failed: 0 | Skipped: 1
- Duration: multi-step WSL session
- Prior verification pass also covered host `cellos-boards` coverage and the workspace RV64 coverage harness.

## Coverage Metrics
- Host coverage for `cellos-boards`: 82.14% line coverage (46/56) — PASS against 80% default, measured in the prior verification pass
- Workspace RV64 coverage script: blocked in the prior verification pass; not rerun in this final pass per instruction

## Build Status
- `cargo fmt --all -- --check`: PASS
- `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu`: PASS (`8/8`)
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf`: PASS
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2`: PASS
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-pioneer`: PASS
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat`: PASS
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3`: PASS
- `cargo build --release -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc`: PASS
- `bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`: PASS
- `dtc -I dts -O dtb boards/qemu/virt-riscv64/qemu-virt-riscv64.dts`: SKIPPED (`dtc` not installed)
- Warnings: kernel build could not strip `init` and `kernel_fs.img`; `board-rpi3` check warned about unused HAL ARM constants/functions

## Failed Tests
### `bash scripts/measure-coverage.sh`
- Error: `error[E0463]: can't find crate for 'profiler_builtins'`
- Error: `error[E0152]: duplicate lang item in crate 'core': 'sized'`
- Cause: repo-wide RV64 `cargo llvm-cov` path is not currently usable under this toolchain/layout; coverage report did not complete
- Fix: none applied in this verification pass; host coverage for `cellos-boards` was measured separately

## Critical Issues
1. Full workspace RV64 coverage measurement is currently blocked by `cargo llvm-cov`/target interaction, so the repo-wide coverage gate still needs a follow-up.

## Recommendations
1. If you want a single green coverage gate, split host-runnable crate coverage from no_std RV64 kernel coverage or adjust the coverage harness for the `build-std` target path.
2. Keep the new `cellos-boards` tests as the regression seam for descriptor validation and board metadata.

## Unresolved Questions
- None for the verified build/test path.
