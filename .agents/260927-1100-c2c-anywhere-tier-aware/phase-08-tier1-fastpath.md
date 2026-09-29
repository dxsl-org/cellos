---
phase: 8
title: "Safe Tier-1 shared-buffer fastpath"
status: pending
priority: P2
effort: "security + benchmark gate"
dependencies: [2]
tier: thinking
---

# Phase 08: Safe Tier-1 shared-buffer fastpath

## Overview

Offer a measured opt-in SAS-local acceleration, without relabeling copied IPC or the existing ring as zero-copy. This phase may run independently of the relay path. It does not change Tier-2 private-root, VM guest or remote transport rules.

## Requirements and architecture

- Two **live authorized Tier-1 SAS** Cell principals may negotiate a channel with exact producer/consumer, owner, generation, buffer bounds, permissions, heartbeat/peer-death and revocation. Kernel/provider must issue an unforgeable generation-bound resource handle; a caller-supplied aligned raw address is not a capability. Current `ChannelClient::connect(handle)` checks only null/alignment before unsafe dereference (`libs/ostd/src/ring_channel.rs:184-208`), so it must not be the general service API.
- Ring SPSC gives zero-trap messages but `try_push` writes payload words into slots and `try_pop` copies them out (`libs/api/src/services/ring_channel.rs:127-214`). Describe it as zero-trap copied-message IPC. Separate true zero-copy *bulk data* when producer writes directly into an explicitly shared owned buffer and consumer reads in place. Account for cache-coherence, ownership and read/write ordering; `ReadGrant` currently copies file image into grant and is not end-to-end zero-copy (`cells/services/vfs/src/grant_read.rs:23-50`).
- Fail closed on peer exit/respawn, forged/stale handle, oversize frame, reuse and queue full. Fastpath is chosen only after authenticated endpoint binding; transport result carries the same caller attestation and ACL as copied path or declines fastpath. Never pass a shared-memory pointer to Tier 2, Tier 3 or a remote peer. If kernel handle minting/revocation requires public ABI, obtain two Law-1 confirmations before code.
- Existing copied IPC remains the correct fallback for Tier-1 calls without a negotiated channel, but not a way to bypass failed authorization for a method requiring the shared resource. No spin-until-forever in a realtime loop: finite attempts/deadline and visible pressure.

## Related files

- Modify only after governance: `libs/ostd/src/{ring_channel,ipc,cluster_endpoint}.rs`, `libs/api/src/services/ring_channel.rs`, kernel shared-buffer/grant authority and lifecycle, Tier-1 service pair integrations. Existing `cells/tests/bench/src/scenarios/ipc_fastpath.rs` is benchmark evidence only.

## Implementation steps

1. Inventory shared pointers currently created by benchmark and any service; specify exact issuer/receiver pairing, scope, revocation, bound and ownership across restart/hotswap/SMP. Prove no general raw token can be attached by an unauthorized Cell; retire it from the general path rather than keeping an alias.
2. Implement an authorized bounded channel handle and peer-death revocation, with release/acquire handoff and no ABA after generation reuse. If a kernel-visible handle or grant policy changes the public ABI, stop for Law-1 confirmations; maintain copied IPC until approved.
3. Verify control-message path versus bulk shared-buffer path separately. Require content-integrity, access denial after revoke, stalled consumer/sender deadline, split-hart concurrency and a baseline throughput/p99 comparison on one named device/build. Preserve copied path and benchmark under the same load.
4. Enable only for selected Tier-1 trusted service pairs when safety and measured benefit both pass. Document exactly which bytes are never copied and which metadata/request/reply bytes still are; expose explicit fastpath vs fallback counters.

## Success criteria

- [ ] Unauthorized/stale/misaligned/other-tier/other-node handles cannot create an endpoint or read a shared buffer; peer restart revokes old generation and memory cannot be reused while referenced.
- [ ] A real Tier-1 service pair completes an opt-in call with bounded pressure and no lost reply under two-hart concurrency; ordinary local IPC and Phase-02 Tier-2 copied oracles still pass.
- [ ] For any claimed zero-copy bulk path, producer and consumer access the *same owned payload region* without a hidden intermediate memcpy; ring is reported only as zero-trap, with named-device p99 and baseline.

## Assumptions

- **Claim:** A resource issuer can mint and revoke a trustworthy shared-buffer handle under the current memory-ownership model. **Confidence:** medium. **Verify:** kernel grant/revocation source and two-hart/exit tests; otherwise keep this phase pending and use copied IPC.

## Security considerations

SAS shared mappings amplify corruption if a raw pointer escapes. Tier-1 signing/review posture and one authorized pair are prerequisites, not a replacement for channel lifetime/revocation. DMA pins require IOMMU/device acknowledgement before frame reuse; never grant a private-root task a SAS page by accident.

## Risk assessment and rollback

A stale pointer can corrupt another Cell, so cannot be mitigated by a latency win. Disable new fastpath negotiation, drain active buffers, revoke and quarantine pinned memory before freeing, then continue copied IPC for eligible requests. Previously exposed shared bytes and in-flight writes cannot be undone. A leaked public ABI handle must be retired through governed versioning, not reused.

## Deviation log

None.
