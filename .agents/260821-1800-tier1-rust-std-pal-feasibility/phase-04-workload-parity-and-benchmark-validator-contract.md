---
phase: 4
title: "Workload Parity and Benchmark Validator Implementation"
status: "FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED"
priority: P1
effort: 1.5d
dependencies: [1]
tier: thinking
---

# Phase 04: Workload Parity and Benchmark Validator Implementation

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs. Never relax a sample, drift, regression, or interference gate as a deviation.

## Overview

Freeze byte-for-byte comparable no-std/std syscall and IPC workloads, then implement a fail-closed promotion validator and behavioral tests over synthetic fixtures only. This phase adds no benchmark binaries, live capture path, ledger writer, or promotion evidence.

## Requirements

- Workloads: `syscall-yield-v1` performs exactly one Yield round trip; `ipc-echo-64-v1` sends one fixed 64-byte ping and receives one fixed 64-byte reply from the same pinned private echo peer.
- Exact cross-arm parity tuple: `(architecture, environment_kind, board_model, board_revision, qemu_binary_digest, qemu_version, machine, firmware_digest, cpu_model, cpu_count, hart_count, frequency_policy, timer_source, timer_frequency_hz, build_profile, rustc_commit, rust_src_digest, target_spec_digest, source_revision, common_codegen_flags_digest, common_linker_inputs, common_linker_inputs_digest, admission_manifest_digest, capability_manifest_digest, service_topology_digest, service_state_digest, workload_id, workload_version, payload_digest, operation_trace_digest)`.
- Common and runtime linker inputs are explicit closed ordered `(role, identity, digest)` manifests with canonical derived digests. Runtime manifests equal pinned per-runtime fixture allowlists; additions, omissions, reordering, swaps, arbitrary digest exemptions, and mlibc/POSIX/libc/host/instrumentation identities are invalid.
- Protocol: physical source order is `no_std_pre/1 → std/2 → no_std_post/3`, never sort-repaired; UTC `captured_at` strictly increases within each triple. Each arm has at least 5 discarded warmups and at least 30 independent measured repetitions. Each repetition retains raw latency.
- Retain raw samples plus p50/p95/p99; any true interference flag or rejection record invalidates the whole fixture/cohort before statistics. Pre/post no-std p99 drift must be ≤2%; std p99 regression must be ≤5% for every workload in every cell.
- This slice accepts only `source_kind="synthetic_fixture"` and emits `promotion_eligible=false`; live/captured inputs fail closed.

## Architecture

`artifacts/workload-parity-spec.md` freezes operation traces and allowed differences. `scripts/rust_std_promotion/benchmark-run.schema.json` is the implemented input schema; `scripts/rust_std_promotion/validator.py` performs deterministic validation; the CLI emits canonical fixture-only reports.

### Run schema

- `schema_version`, `run_id`, strict UTC `captured_at`, `cell_id`, `workload_id`, `workload_version`, `arm` (`no_std_pre|std|no_std_post`), `arm_order`.
- `toolchain {channel, rustc_version, commit_hash, rust_src_digest}`, `source_digest`, `binary_digest`, `runtime_kind`, `build_profile`, `codegen_flags_digest`, closed `common_linker_inputs[]`/`runtime_linker_inputs[]` manifests and their derived digests, admission/capability manifest digests.
- `environment {architecture, target_spec_digest, board, qemu_version, firmware_digest, cpu_count, hart_count, frequency_policy, timer_source, timer_frequency, service_topology_digest, service_state_digest}`.
- `protocol {warmup_count, independent_rep_count, operations_per_rep, reset_rule, predeclared_interference_codes}`.
- `repetitions[] {rep_id, fresh_instance_id, raw_latency_ns, monotonic_clock, interference}`; every interference Boolean must remain false or the whole document is invalid.
- `rejections[]` is schema-closed to zero entries; `summary {valid_n, p50_ns, p95_ns, p99_ns}` and `provenance {producer, schema_digest, raw_digest}` bind all repetitions.

The schema also requires `source_kind="synthetic_fixture"` and `fixture_id`; it forbids unknown fields so live provenance cannot be smuggled into this slice.

### Validator contract

1. Reject schema/provenance mismatch, duplicate IDs, nonpositive latency/timer frequency, changed parity fields, non-UTC/non-increasing timestamps, nonphysical arm/cell order, or fewer than 5 warmups/30 repetitions per arm. Never sort input to repair it.
2. Reject the entire document if any interference Boolean is true or any rejection record exists. No selective deletion, post-hoc outlier trimming, percentile winsorizing, std-specific rejection, or relabeling runtime overhead as noise exists.
3. Recompute and verify closed common/runtime linker-input manifests and digests against pinned per-runtime fixture allowlists. Reject additions, omissions, reordering, role/digest swaps, arbitrary digests, and forbidden mlibc/POSIX/libc/host/instrumentation inputs.
4. Compute percentile by nearest rank on sorted raw values: index `ceil(q*n)-1` for q = 0.50, 0.95, 0.99.
5. Compute `drift_pct = 100 * abs(post_p99 - pre_p99) / pre_p99`; require `pre_p99 > 0` and `drift_pct ≤ 2.0`.
6. Pool pre/post no-std raw samples for `baseline_p99`. Compute `regression_pct = 100 * (std_p99 - baseline_p99) / baseline_p99`; require `baseline_p99 > 0` and `regression_pct ≤ 5.0` for each workload/cell.
7. Emit per-cell reasons and only `VALID_PASS|VALID_FAIL|INVALID`; `INVALID` never promotes. Preserve canonical raw input and validator/schema digests.
8. Preserve validated physical report order, sort reason codes, serialize canonical JSON with sorted keys and fixed integer formatting, and omit wall-clock time, host paths, and nondeterministic IDs. Exit `0`/`1`/`2` for `VALID_PASS`/`VALID_FAIL`/`INVALID`.
9. Set `fixture_only=true` and `promotion_eligible=false` in every report; reject any non-fixture input before statistics are computed.

