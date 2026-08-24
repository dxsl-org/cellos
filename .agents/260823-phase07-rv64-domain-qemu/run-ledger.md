---
goal: "Execute phase-03-domain-user-copy.md from the approved Spec 22 RV64 native-domain plan; scope resolved to chain: verify phase-01 → implement phase-02 → implement phase-03 (deps unclosed at invocation)"
mode: interactive
started: 2026-08-23
phases_cap: 15
tool_call_cap: 400
phases_run: 4
tool_calls_est: 340
baseline_signal: exit-code
---

## Runner Notes

- Kernel is no_std bare-metal; `cargo test -p cellos-kernel` cannot link libtest on any
  target (baseline exit 101, pre-existing). Kernel assertions run as QEMU test hooks via
  `scripts/build-native-domain-test-ci.sh` + `scripts/qemu-native-domain-test.sh`.
- Regression-gate commands per phase:
  1. `bash scripts/check-baseline.sh` (fmt + check + clippy, riscv64 target)
  2. `cargo test -p types --target x86_64-unknown-linux-gnu && cargo test -p api --target x86_64-unknown-linux-gnu` (host libs)
  3. QEMU case runner for the phase's declared cases (exit code)
- ⚠️ test-signal: exit-code only — new-failure detection is best-effort. Compile gates are
  the primary non-QEMU signal for kernel changes.

## Phase Log

| # | Phase | Tier | Status | Result file | Outcome | Residual risk |
|---|-------|------|--------|-------------|---------|---------------|
| 0 | baseline capture | fast | ✅ done | baseline-tests.txt | pre-baseline state recorded | exit-code signal only |
| 1 | RV64 private-root substrate | fast | ✅ done | phase-01-result.md | RV64 AddressSpace verified in QEMU | low |
| 2 | Scheduler domain transitions | thinking | ✅ done | phase-02-result.md | SAS fast-path, SMP migration verified in QEMU | low |
| 3 | Domain-aware user copy | thinking | ✅ done | phase-03-result.md | Checked boundary, trap guard, race verified in QEMU | low |
| 4 | Copied IPC (phase-04) | thinking | ✅ done | phase-04-result.md | Bounded wire buffer, atomic scatter, 1/2-hart QEMU suites PASS | low |
