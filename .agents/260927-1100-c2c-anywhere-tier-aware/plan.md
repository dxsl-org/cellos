---
title: "Cell-to-Cell Anywhere — tier-aware contract and staged delivery"
description: "Unify local Tier-1/Tier-2 IPC and authenticated remote calls without hiding isolation, partition, or relay prerequisites."
status: pending
priority: P1
effort: "milestone-gated; no calendar estimate"
branch: main
tags: [cell-to-cell-anywhere, ipc, dual-mode, relay, security]
blockedBy: []
created: 2026-09-27
---

# Cell-to-Cell Anywhere — tier-aware contract and staged delivery

## Decision and scope

This plan supersedes the unfinished execution portions of [the June foundation](../260624-cell-to-cell-anywhere/plan.md) and [the August relay-first recovery](../260819-1409-cell-to-cell-anywhere-core/plan.md). Preserve their completed tests, decisions and evidence as historical input; no remote implementation or oracle is inherited as *working*. ADR-0015's three execution tiers postdate the June design. The accepted ADR-0008/0009 trust and correlated-relay rules remain binding. Spec 17 remains ratified; Spec 20 is a draft until its amendment and Law-1 gates pass. This document is an implementation plan, **not** approval of a new syscall, remote export, or production enablement.

Native Cell-to-Cell Anywhere means local Tier-1/Tier-2 cells and authenticated peers over self-hosted relay, then direct LAN. Tier-3 is a VM guest, **not** an implicitly trusted native Cell; its separately gated bridge is Phase 09. A single typed method contract is shared, but local/remote/guest error classes and security boundaries stay explicit. A caller never chooses its own tier, fastpath, remote principal, or transport authentication. No transparent local-to-remote fallback.

## Baseline and ceilings

- `LocalEndpoint<M>` is direct copied IPC; `RemoteEndpoint<M>::call` returns `NotSupported` without broker contact (`libs/ostd/src/cluster_endpoint.rs:74-94,127-143`). Broker ingress processes only the local oracle (`cells/services/net-broker/src/local_runtime/request_dispatch.rs:9-35`); encrypted LAN beacon is not a remote call.
- Ordinary IPC is kernel-owned copied wire; SPSC ring copies bytes and its raw-address handle has no general capability/lifetime gate (`kernel/src/task/ipc_wire.rs:31-58`, `libs/api/src/services/ring_channel.rs:127-214`, `libs/ostd/src/ring_channel.rs:184-208`). Tier-2 DomainGrant is denied today (`docs/specs/22-native-domain-cell-implementation-gate.md:126-159`). `WaitCompletion` has only `NET_RX`/`TIMER` sources (`libs/api/src/abi/completion.rs:47-63`).
- A single-guest local broker QEMU oracle passes; it proves neither two-node delivery nor production. Authority-owned relay TLS client, approved protected persistence/time/binding and AC-012 evidence are external gates (`docs/decisions/0008-protected-relay-tls-endpoint-ownership.md:103-146`, `docs/project-roadmap.md:133-134`).
- Evidence ladder: contract → host → isolated two-node QEMU → named physical device → service → production. No result promotes itself to the next rung. Production admission remains separately external-gated.

## Phases

| # | Deliverable | Depends on | Entry/exit ceiling |
|---|---|---|---|
| 01 | [Ratify tier-aware contract](phase-01-contract.md) | none | contract; Law-1 approval before ABI edits |
| 02 | [Correct local Tier-1/Tier-2 routing](phase-02-local-boundary.md) | 01 | host + single-guest QEMU; copied default |
| 03 | [Reliable nonblocking call lifecycle](phase-03-async-ipc.md) | 01, 02 | host + QEMU; public ABI only after 2 confirmations |
| 04 | [Protected relay entry and handoff](phase-04-protected-relay.md) | 01, external ADR-0008 owner | contract/host while blocked; no route enable until AC-012 |
| 05 | [Authenticated broker-to-broker RPC](phase-05-remote-rpc.md) | 01, 02, 04 | host; bounded synchronous call may precede 03; remote off until 06 |
| 06 | [Isolated relay-only two-node oracle](phase-06-relay-oracle.md) | 05 | two-node QEMU/software; guarded remote opt-in only |
| 07 | [Direct LAN optimization and failover](phase-07-direct-lan.md) | 06 | LAN QEMU; relay remains correctness fallback |
| 08 | [Safe Tier-1 shared-buffer fastpath](phase-08-tier1-fastpath.md) | 02 | named local workloads; copied fallback |
| 09 | [Explicit Tier-3 guest bridge](phase-09-guest-bridge.md) | 01, 02 | guest-host QEMU; separate admission/ABI gate |
| 10 | [Performance and release promotion](phase-10-promotion.md) | 06; 03, 07–09 per claimed profile | named hardware/service; production independently blocked |

Phases 03 and 07–09 may proceed independently once their dependencies pass; Phase 03 is needed for a **nonblocking SDK claim**, not a first bounded synchronous remote call. 04/05/06 are **not** unblocked by documentation or by K1 fixtures. Promise pipelining and Internet direct/NAT hole punching are out of the baseline: consider only measured, separately reviewed follow-ons; generic async calls in 03 are not pipelining.

## Parallelism with kernel architecture repair

The [in-progress kernel repair](../260927-0739-kernel-architecture-repair/plan.md) owns its domain-root/TLB and grant-lifecycle work in `kernel/src/{task.rs,task/syscall.rs,memory/address_space.rs}` plus its scheduler/SMP and snapshot fixes while the relevant phases are open. Phase 01 of this plan may inventory, draft and review the C2C contract in parallel **without** mutating those kernel paths, approving a new public ABI, opening remote exports or declaring Spec 20 ratified. This preparatory exception does not promote the queued C2C implementation in the [portfolio](../plan-portfolio.md).

Before C2C Phase 02's real Tier-2 oracle, consume the repair's Phase-02 safe-root/admission evidence on the exact architecture/profile; copied IPC does **not** require its Phase-03 `DomainGrant` completion, and must not circumvent the grant deny gate. C2C Phase 03's syscall/completion/scheduler work and Phase 08's grant/pin/revocation-sensitive work must wait for the overlapping kernel-repair work and explicit file-owner handoff. Broker-only design may advance without overlapping edits, but Phase 04/05/06 remote implementation/enablement still obeys protected-authority AC-012 and their own dependencies. After proof and ownership separation, activate implementation in the portfolio before coding; do not merge two independent changes to the same kernel subsystem concurrently.

## Cross-plan ownership and rollback

The KMS/Silo protected-root [Phase 04](../260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md) owns ADR-0008/0009 authority-side Build, AC-001..AC-012 and the private protocol. This plan owns only broker/client integration **after** those gates; do not create a second TLS client, raw relay codec or production root. Dual-mode admission stays with [its own plan](../260906-dual-mode-kernel-evolution/plan.md) and Spec 22. On any uncertain remote promotion: close remote ingress/exports and sessions, keep local copied IPC; submitted remote side effects cannot be undone by rolling back a binary. Phase files record their narrower rollback and proof obligations.

## Exit rule

No “anywhere complete” before one native request crosses an authenticated isolated relay from node A to an exported Cell on node B and a correlated response returns; all negative identity/replay/partition tests pass. Direct LAN, authorized shared memory and VM guest bridge are separately labeled capabilities, never inferred from that result. No production claim without its external gates and physical/security evidence.
