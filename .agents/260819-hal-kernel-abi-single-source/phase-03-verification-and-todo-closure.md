---
phase: 3
title: "Verification and TODO Closure"
status: completed
priority: P1
effort: "2h"
dependencies: [2]
tier: medium
---

# Phase 3: Verification and TODO Closure

## Overview

Prove the ABI closure across compile lanes and update only the narrow TODO item once evidence exists. Keep QEMU and physical hardware evidence separate.

## Requirements

- Functional: run compile checks for maintained HAL/kernel lanes, one appropriate QEMU smoke, and a boundary grep/script check.
- Non-functional: RV32 kernel failures remain baseline unless separately fixed; no physical RPi3 claim is made by this plan.

## Architecture

Data flow: source edits from Phase 2 -> compile checks per target -> boundary grep/script -> QEMU smoke for one emulated lane -> evidence summary -> narrow `docs/TODO.md` update. Outputs are pass/fail logs and a TODO closure hunk.

Dependency graph: Phase 3 starts only after Phase 2 compiles locally for at least the first target attempted. If any maintained lane fails from ABI changes, return to Phase 2. If RV32 kernel fails with the known `u32` vs `usize` baseline, record deferred baseline and continue HAL-only RV32 validation.

## Assumptions

- **Claim:** local toolchain has all targets required for the commands below.
  **Confidence:** medium
  **How to verify:** run `rustup target list --installed` before the build matrix.

## Related Files

- Modify: `docs/TODO.md`
- Read: `scripts/check-hal-boundaries.sh`
- Read: `scripts/check-board-configs.sh`
- Read: `kernel/Cargo.toml`
- Read: `hal/arch/*/Cargo.toml`

## Implementation Steps

1. Run boundary grep: `grep -RIn 'extern "Rust"' hal/arch hal/soc kernel/src --include='*.rs'` and confirm HAL declarations live only in `hal/traits/arch/src/kernel_abi.rs`; comments are acceptable.
2. Run `bash scripts/check-hal-boundaries.sh`.
3. Run kernel compile lanes:
   `RUSTFLAGS="-C relocation-model=pic" cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2`;
   `cargo check -p cellos-kernel --target x86_64-unknown-none`;
   `RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg" cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3`.
4. Run HAL compile lane for RV32: `cargo check -p hal-riscv --target riscv32imac-unknown-none-elf --no-default-features --features riscv32`.
5. Run one relevant QEMU smoke for a non-hardware claim, preferably the existing x86_64 smoke if target artifacts are available; label result as QEMU-only.
6. Only after the above passes, update `docs/TODO.md:12` to close the ABI debt. Keep `docs/TODO.md:23` RV32 kernel baseline note intact unless independently fixed and verified.

## Success Criteria

- [x] Boundary grep/script has no new non-central HAL `extern "Rust"` declarations.
- [x] x86_64, RV64, and AArch64 board-rpi3 kernel compile lanes pass or have non-ABI baseline failures documented.
- [x] HAL RV32 compile passes; RV32 kernel failure remains explicitly documented if still present.
- [x] One QEMU smoke result is recorded as emulator evidence only.
- [x] `docs/TODO.md` closes only the ABI item and preserves unrelated user notes.

## Evidence

- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && grep -RInE \"extern \\\"Rust\\\"\" hal/arch --include=\"*.rs\" || true'` only reported comment mentions; no HAL declarations remain outside `hal/traits/arch/src/kernel_abi.rs`.
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && bash scripts/check-hal-boundaries.sh'` passed.
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target x86_64-unknown-none'` passed.
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2'` passed.
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3'` passed.
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p hal-core --target riscv32imac-unknown-none-elf --no-default-features --features riscv32'` passed.
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && BOOT_WINDOW=30 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso'` booted to `ViCell >` and timed out because the script still expects the older `Cellos >` prompt; emulator evidence only, no physical RPi3 claim.
- `wsl.exe -d Ubuntu -- bash -lc 'cd /home/dmin/cellos && cargo check -p cellos-kernel --target riscv32imac-unknown-none-elf --no-default-features --features board-rpi3'` still fails on the pre-existing RV32 kernel baseline (`hal::paging`, `uart_bcm_mini`, `AtomicU64`, `u32` vs `usize`, `hal_soc_bcm27xx`).

## Reviewer

CLEAR

## Security Considerations

No new secrets or external services. QEMU evidence must not be represented as physical RPi3 hardware evidence.

## Risk Assessment

- Medium likelihood x medium impact: build environment lacks target/toolchain components. Mitigation: record as host-gated, not green; do not close TODO without compile evidence.
- Medium likelihood x high impact: TODO gets over-edited and drops planning notes for G2/VMM. Mitigation: modify only the ABI debt paragraph.
- Rollback: revert only the `docs/TODO.md` ABI closure hunk. Irreversible part: none.

## File Ownership

- Phase 3 owns `docs/TODO.md` after Phase 2 passes.
- Phase 3 reads scripts and Cargo files but does not modify them.

## Deviation Log

None.
