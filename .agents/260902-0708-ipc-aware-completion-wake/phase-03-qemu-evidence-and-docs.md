---
phase: 3
title: "Local QEMU Oracle and Evidence Sync"
status: completed
priority: P1
effort: 0.5d
dependencies: [2]
tier: medium
---

# Phase 03: Local QEMU Oracle and Evidence Sync

> **Required — deviation-log:** Record each Decision / Deviation / Surprise immediately. Choose the smallest reversible response; do not convert QEMU timing into a physical-latency claim.

## Overview

Prove in the existing isolated RV64 single-guest runner that queued local broker IPC can produce an exact raw-zero, recordless service-net wake before the earliest phase-aligned maintenance deadline, then independently gate the unchanged benchmark after a fresh command `START`.

## Requirements

- Reuse the existing broker-to-net `sys_post` request path and `bench c2c-broker-oracle`; add no completion ABI, request opcode, remote relay, or cross-guest behavior. The review-added detailed OSTD result seam is the only API-level scope deviation.
- Observe a complete chronological same-cycle `ARMED`→`PASS` pair from retained startup output; do not treat an `ARMED` line as a host reservation.
- Emit `idle_ipc_wake status=PASS` only when `sys_wait_completion` returned exact raw `0`, the immediately retried TryRecv handled queued IPC, and elapsed guest mtime was strictly below both the exclusive 900,000-tick proof ceiling and the unchanged 1,000,000-tick maintenance budget.
- Reject any `FAIL`, `BLOCKED`, or maintenance-timeout oracle marker from every observed cycle, not only the selected passing cycle.
- After the startup primitive gate, checkpoint output, issue the unchanged benchmark command, and require a fresh subsequent `START`.
- Retain every existing oracle gate: real NET_RX proof, measured calibration, 1/2/4/8/16 sweeps, 10,000/10,000 soak with zero silent drops, positive network progress, zero heartbeat/watchdog deltas, overflow pass, and restart pass.
- Oracle instrumentation is feature-gated and absent from normal service-net builds; it must not change the one-yield grace, scheduling, syscall count, wait duration, or messages.

## Architecture

The `ipc-wake-oracle` service-net feature is observation-only. A bounded module records the wait-cycle number and `sys_get_time` immediately before the real completion wait. A detailed OSTD decoder separates exact raw `0`, a valid completion, and error/invalid results; only exact raw `0` can become a candidate. The next successful attested IPC drain in the same cycle emits PASS only below an exclusive 900,000-tick proof ceiling, which excludes the earliest phase-aligned 10-tick timeout.

Because autonomous broker traffic shares the same principal and an `ARMED` state is replaceable rather than reserved, the host accepts a completed startup `ARMED`→`PASS` pair as the primitive proof. It then takes a new checkpoint and independently requires the benchmark's fresh `START` and all broad result gates. Feature-on retains the production grace yield of one.

## Assumptions

- **Claim:** The isolated runner can rebuild service-net with one package feature without changing the broker/bench feature set or image layout.
  **Confidence:** high
  **How to verify:** The phase command must build the feature-enabled service-net in the existing private `CARGO_TARGET_DIR` and confirm the runner's existing artifact checks.
- **Claim:** Guest `sys_get_time` and `SMOLTCP_MAINTENANCE_TICKS` share the mtime unit used by the service loop.
  **Confidence:** high
  **How to verify:** Keep both reads inside service-net and assert the existing host maintenance test remains 1,000,000 mtime ticks.

## Related Files

- Modify: `cells/services/net/Cargo.toml` — private `ipc-wake-oracle` feature.
- Modify: `cells/services/net/src/main.rs` — feature-gated oracle module declaration.
- Create: `cells/services/net/src/idle-ipc-wake-oracle.rs` — bounded cycle/elapsed observation, under 200 lines.
- Modify: `cells/services/net/src/service-runtime.rs` — feature-gated before-wait, detailed raw-result, and IPC-drained hooks.
- Modify: `libs/ostd/src/syscall.rs` and focused tests — detailed wait-result decoder with a compatible legacy wrapper.
- Modify: `libs/ostd/src/clients/vfs/read_file/path.rs` — private-type derives needed for the pre-existing package tests to compile; no layout or visibility change.
- Modify: `scripts/run-c2c-broker-oracle-qemu.sh` — build service-net with the oracle feature in the isolated image.
- Modify: `tests/integration/tests/c2c-broker-oracle.rs` — startup-pair ordering, exact PASS fields, whole-run failure rejection, independent command `START`, and retained baseline assertions.
- Modify after PASS: `docs/roadmap/open-risk-register.md`, `docs/roadmap/current-focus.md`, `docs/project-roadmap.md`, `docs/project-changelog.md` — synchronize evidence while preserving local-QEMU and non-production ceilings.

## Implementation Steps

