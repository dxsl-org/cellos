---
title: "IPC-Aware Completion Wake Plan"
description: "Wake NET_RX completion waits for queued IPC without changing the completion ABI, remove service-net's one-tick polling workaround, and prove the local QEMU path."
status: completed
priority: P1
effort: 2d
branch: main
tags: [bugfix, backend, critical]
blockedBy: []
blocks: []
created: 2026-09-02
---

# IPC-Aware Completion Wake Plan

## Overview

Publish each successful IPC enqueue and its receiver wake atomically under `SCHEDULER`. A queued message interrupts only a `NET_RX` `WaitCompletion`: the syscall uses its existing raw `0`/no-record outcome, OSTD distinguishes that exact raw result for the feature-gated oracle while preserving the legacy `Option` API, and service-net polls its mailbox again. Kernel and host contracts are complete. At clean source commit `59501e2b29a7000004249977073c8069e5a67fa6`, one canonical single-guest QEMU run exited 0 with 1/1 test passing and the full gate satisfied. No remote, physical, deployed, or production qualification is claimed.

## Contract

- Preserve syscall 242, the 24-byte `ViCompletion`, source bits (`NET_RX`, `TIMER`), allowlist authority, and the legacy OSTD `sys_wait_completion` signature and behavior.
- Reject both an IPC completion source and a synthetic `NET_RX` record: IPC remains queued mailbox state, not a completion.
- Preserve NET_RX slot ownership and the exact `Owned`/`Completing`/completed cleanup machine; a concurrent NIC completion may win and return its real record while IPC remains queued.
- Preserve all Send, SendGather/post, TrySend admission, mask, blocking, backpressure, and reply behavior. Queue failure never wakes.
- Keep a finite 10-scheduler-tick NET_RX wait for the independent 100 ms smoltcp maintenance driver; IPC must end it earlier. Do not interrupt TIMER waits or expand into remote C2C behavior.
- The QEMU-only oracle may observe the review-added detailed OSTD result seam, but accepts a timing PASS only for exact raw `0` below 900,000 ticks. Late drains are INCONCLUSIVE. The benchmark launches after deterministic kernel proof without waiting for timing, but final parsing still requires at least one exact runtime PASS; INCONCLUSIVE-only output cannot pass. The kernel selftests, not service-net timing, own the causal completion-wake proof.

## Architecture

`producer copies wire record → SCHEDULER: enqueue → classify existing Recv or NET_RX WaitCompletion → Ready/push once → waiter cleanup → exact raw 0 → service-net TryRecv`

The wait side checks `pending_msgs` and publishes `TaskState::WaitCompletion` in one scheduler critical section. IPC-before-lock refuses the park; IPC-after-publication observes and wakes the parked state. The canonical QEMU gate requires exact kernel `IPC-PENDING` completion-wake and `NET-RX-RESERVATION` IPC-safe PASS markers with no corresponding FAIL, then checkpoints and launches every benchmark gate without first waiting for a timing PASS. Final whole-run parsing requires at least one exact sub-ceiling runtime PASS. INCONCLUSIVE markers are neutral, but an INCONCLUSIVE-only run fails the missing-PASS requirement. Runtime timing remains non-causal, and the production one-yield grace is unchanged.

## Phases

| Phase | Name | Effort | Depends on | Status |
|---|---|---:|---|---|
| 01 | [Kernel publication, wake, and race proof](./phase-01-kernel-ipc-wake.md) | 1d | — | completed |
| 02 | [Service-net maintenance-aligned cutover](./phase-02-service-net-cutover.md) | 0.5d | 01 | completed |
| 03 | [Local QEMU oracle and evidence sync](./phase-03-qemu-evidence-and-docs.md) | 0.5d | 02 | completed |

## Dependencies

