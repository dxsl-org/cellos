---
phase: 1
title: "Ratify tier-aware IPC contract"
status: blocked
priority: P1
effort: "contract gate"
dependencies: []
tier: thinking
---

# Phase 01: Ratify tier-aware IPC contract

## Overview

Replace the draft's local-SAS-versus-remote simplification with a two-axis contract: *where the target is* and *which isolation/transport mechanism is authorized*. Freeze observable results, ownership and governance **before** enabling a remote route or changing public ABI. Ratification follows Spec 17's amendment process and the Law-1 two-confirmation process; writing this plan is neither confirmation.

## Requirements and proposed contract

- **Address/principal:** Keep `CellEndpoint<M>::Local(LocalEndpoint<M>) | Remote(RemoteEndpoint<M>)` explicit (`libs/ostd/src/cluster_endpoint.rs:176-181`); **proposed** local binding resolves live kernel/service-registry identity and generation, not a caller-chosen TID/path hint (current constructor only checks nonzero). Remote calls must bind authenticated `NodeId`, `(service_id, export_id)` and server/broker incarnation; current descriptor constructor does not authenticate. Add a separately typed guest endpoint only if Phase 09 admits one. Local attested `(cell_id,generation)` and remote `(peer NodeId)` have disjoint authorization namespaces. Do not trust a peer's self-reported originating Cell/tier ([Spec 20 draft §2.2](../../docs/specs/20-unified-ipc-contract.md)).
- **Route matrix:** Local Tier 1↔Tier 1 defaults to existing copied request/reply; shared buffer/ring only if both live principals and exact resource lifetime are authorized (Phase 08). Local calls involving a private-root Tier 2 use kernel validated bounded copies in either direction; DomainGrant stays disabled (`docs/specs/22-native-domain-cell-implementation-gate.md:126-159`). Remote target tier is opaque to the caller; authenticated receiving broker applies destination's *local* tier policy. Tier 3 guest is a separate bounded VM bridge, not an implicit native fastpath. Direct versus relay is mutable *session path state*, never part of a security principal or public endpoint variant.
- **Wire/version:** Reuse the existing V1 envelope (`cells/services/net-broker/src/c2c_envelope.rs:7-19,62-75`): 112-byte header, max 3,712-byte payload subject to exact transport bound, version, request ID, NodeIds, boot/server epochs, relative deadline and retry class. No implicit fragmentation or streaming. Local Spec 17 byte-0/framing and sender-mask requirements remain normative (`docs/specs/17-ipc-wire-contract.md:40-56,68-110`). Reserve/add public discriminants only through its §9 amendment; do not silently reinterpret existing frames.
- **Call semantics:** A **proposed** bounded `submit` returns an opaque operation handle or a typed definite *not submitted* failure; a completion resolves exactly one `(caller generation, request ID, destination epoch)`. Distinguish local acceptance, authority submission, peer dispatch, completion and unresolved submission. Deadline → `Timeout` only with proof no dispatch was possible; after possible submission/delivery, missing response/partition → `Indeterminate`, never `Unreachable`/`NoService` without proof. `Busy` at fixed capacity; peer restart → incarnation mismatch (current remote enum has no `Respawned` variant); no automatic retry of non-idempotent calls. Dropping a future only abandons waiting unless an acknowledged cancellation proves no execution. Existing blocking APIs retain semantics until callers migrate; no global shim to `block_on`.
- **Trust:** No remote decode/dedup/local delivery before Noise peer identity and prologue are authenticated and envelope source/destination/cluster and export policy checked. ClusterId is routing data, not credentials. Per-service authorization follows the receiving broker's live registered export and local service ACL; no remote caller inherits Tier-1 SAS trust. K1 dev/fixture and unsigned SAS posture cannot satisfy production identity admission.
- **Liveness/safety:** Local death-watch uses live owner generation; remote suspected loss/partition cannot be reported as confirmed death or authorize actuation. Remote watch ABI and cross-node physical failover remain separately gated ([Spec 20 draft §2.5](../../docs/specs/20-unified-ipc-contract.md)); neither is needed for unary RPC.

## Related files

- Modify at ratification: `docs/specs/20-unified-ipc-contract.md` (draft snapshot/outdated phase-02 status), `docs/specs/17-ipc-wire-contract.md` (§9 only if wire/syscall changes), `docs/system-architecture.md` (accurate status).
- Review: `docs/decisions/0015-dual-mode-hybrid-architecture.md`, `docs/specs/22-native-domain-cell-implementation-gate.md`, `libs/ostd/src/cluster_endpoint.rs`, `libs/types/src/c2c.rs`, `cells/services/net-broker/src/c2c_envelope.rs`, `docs/decisions/0008-protected-relay-tls-endpoint-ownership.md`, `docs/decisions/0009-correlate-relay-packet-failures.md`.

## Implementation steps

