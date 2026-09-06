---
phase: 1
title: "Valid Measurements and Truthful Projections"
status: completed
priority: P1
effort: ""
dependencies: []
tier: thinking
---

# Phase 01: Valid Measurements and Truthful Projections

> Log every Decision / Deviation / Surprise immediately in Deviation Log. Escalate contract-breaking or irreversible changes; otherwise choose the smallest reversible change.

## Overview
Close M0: a failed or absent experiment cannot pass a performance gate, and maintained docs describe the actual trust/execution boundary. This does not manufacture a new performance PASS.

## Requirements
- Separate run validity, target verdict and historical regression verdict.
- Preserve public benchmark traits/reports, syscall ABI, hardware/QEMU separation and existing target values.
- Reuse the existing workflow/comparator/report pipeline. No generic evidence platform or source-text assertion suite.

## Architecture
`guest scenario Result -> private runner result -> complete structured run -> validity gate -> profile-compatible historical comparison -> target verdict`.
`runner.rs:19-36` drops errors; IPC scenario logs then returns Ok. Comparator `:44-77` accepts malformed/missing records; workflow `:118-149,171-180` can filter away failures and assemble invalid multi-line JSON.
The current main includes RT, SMP and stage-breakdown roles. Define required scenarios from actual invoked roles, not an invented three-metric subset.

## Assumptions
- Claim: scheduled image can execute the intended suite with its actual hart/memory configuration. Confidence: medium. Verify workflow, SMP preflight and a fresh QEMU run; do not silently omit scenarios to obtain green.
- Claim: historical records mix older schema/configuration. Confidence: medium. Inspect restored examples; legacy/unbound records are non-comparable, not current-run evidence.

## Related Files
- Modify: `scripts/compare-bench-results.sh`, `.github/workflows/perf.yml`.
- Modify: `cells/tests/bench/src/framework/runner.rs`, `cells/tests/bench/src/framework/report.rs`, `cells/tests/bench/src/main.rs`, `cells/tests/bench/src/bench-probe.rs`, affected `cells/tests/bench/src/scenarios/*.rs` consumers.
- Modify: `docs/performance-report.md`, `docs/specs/16-rustc-tcb.md`, `docs/system-architecture.md`, `docs/project-roadmap.md`, `docs/roadmap/runtime-and-platform-tracks.md`, relevant `.agents/TODO.md` projections.
- Regenerate only at settled-source integration: `docs/code-metrics.generated.md` using existing script.
- Create if no current behavior-test home exists: `scripts/test_compare_bench_results.py` (subprocess/temporary-data regression cases, not source matching).
- Read-only: `docs/app-tier-acceptance-ledger.json`, imported/hash-bound specifications, `libs/api/src/services/benchmark.rs`; do not mutate a governed source digest as a side effect of prose repair.

## Implementation Steps
1. Reuse the observed comparator reproduction; inspect current consumers and record baseline/source/profile. Keep the prior false-green evidence; do not first repair inputs to hide it.
2. Propagate setup/run/teardown errors through a private bench runner result. No public trait change. Failed sample terminates that scenario as invalid; always perform safe teardown; preserve original failure if teardown also fails.
3. Require successful send and reply from the intended peer. The generic scenario spawns `/bin/bench-probe`, whose `ipc-echo` sends one zero byte (`bench-probe.rs:44-51`), not the empty reply in the unused main dispatcher. Freeze the actual 64-byte request/one-byte-zero response; validate through existing receive metadata and payload APIs without inventing ABI fields. A changed contract gets a new profile.
4. Freeze the required scenario set and expected successful sample counts per named image profile; account for warmups separately. Reconcile scheduled SMP intent with real hart count. No silent early return for unavailable prerequisite.
5. Capture full raw QEMU output with bounded timeout and producer status. Intentional post-completion termination may be classified only after a complete valid run; timeout before completion, panic, missing metric or guest failure is invalid.
6. Assemble JSON via a real JSON serializer, not comma-less concatenation. Bind exact source, kernel/cell/image digests, toolchain/features, QEMU version/machine/harts/RAM and units. Give each capture an immutable run_id: workflow run + attempt + profile + repetition, or a collision-resistant local capture ID + profile + repetition. Store distinct captures without date-only overwrites; identical-ID conflicting content is invalid. Record content hashes for provenance without requiring identical source hashes across comparable revisions. Keep raw logs on every exit.
7. Reject empty/malformed/duplicate/missing-current scenarios, non-finite or invalid values, wrong sample counts and mismatched current identity. Memory fields use bytes, latency fields use ns; do not universally demand latency statistics from footprint-only records.
8. Compare only compatible valid runs. Freeze the documented 20-run window and >10% / three-distinct-run rule; repeated parsing of one run cannot advance streak state. Invalid runs do not advance or reset it; write state only after validation, failure-atomically. Reconstruct missing/corrupt/incompatible streak state deterministically from distinct compatible valid history; if insufficient, mark the regression row INVALID/BLOCKED, not an empty-streak PASS. A genuinely new profile with no history is BASELINE_ONLY; missing state for an existing profile is not a new profile. Run validity and target rows remain independent.
   Persist the processed run identity with profile/metric state atomically; reprocessing cannot consume a second event. Reconstruct from ordered distinct IDs when state is missing/corrupt; three captures from one revision remain distinct runs, while incompatible profiles never share a streak.
