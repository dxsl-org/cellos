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

### Exit-gate evidence review (2026-10-09) — verified, awaiting contract-owner sign-off

None of the four criteria above is ticked here: they are contract-owner acceptances, not test
results. This is the evidence each one rests on, checked against the tree at `19a10d6fa` plus the
runs named below.

| Criterion | Evidence | Not yet covered |
|---|---|---|
| **1** — one contract table with no caller-selectable tier or identity | Spec 20 §2.1 is that table (four rows: Tier-1→Tier-1; Tier-1↔Tier-2 and Tier-2↔Tier-2; other node relay/direct; Tier-3 guest), with §2.2 for the principal, ingress order and export ceiling. The no-caller-selectable clause is §2.1's *"a future live binding **must** be established against the kernel-owned service registry / caller attestation …, never by trusting a caller-supplied TID, `path_hint`, tier, remote epoch or address"*, reinforced by the Tier-1↔Tier-2 row's *"the receiver authorizes the attested caller, not a declared tier"*. Spot-checked against code: `RemoteEndpoint::new` checks nonzero metadata only (`libs/ostd/src/cluster_endpoint.rs:98-143`), `export_registry` parses at most 16 boot records with no peer-NodeId allowlist and reports remote disabled (`export_registry.rs:82-89,144-189`), and `CallerIdentity` is receiver-requested metadata written *after* the payload copy rather than a pre-delivery gate (`kernel/src/task/syscall.rs:2732-2752,3181-3196`). | Owner review of the table as contract text. |
| **2** — a state-transition matrix over timeout, cancellation, restart, replay, congestion, partition, with no automatic unsafe retry | Spec 20 §2.4 is that matrix: `NotSubmitted → Submitted → {AuthenticatedCompleted \| DefiniteFailed \| Unresolved}` with receiver `Accepted → Dispatched → Completed`, and a row each for pre-admission rejection, disabled remote, queue full, failed authentication, definite unreachable, pre-dispatch deadline, and post-dispatch / partition / lost reply / cancellation, plus incarnation mismatch. §2.3 supplies the congestion half (queue caps 16/16/32/4, dedup 16 entries with 16 replay-floor slots and 30 s retention, in-flight never evicted, duplicate in-flight `Busy`, expired `Never`/`Conditional` `Indeterminate`) and §2.2 the epoch-before-dedup ingress order. "No silent retry" is stated in §2.4 and carried by `RetryClass`. | The matrix is a contract, not behaviour: only its local restart half is witnessed today (`restart status=PASS … stale_send=INDETERMINATE`, `docs/evidence/c2c-broker-oracle-qemu-local.{txt,log}`). The deadline/cancellation/partition half belongs to Phase 03 and is unexercised. |
| **3** — Spec 20 status and Spec 17 amendments consistent with code, ADR-0015, Spec 22, ADR-0008/0009; governance before coding | Spec 20 still reads **Draft v3, not ratified**, and nothing in this pass changed that. The only ratified-surface change is `LookupServiceBound = 429`, which carries no frame, byte-0 discriminant, envelope or attested-tail change and shares the syscall allowlist bit 37 with `LookupService = 206` in the form the `ReadCap`/`WriteCap` cap syscalls already use (`libs/api/src/abi/syscall.rs:66-84`; the opcode's own note at `:595-597`); it is recorded in Spec 17 §9's amendment log as a changed syscall surface with **no** wire amendment. ADR-0015's three tiers are this table's three tiers; ADR-0023 §5 touches neither Spec 22's private-root grant denial nor ADR-0008/0009's relay ownership; and the ABI's governance (ADR-0023 plus Law-1 checkpoints 1 and 2, revision pinned by `scripts/check-lookupservicebound-law1-digests.sh`) preceded the code. | Owner review of the §9 entry's wording. |
| **4** — no remote route opened and the local oracle still passes | `RemoteEndpoint::call` still returns `RemoteCallError::NotSupported` (`libs/ostd/src/cluster_endpoint.rs:137-144`) and its integration fixture asserts that. Local oracle re-run on this tree: `test result: ok. 1 passed`, soak 10000/10000 with `indeterminate=0 stale=0 correlation=0 silent_drop=0`, bounded-queue refusal `overflow status=PASS … busy=1 queue_peak=16 … busy_frame=0x7f01`, `restart status=PASS … stale_send=INDETERMINATE`, `role_gate=PASS` (`docs/evidence/c2c-broker-oracle-qemu-local.{txt,log}`). Typed guest VFS request/reply suites green on the AArch64, RV64 and x86_64 lanes. | Nothing outstanding for the criterion itself. |

Review depth: the code points behind criteria 1 and 3 were spot-checked directly and re-derived by
two read-only Phase-02 scouts whose file:line map is in
[`phase-02-local-boundary.md`](phase-02-local-boundary.md) § *Next acceptance scenario*; the §2.3/§2.4
local-capacity citations are the spec's own and were not all re-derived in this pass.

**Still open before the phase can close:** the contract owner's sign-off on the four criteria (Spec 17
§9 entry wording included), and activation of a Phase-02 slice — the scenario that would consume this
contract is identified in `phase-02-local-boundary.md`.