1. Add the feature and bounded focused module. Track only cycle, wait-start tick, raw result classification, and the current candidate; reset or clear it on every disqualifying outcome.
2. Emit `ARMED` immediately before the real wait with coherent cycle/start/budget/ceiling fields. Emit PASS only after exact raw `0` and the immediate successful attested IPC drain in that same cycle.
3. Add a detailed OSTD wait-result decoder that distinguishes `NoRecord`, valid `Completion`, and `ErrorOrInvalid`, while preserving the legacy `Option` wrapper.
4. Keep the real wait at 10 scheduler ticks, the post-reply grace at one, and the feature off by default. Confirm normal RV64 artifacts contain no oracle markers.
5. In the host test, reject `FAIL`, `BLOCKED`, and maintenance-timeout markers from every retained oracle line, then require one chronological same-cycle startup pair with exact raw zero and elapsed below the exclusive ceiling.
6. After that primitive gate, checkpoint output, launch the unchanged benchmark once, require a later `START`, and evaluate the retained NET_RX, calibration, sweep, soak, network-progress, liveness, overflow, and restart gates.
7. Keep evidence wording local and software/QEMU-only. Do not infer that the benchmark caused the accepted startup pair.
8. Synchronize the four living evidence documents with the atomic queued-IPC/raw-zero contract, 10-tick maintenance fallback, exact local QEMU evidence, and explicit remote/physical/production caveats.

## Success Criteria

- [x] The canonical runner, invoked once, observed a chronological same-cycle startup `ARMED`→`PASS` pair with exact raw `0`: `cycle=36`, `start_ticks=144542529`, `elapsed_ticks=442232`, proof ceiling `900000`, budget `1000000`.
- [x] The exclusive ceiling proves the accepted pair preceded the earliest phase-aligned 10-tick maintenance deadline; every observed `FAIL`, `BLOCKED`, and maintenance-timeout marker is rejected.
- [x] The independent post-checkpoint benchmark emitted a fresh `START`; real NET_RX proof, measured 1000/1000 calibration, 1/2/4/8/16 sweeps, 10000/10000 soak, positive network progress, zero heartbeat/watchdog deltas, overflow, and restart all passed.
- [x] A normal feature-off RV64 kernel plus service-net build succeeded and contained zero oracle markers.
- [x] The detailed OSTD decoder test passed.
- [ ] The containing OSTD package suite is **not** a PASS: 23/24 because the unrelated pre-existing `clients::vfs::read_file::tests::bounds::read_uses_requested_bound_for_followup_chunks` bounds test still fails after private derives allowed the suite to compile.
- [x] Risk, current-focus, project-roadmap, and changelog projections were synchronized with the captured local QEMU result and explicitly preserve ABI, observation-only, local-only, remote, physical, and production caveats.

## Evidence and Results

- Diff and formatting checks passed; API passed 91 tests; service-net passed 30 tests; the fresh RV64 release kernel and exact IPC boot test passed 1/1.
- The final hardened canonical QEMU run passed 1/1. Its accepted primitive pair was `cycle=36 start_ticks=144542529 raw_ret=0 elapsed_ticks=442232 proof_ceiling_ticks=900000 budget_ticks=1000000`, satisfying `442232 < 900000 < 1000000`.
- After the independent command checkpoint and fresh `START`, measured calibration completed 1000/1000; all 1/2/4/8/16 sweeps completed; soak completed 10000/10000 with positive network progress and zero heartbeat/watchdog deltas; overflow and restart passed.
- The detailed OSTD decoder behavior passed its focused test. The package result remains 23 passed, 1 failed and is recorded as a known unrelated baseline, never as a pass.
- Final review found no Critical, High, or Medium issue after the raw-result seam, exclusive ceiling, and whole-run failure scan were added.
- Evidence ceiling: local software plus isolated single-guest QEMU only. No remote, external-system, physical-latency, deployment, or production claim.
- The four living evidence documents were synchronized after the passing local QEMU gate; they do not broaden the evidence ceiling.

## Security Considerations

The feature must be off by default, expose no syscall/request surface, allocate no unbounded state, and log no payload or caller identity. The runner remains isolated and keeps its existing private key/image handling. Documentation must not overstate QEMU evidence as remote or hardware qualification.

## Risk Notes

An `ARMED` line is an observation, not a reservation: autonomous broker traffic shares the principal and can replace or consume a cycle before a host command reaches the benchmark. The accepted startup pair therefore proves only the queued-IPC/raw-zero primitive; the fresh post-checkpoint `START` and broad benchmark gates prove the independent benchmark run. Exact raw-result decoding, the 900,000-tick exclusive ceiling, and whole-run rejection of failure markers close the false-positive paths identified by review.

## Deviation Log

- **Observation-only oracle deviation:** Rejected the planned host-selected fresh post-shell cycle because `ARMED` provides no lease or acknowledgement and autonomous broker traffic uses the same service principal. Restored the production post-reply grace of one for feature-on builds. The final gate accepts a retained, completed startup same-cycle raw-zero pair, then independently checkpoints and requires a fresh benchmark `START` plus every broad result gate. This deliberately does not claim benchmark provenance for the startup wake pair.
- **Review-driven hardening:** Added a detailed raw-result seam so only exact raw `0` can arm proof, introduced the exclusive 900,000-tick ceiling below the unchanged 1,000,000-tick maintenance budget, and reject `FAIL`, `BLOCKED`, or maintenance-timeout evidence from every cycle.
- **OSTD baseline caveat:** Private trait derives allowed the detailed decoder tests to compile. The focused decoder test passed, but the package remains 23/24 due the unrelated pre-existing `read_file` bounds failure; it is not reported as a pass.