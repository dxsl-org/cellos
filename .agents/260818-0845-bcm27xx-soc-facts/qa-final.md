# Final QA — BCM27xx SoC Facts

## Verdict

All 12 gates passed against the final uncommitted source at `HEAD c6a31372`.

## Passing Gates

1. `cargo fmt --all -- --check`
2. `cargo test -p hal-soc-bcm27xx --target x86_64-unknown-linux-gnu` — 2 passed
3. `cargo test -p hal-soc-riscv --target x86_64-unknown-linux-gnu` — 3 passed
4. `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu` — 8 passed
5. RV64 kernel check, default features
6. RV64 kernel check, `board-vf2`
7. RV64 kernel check, `board-pioneer`
8. RV64 kernel check, combined `board-vf2 board-pioneer`
9. AArch64 kernel check, default features
10. AArch64 kernel check, `board-rpi3`
11. RV64 release kernel build with `-Z build-std=core,alloc`
12. QEMU boot — `PASS: FAT16 mounted — kernel booted (no disk)`

## Review

The reviewer found one target-gating mismatch. The RPi3 SDHCI constant now uses
the same AArch64 target guard as the BCM27xx dependency; final re-review passed.

## Evidence Boundary

QEMU RV64 runtime is verified. VF2, Pioneer, and RPi3 remain compile-only for
this slice; no new physical-board runtime claim is made.