## Preparatory progress (2026-09-27; not ratification)

- Inventoried existing `CellMethod`, `LocalEndpoint`, `RemoteEndpoint`, `CellEndpoint`, `RemoteCallError`, `types::c2c` wire values, `RegisterService`/`LookupService`, Spec-17 attested `Recv` and `WaitCompletion` source bits. Only the `ostd` endpoint integration fixture currently calls the remote descriptor; `RemoteEndpoint::call` still returns `NotSupported`. No public ABI or kernel source changed.
- Updated [Spec 20 Draft v3](../../docs/specs/20-unified-ipc-contract.md) with a Tier-1/Tier-2/remote/VM-guest transport matrix, actual runtime ceiling, NodeId-vs-CellId authorization, bound 3,712-byte V1 envelope, ownership/result proof matrix and test/evidence gates. Spec 17 remains unchanged and ratified; this draft is **not** approved or implemented.
- **Ingress design proposal:** The legacy broker oracle treats its first eight bytes as an unrestricted sequence, so no byte-0 tag can coexist safely on the same receiver. Prefer mutually exclusive broker image profiles on existing `service::NET_BROKER = 8`: legacy benchmark vs strictly typed RPC, one parser per TID and no fallback. `tests/bench` oracle callers stay in the legacy regression profile until cutover; the typed two-node profile must exclude them. A simultaneous image instead needs a separate registered receiver/service ID and Law-1 review. Nothing has been packaged or enabled.
- **Replay-floor discovery:** `source_window.rs` orders a peer's source boot epoch numerically; the existing beacon derives its boot epoch from `sys_get_time_ms()`, not a protected monotonic cross-reboot source. If that value were reused for C2C, reboot could make the new node look older. Require authority-bound nonrollback C2C incarnation evidence or a separately reviewed replay-model change before Phase-05 ingress. This requirement is an external gate, not solved by the current KMS/relay scaffold.
- **Policy/deadline audit:** `RemoteExports` parses five static fields and stays `NoSecureIdentity`; it contains no peer allowlist or live target binding. Draft v3 now requires the intersection of separately provisioned peer policy, authenticated NodeId, exact typed method/retry class and live destination **before** dedup. V1's `relative_deadline` is a `u32` duration, not a cross-node absolute time: an origin admission deadline survives authority queueing, while any destination-arrival budget is separately local. After possibly submitted work, origin expiry without completion is `Indeterminate`. Phase-05/06 negatives now cover config-only export and delayed delivery. These are design findings, not passing runtime evidence.
- **Stop / blocker:** The draft now proposes ingress profiles and retains the existing remote error enum, but Phase-01 ratification still needs kernel-repair owner review of live local generation binding (**design now decided in [ADR-0023](../../docs/decisions/0023-local-service-generation-binding.md); owner handoff still outstanding**), a source-backed ingress/quota prototype under portfolio promotion (**non-activating core now implemented**, see below), replay-epoch provenance with the protected authority, independent Law-1 confirmations for genuinely needed ABI changes (**opcode 429 needed and unconfirmed**), and contract-owner sign-off. Phase-02 Tier-2 runtime also waits for exact-profile safe-root proof and file-owner handoff. KMS/Silo relay Phase 04 remains `blocked` on its AC-001..AC-012 gates. No kernel/ABI work or remote enablement has started; the local oracle was **not** rerun for documentation-only edits.

