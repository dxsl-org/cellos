---
phase: 2
title: "Service-Net Maintenance-Aligned Cutover"
status: completed
priority: P1
effort: 0.5d
dependencies: [1]
tier: medium
---

# Phase 02: Service-Net Maintenance-Aligned Cutover

> **Required — deviation-log:** Record each Decision / Deviation / Surprise immediately. Choose the smallest reversible response; escalate any completion-contract or maintenance-cadence change.

## Overview

Remove service-net's one-scheduler-tick IPC polling workaround after the kernel wake contract is proven. Retain a finite NET_RX wait solely to drive the existing 100 ms smoltcp maintenance path.

## Requirements

- Replace `NET_RX_IDLE_WAIT_SCHEDULER_TICKS = 1` with the documented maintenance-aligned budget of 10 scheduler ticks (100 ms at the current 10 ms quantum).
- IPC must normally interrupt that wait earlier through raw `0`/OSTD `None`; the finite timeout is a maintenance fallback, not IPC liveness machinery.
- Preserve the loop order: pump NET_RX, perform due smoltcp/DHCP maintenance, TryRecv IPC, keep one post-reply grace yield, then wait.
- Preserve `pending_net_rx_proof` and accept a NIC proof only from a real `Some(ViCompletion)` whose source and result are `NET_RX`.
- Do not change OSTD, request/reply encoding, socket ownership, heartbeat policy, hypervisor-bridge behavior, or broker code.

## Architecture

The existing `else if let Some(completion)` shape already treats a recordless wake as “start the loop again,” so only the wait budget and its host contract change. A timeout also returns `None`, causing the same retry and allowing `iface.poll` once the 100 ms mtime deadline is due. IPC and maintenance therefore share the no-record return without adding a source bit.

## Assumptions

- **Claim:** One scheduler tick remains 10 ms, so 10 relative wait ticks match the documented 100 ms maintenance interval.
  **Confidence:** medium
  **How to verify:** Inspect the scheduler tick configuration and `docs/roadmap/open-risk-register.md` before editing; if the quantum changed, derive the smallest positive wait count that does not exceed 100 ms and update the host assertion coherently.

## Related Files

- Modify: `cells/services/net/src/service-runtime.rs` — maintenance-aligned NET_RX wait budget only.
- Modify: `cells/services/net/src/service-runtime-tests.rs` — replace the one-tick workaround assertion and retain grace/maintenance contracts.

## Implementation Steps

1. Rename the idle wait constant to communicate maintenance ownership and set it to 10 scheduler ticks; keep the call as `sys_wait_completion(NET_RX, budget)`.
2. Update the nearby comment so raw `None` means either an early queued-IPC interrupt or the maintenance timeout; neither is a completion record.
3. Leave the `Some` branch and `pending_net_rx_proof` predicate unchanged so IPC cannot masquerade as NIC activity.
4. Replace `idle_ipc_wait_is_one_scheduler_tick` with a test asserting a positive 10-tick budget aligned to the 100 ms maintenance interval. Keep the exact one-yield grace and 1,000,000 mtime-tick maintenance tests.
5. Do not add a second timer, retry loop, sleep, completion source, or service/broker protocol. Keep any added production logic within the existing file; create no new code file for this constant-only cutover.

## Success Criteria

- [x] No one-tick IPC polling constant or assertion remains.
- [x] The idle NET_RX wait remains finite at the 100 ms maintenance cadence, while Phase 01 makes queued IPC wake it earlier.
- [x] `None` immediately returns control to the loop's IPC-first retry without setting `pending_net_rx_proof`.
- [x] A genuine NET_RX completion still sets the existing proof state; IPC produces no synthetic `Some` record.
- [x] `cargo test -p service-net --target x86_64-unknown-linux-gnu` passed all 30 maintenance-budget, grace-yield, smoltcp-cadence, and oracle-boundary contracts.

## Evidence and Results

- The wait budget is 10 scheduler ticks at the existing 10 ms quantum, preserving the independent 100 ms / 1,000,000-mtime-tick maintenance cadence.
- The production one-yield post-reply grace remains exactly one; oracle-enabled and feature-off builds use the same scheduling path.
- Service-net passed 30/30 host tests. A normal feature-off RV64 kernel plus service-net build succeeded, and scans found zero occurrences of all four oracle marker classes in both artifacts.

## Security Considerations

A finite fallback prevents idle waiting from suppressing DHCP/TCP timer maintenance. Keep caller attestation, generation checks, request ownership, and message parsing untouched; this phase changes no trust boundary.

## Risk Notes

Using timeout `0` is explicitly rejected because this loop is the only 100 ms `iface.poll` driver and an indefinite wait would suppress protocol timers when no NIC/IPC event arrives. Conversely, retaining one tick would hide a broken kernel wake; the QEMU phase must prove handling before the 10-tick fallback.

## Deviation Log

- **No phase-local deviation.** The cutover changed the maintenance-owned wait budget without adding a timer, retry loop, completion source, or broker/service protocol.