9. Keep unchanged QEMU target failures red, including footprint. Update doc claims about forbid/dependencies, mediated IPC, Tier-2 substrate vs supported product and semihosting closure. Ledger kvm-arm64 blockers stay blocked; add explanatory scope links outside governed records instead of rewriting them.
10. After source settles, project accurate counts/status through the existing roadmap owner. Do not edit historical research into fabricated new observations; mark superseded claims via living navigation.

## Success Criteria
- [x] Empty/malformed/missing/duplicate latest records fail even with valid history; regression control still fails at its unchanged threshold.
- [x] Setup/send/receive/teardown errors produce an invalid experiment, not a latency sample or target PASS.
- [x] Multi-scenario output is parseable; all required RT/SMP/normal scenarios are accounted for; missing prerequisites cannot pass.
- [x] A valid first run reports BASELINE_ONLY; identical rerun does not consume another historical streak event.
- [x] Missing/corrupt state before the third distinct bad run cannot erase sustained regression; reconstruction is deterministic or the regression row fails closed.
- [x] Fresh actual QEMU execution is bound to the exact producing source tree. Captured with exact source-patch binding the uncommitted tree (`source-patch=build/source-tree.patch` sha256 bound in `capture.source.inputs`), achieving full immutable provenance.
- [x] Raw failure logs retained; public ABI/targets/admission defaults/ledger statuses unchanged; docs agree about scope and subject.

## Security Considerations
Untrusted log/data fields must not be interpolated into executable shell. Artifact integrity metadata is not production attestation. No new cross-cell telemetry syscall or permission.

## Risk Assessment
Scoped revert restores source compatibility but cannot bless invalid historical evidence. Keep original history immutable and classify it non-comparable when necessary. No irreversible operations. CI may become honestly red on the known footprint target; do not disable that gate.

## Deviation Log
- Red-team F3 accepted: failure-atomic writes alone do not protect corrupt/missing state reads; added deterministic reconstruction and row-local fail-closed evidence.
- Red-team A1/A4 accepted: corrected the actual spawned probe contract; made capture identity/storage and idempotent state explicit.
- Implementation kept the frozen public benchmark API intact and introduced a private profile-v2 JSON/event contract with 17 required records.
- Surprise: disk-table auto-launch no longer works after block ownership moved to a userspace Driver Cell. A temporary special-init approach was rejected after launch-policy denial; the final collector exercises the normal shell path only after its exact prompt.
- Surprise: `control_loop` mixed the 10 MHz measurement timebase with 10 ms receive-timeout ticks. The constants are now distinct; the fresh run completes 200 periods.
- Surprise: multiplexed UART transport prefixes userspace lines, may append kernel logs, and may split multibyte human text. Exact raw bytes remain hash-bound; strict ASCII JSON objects are parsed independently of surrounding transport text.
- Evidence: `evidence/perf-results-reviewed/perf-local-20260905T084134Z-rv64-qemu-virt-2h-256m-v2-1.json` and its bound raw log. The collector reports `VALID`, target `FAIL`, history `BASELINE_ONLY`, but the capture is diagnostic because its dirty producing source tree is not immutably identified.
- Verification: 16 comparator contract tests pass; release RV64 bench/kernel image builds; the reviewed 2-hart/256 MiB QEMU capture completes all records. Comparator exits nonzero on the unchanged diagnostic 76.08 MiB footprint miss; IPC hardware-target status remains informational.
- Closure — fresh QEMU capture `perf-local-20260905T224048Z-rv64-qemu-virt-2h-256m-v2-1.json` collected with full immutable source-patch binding in `evidence/perf-results-final/`. Comparator validates `VALID` and honestly records `BASELINE_ONLY` and footprint target miss. Phase 01 is COMPLETED.
