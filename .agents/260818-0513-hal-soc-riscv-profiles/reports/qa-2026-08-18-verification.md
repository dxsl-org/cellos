# Test Report — 2026-08-18 — HAL SoC / board profile verification

## Test Results Overview
- Total checks: 11
- Passed: 11 | Failed: 0 | Skipped: 0
- Duration: single sequential WSL shell

## Build Status
- `cargo fmt --all -- --check`: PASS
- `cargo test -p hal-soc-riscv --target x86_64-unknown-linux-gnu`: PASS (`2/2`)
- `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu`: PASS (`8/8`)
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf`: PASS
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2`: PASS
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-pioneer`: PASS
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features 'board-vf2 board-pioneer'`: PASS
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat`: PASS
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3`: PASS
- `cargo build --release -p cellos-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc`: PASS
- `bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`: PASS

## Warnings
- `cargo check`/`cargo build` for `cellos-kernel` still emit the existing strip warnings for `init` and `kernel_fs.img`
- `cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3` still emits 5 HAL ARM dead-code warnings

## Scope Guard
- `git rev-parse --short=8 HEAD`: `9427482f`
- `git diff --name-only HEAD`: `Cargo.toml`, `kernel/Cargo.toml`, `kernel/src/platform.rs`
- Result: HEAD stayed fixed and the uncommitted HAL SoC diff remained the only tracked scope change

## Failed Tests
- None

## Unresolved Questions
- None for the verified path