1. Inventory each public syscall, discriminant, type and import affected; read current consumers and test fixtures. Record a versioned local/remote/guest address + error + operation-state matrix, including exact response after caller/receiver restart and partial network send. Keep existing V1 byte layout unless a separately confirmed amendment proves it insufficient.
2. Amend Spec 20's aspirational text to distinguish implemented local copied IPC, ring/grant test-only/conditional paths, relay client absence and mutable direct/relay path. Remove stale references to the June implementation as active ownership; cite the new plan. Align Spec 17 only where ratified wire law actually changes; never silently replace its 4 KiB/attested-tail/sender-mask guarantees.
3. Freeze security principal and export policy by review against ADR-0008/0009 and Spec 22; define the exact pre-dedup ingress ordering and the class of proof needed for each definite error.
4. Write outcome tests for contract consumers and a two-node adversarial matrix before public ABI work. Obtain two explicit Law-1 confirmations for *each* proposed public ABI addition; pending confirmation, leave remote off and implement only code that does not require that change.

## Success criteria

- [ ] One reviewed contract table covers local Tier-1/Tier-2, remote via relay/direct, and explicit guest isolation, with no caller-selectable tier or authenticated identity.
- [ ] A state-transition matrix classifies before/after-dispatch timeout, cancellation, restart, replay, congestion and partition without automatic unsafe retry.
- [ ] Spec 20 status and Spec 17 amendments are consistent with code, ADR-0015, Spec 22 and ADR-0008/0009; every changed ABI/wire ID has separate governance evidence before coding.
- [ ] No remote route is opened and the existing local oracle still passes after documentation-only changes.

## Preparatory progress (2026-09-27; not ratification)

- Inventoried existing `CellMethod`, `LocalEndpoint`, `RemoteEndpoint`, `CellEndpoint`, `RemoteCallError`, `types::c2c` wire values, `RegisterService`/`LookupService`, Spec-17 attested `Recv` and `WaitCompletion` source bits. Only the `ostd` endpoint integration fixture currently calls the remote descriptor; `RemoteEndpoint::call` still returns `NotSupported`. No public ABI or kernel source changed.
- Updated [Spec 20 Draft v3](../../docs/specs/20-unified-ipc-contract.md) with a Tier-1/Tier-2/remote/VM-guest transport matrix, actual runtime ceiling, NodeId-vs-CellId authorization, bound 3,712-byte V1 envelope, ownership/result proof matrix and test/evidence gates. Spec 17 remains unchanged and ratified; this draft is **not** approved or implemented.
- **Ingress design proposal:** The legacy broker oracle treats its first eight bytes as an unrestricted sequence, so no byte-0 tag can coexist safely on the same receiver. Prefer mutually exclusive broker image profiles on existing `service::NET_BROKER = 8`: legacy benchmark vs strictly typed RPC, one parser per TID and no fallback. `tests/bench` oracle callers stay in the legacy regression profile until cutover; the typed two-node profile must exclude them. A simultaneous image instead needs a separate registered receiver/service ID and Law-1 review. Nothing has been packaged or enabled.
- **Replay-floor discovery:** `source_window.rs` orders a peer's source boot epoch numerically; the existing beacon derives its boot epoch from `sys_get_time_ms()`, not a protected monotonic cross-reboot source. If that value were reused for C2C, reboot could make the new node look older. Require authority-bound nonrollback C2C incarnation evidence or a separately reviewed replay-model change before Phase-05 ingress. This requirement is an external gate, not solved by the current KMS/relay scaffold.
- **Policy/deadline audit:** `RemoteExports` parses five static fields and stays `NoSecureIdentity`; it contains no peer allowlist or live target binding. Draft v3 now requires the intersection of separately provisioned peer policy, authenticated NodeId, exact typed method/retry class and live destination **before** dedup. V1's `relative_deadline` is a `u32` duration, not a cross-node absolute time: an origin admission deadline survives authority queueing, while any destination-arrival budget is separately local. After possibly submitted work, origin expiry without completion is `Indeterminate`. Phase-05/06 negatives now cover config-only export and delayed delivery. These are design findings, not passing runtime evidence.
- **Stop / blocker:** The draft now proposes ingress profiles and retains the existing remote error enum, but Phase-01 ratification still needs kernel-repair owner review of live local generation binding, a source-backed ingress/quota prototype under portfolio promotion, replay-epoch provenance with the protected authority, independent Law-1 confirmations for genuinely needed ABI changes, and contract-owner sign-off. Phase-02 Tier-2 runtime also waits for exact-profile safe-root proof and file-owner handoff. KMS/Silo relay Phase 04 remains `blocked` on its AC-001..AC-012 gates. No kernel/ABI work or remote enablement has started; the local oracle was **not** rerun for documentation-only edits.

## Assumptions

None about implemented remote operation. Open design validation: whether generic local async submission needs a new syscall or can be provided safely without it; Phase 03 must prove this before choosing ABI. Do not claim a hardware p99 from ADR targets.

## Security considerations

An enum is a dispatch description, not authority. Resolve binding/tier from kernel-owned facts and session identity; never deserialize a privileged `LocalFast` variant from a peer. The old Spec 20 remote `watch` sketch is not a permission to add a broker-wide SpawnCap.

## Risk assessment and rollback

The danger is ratifying an ABI before checking all callers or treating a future epoch as durable. Stop with the draft and rerun the Law-1 process on conflict; revert draft text if rejected. No on-wire traffic or persistent state should be produced in this phase, so rollback loses only proposal edits. If governance changes an existing public ABI, rollback cannot restore messages already sent by later phases; that requires a new reviewed migration, not a hidden compatibility shim.

## Deviation log

None.
