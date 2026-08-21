# Fixture-Only Benchmark Validator Contract

Contract ID: `CELLOS-RUST-STD-BENCHMARK-VALIDATOR-v1`
Schema: `rust-std-benchmark-run/v1`
Implementation: `scripts/rust_std_promotion/`, `scripts/validate-rust-std-promotion.py`

## Closed Boundary

Input must say `source_kind="synthetic_fixture"` and `requested_designation="fixture_validation_only"`. The bundled stdlib-only schema validator enforces exact JSON types, constants, enums, digests, RFC 3339 date-time values, bounds, non-empty required arrays/strings, and closed root/nested objects before statistics. Live capture, promotion/ledger designations, missing/extra cells, substituted provenance, non-finite numbers, and unknown fields are `INVALID`. There is no capture, authentication, ledger, promotion, PAL, or runtime API.

Every report says `fixture_only=true` and `promotion_eligible=false`. Exit status is `0` for `VALID_PASS`, `1` for a structurally valid regression failure, and `2` for `INVALID`.

## Gates

1. Exactly three physically contiguous arms per declared cell/workload, in source order: `no_std_pre/1`, `std/2`, `no_std_post/3`. The validator never sorts arms or cells to recover malformed input. Each triple has UTC `Z` `captured_at` values that strictly increase.
2. The complete tuple in [`workload-parity-spec.md`](workload-parity-spec.md) plus reset/interference policy matches. Common linker inputs are an exact closed ordered manifest and canonical derived digest. Runtime linker inputs exactly equal the pinned `no_std` or `std` fixture allowlist and canonical derived digest; only runtime kind, binary digest, and the permitted runtime manifest differ.
3. Additions, omissions, reordering, role/digest swaps, arbitrary digest exemptions, and mlibc/POSIX/libc/host/instrumentation identities are `INVALID`.
4. Workload/version/payload/trace is one frozen workload.
5. Each arm has at least five warmups, one operation per repetition, and 30 independent repetitions. Every `run_id`, `rep_id`, and `fresh_instance_id` is unique across the complete document, including separate cells.
6. Every repetition retains positive integer raw latency, a true Boolean monotonic assertion, one non-empty threshold profile matching all arms in its cell, and complete false interference metadata. Counts, summary, and raw digest are recomputed over every repetition.
7. Any true interference flag or any rejection record invalidates the complete document/cohort before statistics. No repetition is selectively deleted.
8. Missing/noisy/invalid cells invalidate the document. Results never pool across cells/workloads.

## Exact Integer Math

Nearest rank uses sorted values and rank `(q*n + 99) // 100` for `q=50,95,99`, then zero-based index `rank-1`. There is no floating point, interpolation, trimming, or winsorizing.

Drift passes exactly when `abs(post_p99-pre_p99)*100 <= pre_p99*2`. Baseline p99 is nearest-rank p99 over concatenated pre/post raw samples. Regression passes exactly when `(std_p99-baseline_p99)*100 <= baseline_p99*5`. Equality at 2% and 5% passes. Drift above 2% is `INVALID`; regression above 5% is `VALID_FAIL` only after every structural gate passes.

## Determinism and Tests

Canonical JSON sorts keys, uses fixed separators/ASCII/integer values, and ends with one newline. The CLI has one fixture argument, writes the report only to stdout, and exposes no arbitrary output path. Cells and arms preserve the validated physical source order; reasons sort lexically. Reports omit wall-clock generation time, host paths, and generated IDs; they retain raw arrays, zero rejection counts, summaries, baseline p99, input digest, schema digest, and validator version.

`tests/rust-std-promotion/test_validator.py` covers pinned reports, statistical behavior, physical arm order, equal/reversed/non-UTC timestamps, complete sys-module scope, whole-document interference invalidation, and closed linker allowlists including additions/omissions/role/digest swaps and forbidden mlibc/POSIX/instrumentation identities. `test_validator_rejections.py` covers the adversarial schema/type/constant/date-time/digest/string matrix, empty documents/arrays, global identity collisions, non-finite JSON constants, threshold profiles, approval-input-manifest binding, and removed CLI output option. Eight named synthetic fixtures and two expected reports are executable contract examples, never measurement or promotion evidence.

Approval state is recorded only in [`../approvals/benchmark-contract.md`](../approvals/benchmark-contract.md).