No cross-plan dependency. Phase 01's kernel and exact boot gates passed before the Phase 02 cutover. Phase 03's corrected oracle anchors the canonical run in those deterministic kernel markers and the independent benchmark. The final clean-source run at commit `59501e2b29a7000004249977073c8069e5a67fa6` completed the local single-guest QEMU evidence. Frozen-ABI tests and the existing local C2C runner were reused rather than redesigned.

## Verification Gates

1. `cargo test -p api --target x86_64-unknown-linux-gnu` — PASS, 91 tests.
2. Fresh RV64 release kernel build and exact `ipc_pending_delivery_selftest_passes` boot gate — PASS, 1/1.
3. `cargo test -p service-net --target x86_64-unknown-linux-gnu` — PASS, 30/30.
4. Feature-off RV64 kernel plus service-net build and binary marker scan — PASS; zero oracle markers.
5. `bash scripts/run-c2c-broker-oracle-qemu.sh` — PASS at clean commit `59501e2b29a7000004249977073c8069e5a67fa6`; invoked once, exited 0, and reported 1/1 passing with both exact kernel PASS markers, no corresponding FAIL, every post-checkpoint benchmark gate, and an exact runtime timing PASS.
6. OSTD detailed decoder test — PASS. The containing package suite is **not** a pass: 23/24 because the unrelated pre-existing `clients::vfs::read_file::tests::bounds::read_uses_requested_bound_for_followup_chunks` bounds test fails.

## Completion Evidence

- Earlier diff/formatting, API 91/91, service-net 30/30, feature-off marker exclusion, focused decoder, and exact kernel boot-test evidence remains recorded in the owning phases.
- The prior cycle 36 capture (`start_ticks=144542529`, `raw_ret=0`, `elapsed_ticks=442232`) predates the final oracle/classifier source. It is supplemental historical observation, not final-source proof or a causal runtime wake claim.
- A first final-source attempt stopped on a cycle 30 late drain reported by the legacy maintenance-timeout classification and did not run the benchmark. Late raw-zero drains are now INCONCLUSIVE rather than PASS or FAIL.
- The corrected canonical runner was invoked exactly once at clean source commit `59501e2b29a7000004249977073c8069e5a67fa6`; the tree was clean at the same commit before and after, the command exited 0, and the test result was 1/1 passing. Captured output contained `[selftest] IPC-PENDING: PASS (deferred, bounded, quota-safe, completion-wake)` and `[selftest] NET-RX-RESERVATION: PASS (fills, remembers, releases, IPC-safe)` with no corresponding FAIL. Runtime cycle 36 reported `wake=recordless raw_ret=0 start_ticks=144911300 elapsed_ticks=586804 proof_ceiling_ticks=900000 budget_ticks=1000000`, satisfying the exact sub-ceiling PASS requirement; there were no INCONCLUSIVE markers. This mandatory runtime PASS remains supplemental and non-causal.
- After the fresh benchmark `START`, `[net-rx-producer] irq->completion PASS` established the real NET_RX gate, measured calibration completed 1000/1000, and the role gate passed; the 1/2/4/8/16 sweeps passed, the soak completed 10000/10000 with zero silent drops, network progress was positive, heartbeat-miss and watchdog-expired deltas were zero, and overflow and restart passed. No forbidden marker appeared: no oracle `FAIL`, `BLOCKED`, or legacy maintenance-timeout; kernel-selftest FAIL; restart role-drain or IPC-admission timeout; beacon IPC timeout/network-disabled marker; or heartbeat/watchdog task termination.
- This work is limited to local software and isolated single-guest QEMU evidence. It is not evidence for remote C2C, external systems, physical latency, deployment, or production readiness.

## Scope Boundary

No completion ABI/source/layout change and no remote relay/C2C work. The original “no `libs/ostd/**` edits” boundary was narrowed only for review-driven hardening: a detailed raw-result decoder seam and private-type test-enabling trait derives were added while the legacy API remained compatible. No physical-latency, deployed-system, or production claim is made.