---
phase: 2
title: "Correct local Tier-1/Tier-2 routing"
status: pending
priority: P1
effort: "implementation gate"
dependencies: [1]
tier: thinking
---

# Phase 02: Correct local Tier-1/Tier-2 routing

## Overview

Make local requests obey the tier-aware contract without inserting `net-broker` into the hot path. Deliver an actual named service exchange between a trusted Tier-1 Cell and an admitted private-root Tier-2 Cell, with copied IPC and correct caller identity. Preserve existing Tier-1 behavior; do not advertise ring/grant as the default.

## Requirements and architecture

- Resolve service ID to a *live registered binding* (Cell ID, generation, current TID, isolation domain) through the existing service registry. Bind calls and replies to the same incarnation; a stale late reply must not be accepted after respawn. SDK address construction must not let caller-declared tier grant fastpath privileges.
- Local Tier-1↔Tier-1 and any Tier-2 boundary use sender-masked typed IPC (Spec 17); domain-aware `copy_from_user`/`copy_to_user` remains the only Tier-2 data path. `DomainGrant` to a private root is forbidden until Spec 22's owner/revoke/TLB/DMA state machine lands (`docs/specs/22-native-domain-cell-implementation-gate.md:126-159`). Ring raw address token (`libs/ostd/src/ring_channel.rs:184-208`) remains limited to explicitly trusted fixture/benchmark paths.
- Preserve kernel-attested sender TID/Cell ID/generation and service-side authorization, independent of copied versus later shared transport (`kernel/src/task/ipc_wire.rs:19-36`; `cells/services/vfs/src/main.rs:126-150`). No guest or remote target can be silently resolved as a local Tier-1 service.
- Maintain bounded 4096-byte messages, explicit admission failure and correct sender-mask; never turn a blocked sender into a drop-on-not-ready `sys_try_send` call.

## Related files

- Modify if needed: `libs/ostd/src/cluster_endpoint.rs`, `libs/ostd/src/ipc.rs`, `kernel/src/cell/service_registry.rs`, `kernel/src/task/{ipc_wire,syscall}.rs`, `kernel/src/loader/domain_admission.rs`.
- Integration fixtures: existing native-domain QEMU tests and `scripts/run-c2c-broker-oracle-qemu.sh`; add one real cross-tier named-service exchange in the existing integration suite, not a source-text/wiring assertion.

## Implementation steps

1. Record existing local service lookup/callers and receiver allowlists; create a precise binding lookup with generation validation only where needed. Keep direct local `LocalEndpoint` use, preserve `recv(service_tid)` and attested receiver context.
2. On RV64 with qualified private roots, run Tier-1→Tier-2 and Tier-2→Tier-1 typed requests carrying nontrivial payload and reply. Deny wrong-user-buffer mapping, stale generation, unauthorized service method and oversize frames *before* delivery; show Tier-1↔Tier-1 existing service calls remain unchanged.
3. Test restart during queued/in-flight local work: old reply cannot satisfy new binding; record deterministic `Busy`/target-gone/indeterminate semantics consistent with the Phase-01 matrix, without silently replaying side effects.
4. Keep tier/guest disambiguation local to resolver/kernel binding; avoid adding one `match tier` into every application's hot path. Benchmark existing IPC and the new copied cross-tier path separately with source/build provenance, concurrency and p99 tails.

## Success criteria

- [ ] QEMU RV64 real Tier-1↔Tier-2 service request and reply pass with authenticated owner, invalid-buffer rejection and stale-reply exclusion; an ineligible architecture/profile refuses Tier-2 admission rather than silently launching in SAS.
- [ ] The existing local C2C broker oracle and normal typed VFS request/reply still pass, with measured source-bound before/after p99 and no watchdog misses; no unmeasured improvement claim.
- [ ] No private-root grant, raw shared pointer or remote broker fallback is used by cross-tier IPC.

## Assumptions

- **Claim:** Existing service registry yields enough generation/domain metadata for unambiguous new binding. **Confidence:** medium. **Verify:** inspect registry owner/generation and restart tests before adding an ABI; keep a local trusted lookup if possible.

## Security considerations

Caller cannot submit tier or `TrustedShared` directly. Copy validation spans entire private mapping, not only the first byte; do not accept domain pointers into SAS/kernel/peer mappings. Preserve service ACL independent of sender transport.

## Risk assessment and rollback

Cost: a tier dispatch or binding lookup could regress Tier-1 hot calls. Preserve old direct local copied path and disable only the new tier-aware resolver on regression; no on-disk state changes. A delivered request's side effects cannot be rolled back by changing the resolver—use request IDs and explicit unknown-outcome reporting. Do not relax Tier-2 fail-closed gate to force a green oracle.

## Deviation log

None.