## Admitted QEMU/host preparation progress (2026-10-08; non-activating)

- **Broker ingress & quota decision core:** Implemented `cells/services/net-broker/src/c2c_ingress.rs` with `IngressDecisionCore` and private-field trust types (`AuthenticatedPeer`, `ProvisionedPeerPolicy`, `LiveDestinationProof`, `ProtectedEpochProof`). Enforces reject-before-dispatch across peer identity mismatch, destination mismatch, unauthorized method, disabled/unmatched export registry, unverified epoch provenance, per-peer work quota (max 4 in-flight), per-peer byte quota (max 2 frames), and global capacity (max 16 in-flight). Cached completed replays return without consuming work quota. Verified by 123 unit tests via `cargo test --locked -p service-net-broker --lib --target x86_64-unknown-linux-gnu`.
- **Local endpoint lifecycle witness:** Implemented tests in `libs/ostd/tests/cluster-endpoint.rs` demonstrating that `LocalEndpoint` holds strictly a raw TID (`usize`) with no generation or epoch field. Confirmed that cached endpoints cannot distinguish a recycled TID or detect provider restart without an attempted send or explicit generation tokens.
- **Intel x86 QEMU runner verification:** Executed `BOOT_WINDOW=45 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso`, verifying clean boot to interactive shell on QEMU q35 with serial COM1.
- **Hardware qualification clarification:** Updated ADR-0022, HCL, and plan records so that matching configuration for the second node is preferred to minimize bring-up friction, but not mandatory; each node requires independent qualification against applicable HCL gates.

## Broker image / oracle consumer inventory (preparatory; not approval)

| Boundary | Current source-backed fact | Typed-profile review condition |
|---|---|---|
| Registration | `app-init` includes `/bin/net-broker` as `Registration::Init(service::NET_BROKER)` only with `c2c-broker` (`cells/tools/init/src/service_table.rs:128-133`). The broker does not self-register (`cells/services/net-broker/src/main.rs:18-20`). | One registered receiver and one parser per TID; keep registration explicit. |
| Existing receiver | `local_runtime::receive_once` enters the benchmark request path; `request_dispatch::process_request` parses `bench_oracle` commands (`cells/services/net-broker/src/local_runtime.rs`, `cells/services/net-broker/src/local_runtime/request_dispatch.rs:9-10`). | A typed image must not retain an oracle fallback or treat arbitrary sequence bytes as a type tag. |
| Oracle image | `scripts/run-c2c-broker-oracle-qemu.sh:86-105,110-123,140-155` builds broker/bench with `restart-oracle`, builds init with `c2c-broker`, and packages `/bin/net-broker`, `/bin/bench`, `/bin/bench-probe`. | Keep this image and its local regression separate; do not swap its broker parser in place. |
| Oracle activation | `tests/integration/tests/c2c-broker-oracle.rs:327-333` sends `bench c2c-broker-oracle` at the shell; `cells/tests/bench/src/main.rs:409-420` selects that role; `cells/tests/bench/src/scenarios/c2c_broker_oracle_orchestrator/support.rs:17-23` spawns `/bin/bench-probe` clients, which look up `NET_BROKER` (`.../c2c_broker_oracle_client/support.rs:54-58`). Packaging alone does not start that workload. | A typed image must neither start this role nor package a reachable oracle caller; retain the old runner as its own regression. |
| Shared image builders | `scripts/gen-disk-ci.sh:154-157,261-265,413-414` builds/packages both broker and bench; `scripts/build-phase04-qemu-image.sh:33-35,57-58` also packages broker. | Audit each selected init feature and shell/bench launch path when defining a typed image; presence of a broker ELF does not prove it is registered or that an oracle caller executes. |

This inventory establishes packaging and callsites, **not** mutually exclusive typed packaging or a passing negative oracle. Phase-01 checklist item for profile review remains open. No image, source, public ABI, Spec 17, or remote route changed.

## Remote ingress review handoff (source inventory; no activation)

