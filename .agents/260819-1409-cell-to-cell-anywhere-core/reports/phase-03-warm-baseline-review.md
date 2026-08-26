---
title: "Phase 03 Warm Baseline Review"
status: complete
created: 2026-08-19
verdict: pass_with_risk
---

**VERDICT:** PASS_WITH_RISK - calibration behavior is production-safe for the benchmark oracle, with one non-blocking file-size standards issue.

[LOW] cells/tests/bench/src/scenarios/c2c_broker_oracle_orchestrator.rs:200 - file is exactly 200 lines, while AGENTS asks code files stay under 200 lines. Move one small helper/import block into an existing submodule before commit if enforcing the rule strictly.
[POSITIVE] cells/tests/bench/src/scenarios/c2c_broker_oracle_client/calibration.rs:64 - measured sample count is capped at 1000, warmup is excluded, and zero-sample input returns a default summary instead of indexing an empty sample set.
[POSITIVE] cells/tests/bench/src/scenarios/c2c_broker_oracle_client/calibration.rs:96 - timing-invalid samples are classified separately and block MEASURED output without hiding the transport success count.
[POSITIVE] cells/tests/bench/src/scenarios/c2c_broker_oracle_client/calibration.rs:149 - timestamp monotonicity is checked across send, worker completion, reply pump, and client wake before latency fields are trusted.
[POSITIVE] cells/tests/bench/src/scenarios/c2c_broker_oracle_client/calibration.rs:162 - p50 and p99 use the same index convention as BenchReport::from_sorted for 1000 samples.
[POSITIVE] cells/tests/bench/src/scenarios/c2c_broker_oracle_report/calibration.rs:8 - baseline reporting requires 1000 measured samples, zero warmup failures, zero timing-invalid samples, and exact broker accepted/completed deltas; it does not introduce an arbitrary broker latency pass/fail ceiling.
[POSITIVE] cells/tests/bench/src/scenarios/c2c_broker_oracle_wire.rs:125 - summary encode/decode remains fixed-width and bounded for the private client-orchestrator wire.

Verification:
- cargo fmt --all -- --check: PASS
- cargo test -p service-net-broker --lib --target x86_64-unknown-linux-gnu: PASS, 54/54
- RUSTFLAGS="-D warnings" cargo check -p service-net-broker --target riscv64gc-unknown-none-elf: PASS
- RUSTFLAGS="-D warnings" cargo check -p app-bench --target riscv64gc-unknown-none-elf: PASS
- git diff --check: PASS
- retained QEMU warm baselines 01..03: calibration=MEASURED, 1000/1000 success, 0 warmup_failures, 0 timing_invalid, 0 heartbeat/watchdog expiry
