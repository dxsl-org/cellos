# D1b — IPC p99 target semantics

**Status:** approved for Part 6 application.

## Finding

The 50 us p99 target is stated as a product requirement, but the scheduled workflow runs only
QEMU TCG and fails whenever any `[bench]` line contains `FAIL`. Therefore the emulator currently
acts as a hardware qualification gate despite docs saying it is trend evidence. The same
workflow already has a separate sustained-regression detector: more than 10% above historical
median for three consecutive runs.

Measured QEMU runs have crossed both sides of 50 us (48.5 us p50/86.6 us p99 in D1, and 104 us
p99 in the changelog), so treating 50 us as a deterministic TCG ceiling is not supported.

## Recommended ruling [FINAL]

**The 50 us p99 requirement is a qualified-hardware target, not a QEMU-TCG release gate.**

1. Keep the target unchanged and require a named hardware/clock/build profile for qualification.
2. In scheduled QEMU CI, record p99 and gate sustained relative regressions through
   `compare-bench-results.sh`; a miss against 50 us is `HW-TARGET-MISS`, not `FAIL`.
3. Keep functional completion, deadline misses, and the other explicitly QEMU-calibrated
   thresholds fail-closed.
4. Do not invent a new absolute QEMU ceiling from two noisy samples.