| Required gate before dedup/dispatch | Current evidence | Unresolved review question |
|---|---|---|
| Authenticated peer and export policy | `RemoteExports::from_bytes` parses at most 16 records; each record has only service/export/version/retry/scope (`cells/services/net-broker/src/export_registry.rs:22-27,82-89,145-188`). Every parsed registry retains `NoSecureIdentity`; no peer allowlist or live destination identity is represented. | Which independently provisioned, authenticated NodeId allowlist and live service generation are intersected with the static record? A config record alone cannot grant remote access. |
| Replay provenance | V1 includes `src_boot_epoch` (`cells/services/net-broker/src/c2c_envelope.rs:62-74`); `c2c_dedup/source_window.rs:7-29` compares epochs numerically. Current **beacon** epoch comes from `sys_get_time_ms()` (`cells/services/net-broker/src/local_runtime.rs:73-86`), not a durable cross-reboot source. | Obtain protected nonrollback C2C incarnation or approve a different replay rule; do not reuse beacon uptime as proof. |
| Capacity/fairness | Local `BrokerState::handle_ingress` charges attested local caller identity at `PER_CALLER_WINDOW=4`, with request queue 16 and in-flight 32 (`cells/services/net-broker/src/local_queue/state/ops.rs:64-105`, `local_queue/state/types.rs:4-8`). | Define separate per-authenticated-NodeId byte/work quotas and reject-before-dispatch behavior; local Cell/TID quotas do not protect a remote peer budget. |

Review ordering remains Spec 20 Draft v3 §2.2; this table identifies missing inputs, not an implemented remote ingress. No quota, identity, replay, ABI or transport change is authorized here.

## Local binding handoff (design decided 2026-10-08; kernel-owner handoff pending)

`LookupService=206` returns only the current provider TID or zero (`kernel/src/task/syscall.rs:5500-5505`). The kernel registry stores `service_id → Active(tid) | Paused(tid)`, clears dead providers and replaces a service on respawn (`kernel/src/cell/service_registry.rs:1-11,25-33,43-71`); it does **not** return a provider generation. `LocalEndpoint::new(tid)` checks only nonzero and `call` sends to that stored TID (`libs/ostd/src/cluster_endpoint.rs:57-95`). Thus lookup followed by a send has no atomic live `(service_id, cell_id, generation, tid)` binding in the exported endpoint contract; lookup alone cannot prove a cached descriptor still names the same provider after a restart.

Separate **caller** identity already exists on opt-in `Recv`: kernel-written `CallerIdentity(cell_id, generation, sender_tid)` (`libs/api/src/abi/caller_identity.rs:10-32,59-72`; `kernel/src/task/syscall.rs:2652-2669`). That trailer attests the sender to a receiver; it is not a recipient-generation token for the sender. Review with the kernel-repair owner whether service resolution plus IPC admission can provide a stable recipient binding and how an outstanding reply is rejected across provider/caller generations, without changing the ratified Spec-17 frame implicitly. No new syscall, registry field or ABI shape is proposed as approved by this inventory.

### Local binding negative evidence to request at owner review

| Transition | Current observable boundary | Required proof before claiming generation-safe endpoint |
|---|---|---|
| Provider dies between lookup and send | `clear_tid` removes future lookups (`kernel/src/cell/service_registry.rs:138-152`), but `LocalEndpoint` retains its earlier TID. | A send to the stale endpoint cannot reach a different service or be reported as a successful call; a fresh lookup selects the replacement only after registration. |
| Provider is paused for hot-swap | Registry hides paused TID from new lookups and `is_paused_tid` supplies an IPC admission barrier (`kernel/src/cell/service_registry.rs:74-124`). | A previously cached endpoint cannot bypass the quiesce barrier; after commit only the replacement may receive new work. |
| Reply arrives after caller timeout or provider restart | `service_call_typed` receives masked by sender TID, not by generation/request ID (`libs/ostd/src/ipc.rs:57-76,95-110`); the bounded helper explicitly requires poisoning the service generation after a receive error (`libs/ostd/src/ipc.rs:120-126`). | Demonstrate stale replies cannot satisfy a later request, including any TID reuse; otherwise classify the outcome as unresolved rather than attributing it to the new provider. |