## Assumptions

- **Claim:** Fresh benchmark-process/cell instances can provide independent repetitions without changing the pinned service topology.
  **Confidence:** medium
  **How to verify:** Define and review a reset rule that proves instance freshness while retaining equivalent peer/service state.
- **Claim:** The timer source can report stable nanosecond conversion metadata on each intended cell.
  **Confidence:** medium
  **How to verify:** Record per-environment timer source/frequency and reject environments lacking trustworthy conversion.

## Related Files

- Read only: `cells/tests/bench/src/scenarios/syscall_yield.rs`, `cells/tests/bench/src/scenarios/ipc_send_recv.rs`, `cells/tests/bench/src/framework/{runner.rs,report.rs,timer.rs}`, `libs/api/src/services/benchmark.rs`, `scripts/compare-bench-results.sh`, `docs/performance-report.md`
- Create: `scripts/rust_std_promotion/__init__.py`, `scripts/rust_std_promotion/validator.py`, `scripts/rust_std_promotion/benchmark-run.schema.json`, `scripts/validate-rust-std-promotion.py`
- Create: `tests/rust-std-promotion/test_validator.py`
- Create fixtures: `tests/rust-std-promotion/fixtures/{valid-pass,invalid-warmups,invalid-repetitions,invalid-parity,invalid-drift,valid-fail-regression,invalid-interference,invalid-raw-samples}.json`
- Create expected reports: `tests/rust-std-promotion/fixtures/{expected-valid-pass,expected-valid-fail-regression}.report.json`
- Create plan artifacts: `artifacts/workload-parity-spec.md`, `artifacts/benchmark-validator-contract.md`, `approvals/benchmark-contract.md`

## Implementation Steps

1. Freeze operation traces, payload bytes, peer lifecycle, timer boundaries, excluded setup, error handling, the exact parity tuple, and allowed variant fields.
2. Implement the closed JSON schema with raw repetitions, provenance, explicit closed linker manifests, all-false interference metadata, zero permitted rejection records, and fixture-only source classification.
3. Implement pure validator logic and a thin CLI with deterministic percentile, drift, regression, whole-document interference invalidation, strict source order/time, closed linker allowlists, parity, report-order, and exit-status behavior; provide no capture or ledger-write API.
4. Add behavioral tests for valid pass, syntactically valid `>5%` regression failure, warmup/repetition/raw boundaries, tuple mismatch, >2% drift, any interference/rejection, per-cell non-aggregation, percentile boundaries, no arm-order repair, equal/reversed/non-UTC timestamps, linker additions/omissions/swaps/forbidden identities, deterministic reports, and non-fixture rejection.
5. Pin fixture and expected-report bytes in tests, content-address the canonical approval-input manifest, then obtain performance-owner and independent measurement-reviewer approval.

## Success Criteria

- [x] Schema and validator enforce the exact parity tuple, closed linker manifests/allowlists, physical arm order, strictly increasing UTC capture times, raw samples, ≥5 warmups, and ≥30 independent reps per arm.
- [x] p50/p95/p99 nearest-rank math, ≤2% bracketing drift, and ≤5% per-cell/per-workload regression are deterministic and cannot be masked by aggregate results.
- [x] Any interference flag or rejection record invalidates the whole fixture/cohort; no sample can be selectively deleted.
- [x] Behavioral fixtures cover every pass/fail/invalid boundary and repeated CLI runs produce byte-identical reports.
- [x] Non-fixture input is rejected; every report says `promotion_eligible=false`; no live capture or promotion evidence is produced.

## Verification Evidence

Final verification passed all 33/33 feasibility tests and all 57/57 validator adversarial attacks; the unchanged host aggregate was 105 passed, 0 failed, and 4 ignored. Every fixture, expected report, schema, validator, CLI, test digest, and link in the 101-input manifest matched. Final quality and security reviews both returned PASS with no findings. The validator remains synthetic-fixture-only and non-promotional, and both benchmark-contract human approval rows remain `NOT GRANTED`.

## Security Considerations

Raw artifacts and binary/environment identities are content-addressed to prevent substitution. Both variants receive identical capabilities. Rejection reasons are bounded and auditable to prevent selective deletion of adverse std samples.

## Risk Notes

The existing 1,000 in-process iterations are not automatically independent repetitions, and the current API omits p95/raw values. The new validator therefore consumes only explicit synthetic fixtures in this slice; extending it to authenticated live capture is separately blocked.

## Deviation Log

None.
