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

Publish each successful IPC enqueue and its receiver wake atomically under `SCHEDULER`. A queued message interrupts only a `NET_RX` `WaitCompletion`: the syscall uses its existing raw `0`/no-record outcome, OSTD distinguishes that exact raw result for the feature-gated oracle while preserving the legacy `Option` API, and service-net polls its mailbox again. The implementation and local single-guest QEMU gates are complete; no remote, physical, deployed, or production qualification is claimed.

## Contract

- Preserve syscall 242, the 24-byte `ViCompletion`, source bits (`NET_RX`, `TIMER`), allowlist authority, and the legacy OSTD `sys_wait_completion` signature and behavior.
- Reject both an IPC completion source and a synthetic `NET_RX` record: IPC remains queued mailbox state, not a completion.
- Preserve NET_RX slot ownership and the exact `Owned`/`Completing`/completed cleanup machine; a concurrent NIC completion may win and return its real record while IPC remains queued.
- Preserve all Send, SendGather/post, TrySend admission, mask, blocking, backpressure, and reply behavior. Queue failure never wakes.
- Keep a finite 10-scheduler-tick NET_RX wait for the independent 100 ms smoltcp maintenance driver; IPC must end it earlier. Do not interrupt TIMER waits or expand into remote C2C behavior.
- The QEMU-only proof may observe the review-added detailed OSTD result seam, but accepts only exact raw `0` and must remain scheduling-neutral.

## Architecture

`producer copies wire record → SCHEDULER: enqueue → classify existing Recv or NET_RX WaitCompletion → Ready/push once → waiter cleanup → exact raw 0 → service-net TryRecv`

The wait side checks `pending_msgs` and publishes `TaskState::WaitCompletion` in one scheduler critical section. IPC-before-lock refuses the park; IPC-after-publication observes and wakes the parked state. The local QEMU oracle observes this path without reserving an armed cycle or changing the production one-yield grace.

## Phases

| Phase | Name | Effort | Depends on | Status |
|---|---|---:|---|---|
| 01 | [Kernel publication, wake, and race proof](./phase-01-kernel-ipc-wake.md) | 1d | — | completed |
| 02 | [Service-net maintenance-aligned cutover](./phase-02-service-net-cutover.md) | 0.5d | 01 | completed |
| 03 | [Local QEMU oracle and evidence sync](./phase-03-qemu-evidence-and-docs.md) | 0.5d | 02 | completed |

## Dependencies

No cross-plan dependency. Phase 01's kernel and exact boot gates passed before the Phase 02 cutover; Phase 03 then closed the local single-guest QEMU ceiling. Frozen-ABI tests and the existing local C2C runner were reused rather than redesigned.

## Verification Gates

1. `cargo test -p api --target x86_64-unknown-linux-gnu` — PASS, 91 tests.
2. Fresh RV64 release kernel build and exact `ipc_pending_delivery_selftest_passes` boot gate — PASS, 1/1.
3. `cargo test -p service-net --target x86_64-unknown-linux-gnu` — PASS, 30/30.
4. Feature-off RV64 kernel plus service-net build and binary marker scan — PASS; zero oracle markers.
5. `bash scripts/run-c2c-broker-oracle-qemu.sh` — PASS, invoked once, 1/1.
6. OSTD detailed decoder test — PASS. The containing package suite is **not** a pass: 23/24 because the unrelated pre-existing `clients::vfs::read_file::tests::bounds::read_uses_requested_bound_for_followup_chunks` bounds test fails.

## Completion Evidence

- Diff and formatting checks passed.
- API passed 91 tests; service-net passed 30 tests; a freshly rebuilt RV64 kernel passed the exact IPC pending-delivery boot gate 1/1.
- The final hardened local QEMU run observed one same-cycle startup pair: `cycle=36`, `start_ticks=144542529`, `raw_ret=0`, `elapsed_ticks=442232`, exclusive proof ceiling `900000`, maintenance budget `1000000`.
- After an independent command checkpoint and fresh `START`, the same run passed measured 1000/1000 calibration, the 1/2/4/8/16 sweeps, 10000/10000 soak, positive network progress, zero heartbeat/watchdog deltas, overflow, and restart.
- Final adversarial review reported no Critical, High, or Medium finding.
- This closes only local software and isolated single-guest QEMU evidence. It is not evidence for remote C2C, external systems, physical latency, deployment, or production readiness.

## Scope Boundary

No completion ABI/source/layout change and no remote relay/C2C work. The original “no `libs/ostd/**` edits” boundary was narrowed only for review-driven hardening: a detailed raw-result decoder seam and private-type test-enabling trait derives were added while the legacy API remained compatible. No physical-latency, deployed-system, or production claim is made.