These are proposed behavioral witnesses, **not** tests added or results observed. The kernel owner must confirm the binding mechanism and the feasible negative oracle before Phase 01 can close.

### Decision recorded 2026-10-08: [ADR-0023](../../docs/decisions/0023-local-service-generation-binding.md)

The design review this section asked for is done. Findings that change the handoff above:

- **Premise corrected.** TIDs are **never re-issued within one boot** (`kernel/src/task/scheduler.rs:290,333`,
  `kernel/src/task/launch.rs:118,246,251`; no free list exists). A stale cached TID therefore
  fails hard through `TargetGone` (`kernel/src/task.rs:2166`), not by misdelivery. The reuse that
  is real is on **`CellId` slots**, which is why a per-Cell `cell_generation` epoch exists
  (`kernel/src/task/tcb.rs:547-556,479-493`).
- **The binding axis is `(cell_id, generation)`**, the kernel's existing epoch already consumed by
  `CallerIdentity`, `CellOwner`, dir attestation, hot swap, retirement matching and the
  exact-operation IPC path. No new token type or per-task generation is introduced.
- **The real defect is the sync reply path**, which the kernel itself documents as unfinished: a
  reply waiter that has already left `Sending` is not woken on provider death
  (`kernel/src/task/scheduler.rs:1279-1284`).
- **The kernel already ships the correct primitive**: bounded exact-operation IPC binds the peer
  as `(cell_id, cell_generation)`, and `peer_died` delivers terminal `PEER_GONE` keyed on
  `(tid, cell, generation)` (`kernel/src/task/async_ipc.rs:106-138,165-172,298-307`;
  `kernel/src/task/scheduler.rs:1324-1325`). Local service calls must move onto it.

ADR-0023 therefore decides: registry records the provider `(cell_id, generation)` (kernel-internal);
**one** append-only opcode `LookupServiceBound` (**429**, next free after `SerialConfigure = 428`)
returns the binding and leaves `LookupService = 206` untouched; **no new send opcode**; and the
TID non-reuse invariant gets two guards (fail-closed `checked_add` at the two bare `+= 1` sites,
plus a boot-time `test-hooks` no-re-issue guard).

### Implemented 2026-10-08 under the granted handoff

Law-1 checkpoint 1 covered the ABI items; the kernel-repair owner granted the **full** file-owner
handoff the same day (registry record + dispatch + the two `next_task_id` guard sites). Landed:

| Item | Location | Evidence |
|---|---|---|
| `LookupServiceBound = 429`, record type, tests | `libs/api/src/abi/{syscall.rs,service_binding.rs,syscall_tests.rs}` | `cargo test -p api` 109 passed |
| Client wrapper | `libs/ostd/src/syscall.rs` | reachable from a cell; leg 0 of the witness |
| Registry records/returns `(tid, cell_id, generation)` | `kernel/src/cell/service_registry.rs` | 8 unit tests incl. absent/paused/identity-less |
| Provider identity captured at every `register` site | `kernel/src/task/syscall.rs`, `kernel/src/cell/hotswap.rs`, `kernel/src/loader/atomic_publication_tests/baseline.rs` | witnessed: `VFS-BINDING tid=4 cell=1 gen=109` |
| Fail-closed id advance + boot no-re-issue guard | `kernel/src/task/scheduler.rs`, `kernel/src/task/task_id_selftest.rs` | `TASK-ID-REUSE: PASS` on AArch64 test-hooks |

End-to-end on Intel x86_64 QEMU (production feature set): `docs/evidence/local-service-lifecycle-x86-qemu.{txt,log}`,
driven by `tests/integration/tests/local-service-lifecycle-x86.rs` (passes; also fails against an
image without `/bin/bench`, so it is not vacuous).

**Still open before Phase 01 can close:** contract-owner sign-off. Law-1 is complete: **checkpoint 2
was recorded 2026-10-08 and the surface is FROZEN** —
[`law1-lookupservicebound.md`](law1-lookupservicebound.md) §2.2 pins the confirmed revision and
`scripts/check-lookupservicebound-law1-digests.sh` fails on drift. The kernel-repair handoff is
satisfied for this slice.

### Two pre-existing baseline blockers found (neither caused by this slice) — both resolved

1. **x86_64 `test-hooks` boot panicked in the alignment ledger check — RESOLVED 2026-10-08.**
   The case assumed the second preparation sat on the ledger's fixed point; on x86_64
   it needs a third, because the frame allocator materializes the low RAM identity
   map on demand (RAM is reached through the HHDM, and `release_frames` keeps every
   free frame identity-mapped at VA == PA), so the windows a stack touches cost a
   page table each. The case now warms to the fixed point (x86_64 only, bounded at 6
   cycles) and then requires a further cycle to restore the ledger exactly, failing if
   it never settles. `scripts/x86/qemu-domain-test.sh` now runs to its own end
   (admission, live CR3, one contained fault, frame release, teardown, shell
   recovery); AArch64 and RV64 `test-hooks` were re-run with the change. Evidence
   `docs/evidence/atomic-publication-ledger-x86-settling.{txt,log}`; pre-fix
   reproduction `docs/evidence/atomic-publication-x86-pre-existing-failure.{txt,log}`.
2. **AArch64 `test-hooks` could not build** (`-D warnings` on a dead `IDENTITY_DMA_LOGGED` static
   in `kernel/src/task/drivers/iommu.rs`). **Fixed** with the file's own idiom
   (`#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]`); no behaviour change. That fix
   is what made the guard witness above possible.

### Witness observed on Intel x86_64 QEMU (2026-10-08)

ADR-0023's defect 2 is no longer source-only. `bench local-service-lifecycle`
(`cells/tests/bench/src/scenarios/local_service_lifecycle.rs`) runs both paths against the same
provider task, which consumes one request and then exits **without replying**:

| Leg | Path | Observed |
|---|---|---|
| A | `ostd::ipc::service_call_typed` (what `LocalEndpoint::call` uses) | caller never returns; `SYNC-RESULT=RETURNED` absent; reclaimed only by `sys_force_exit` |
| B | `ostd::ipc::submit`/`wait`/`take` | `ASYNC-TERMINAL=PEER-GONE` |

Image: `scripts/build-x86_64-c2c-lifecycle-ci.sh` (production feature set, no `test-hooks`;
isolated `CARGO_TARGET_DIR`, `EMBEDDED_OVERRIDE` and ISO root). Test:
`tests/integration/tests/local-service-lifecycle-x86.rs`. Evidence:
`docs/evidence/local-service-lifecycle-x86-qemu.{txt,log}`. The test also fails against an image
without `/bin/bench`, so its assertions cannot pass vacuously.

Not covered: provider **restart** (the provider is not respawned), any remote/relay path, and the
TID non-reuse invariant. The existing RV64 broker oracle covers restart-and-re-lookup, but its
client is never a plain reply waiter blocked in a masked `Recv`.

Incidental repair admitted with this slice: `scripts/unsafe-allowlist.toml` carried a stale entry
for `cells/services/net/src/tls_handler.rs` (now `unsafe`-free) and omitted the file the `unsafe`
actually moved to, `cells/services/net/src/tls/dispatch.rs`. `cellos-sign`'s F1 check therefore
refused **every** image signing step. The entry was retargeted; no source changed.

## Assumptions

None about implemented remote operation. Open design validation: whether generic local async submission needs a new syscall or can be provided safely without it; Phase 03 must prove this before choosing ABI. Do not claim a hardware p99 from ADR targets.

## Security considerations

An enum is a dispatch description, not authority. Resolve binding/tier from kernel-owned facts and session identity; never deserialize a privileged `LocalFast` variant from a peer. The old Spec 20 remote `watch` sketch is not a permission to add a broker-wide SpawnCap.

## Risk assessment and rollback

The danger is ratifying an ABI before checking all callers or treating a future epoch as durable. Stop with the draft and rerun the Law-1 process on conflict; revert draft text if rejected. No on-wire traffic or persistent state should be produced in this phase, so rollback loses only proposal edits. If governance changes an existing public ABI, rollback cannot restore messages already sent by later phases; that requires a new reviewed migration, not a hidden compatibility shim.

## Deviation log

None.
