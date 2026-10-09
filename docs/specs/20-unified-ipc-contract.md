# Spec 20 — Tier-aware Cell-to-Cell IPC Contract (DRAFT v3)

> **Status:** Draft v3, 2026-09-27 — contract proposal and code inventory only; **not ratified**. [Spec 17](17-ipc-wire-contract.md) remains the normative local wire/attestation contract. ADR-0015, ADR-0008 and ADR-0009 remain accepted decisions; this draft cannot amend them.
> No public syscall, byte-0 discriminant, enum variant, broker watch authority or remote export is approved by this text. Each actual Law-1/ABI change requires its own two explicit confirmations and a Spec-17 §9 amendment where relevant.
> Execution owner: [tier-aware C2C plan](../../.agents/260927-1100-c2c-anywhere-tier-aware/plan.md). Its Phase 01 contract drafting may overlap kernel repair, but kernel and shared ABI work may not. [June](../../.agents/260624-cell-to-cell-anywhere/plan.md) and [August](../../.agents/260819-1409-cell-to-cell-anywhere-core/plan.md) plans are historical, not implementation authority.
> v2's `CellAddr`/raw `path_hint`, UDP-sized universal payload, remote watch and one-error-for-all-ranges sketches are **not** approved interfaces. The concrete existing endpoint and transport types below are the starting point; proposed behavior is labelled.

## 1. Context and implementation ceiling

One typed `CellMethod` may describe an operation on a same-node Cell or an authenticated remote node; *locality* and *isolation tier* are independent axes. A Tier-3 VM guest is not a native Cell. The same request/response schema does not make local and remote identities, errors, latency or physical-safety decisions interchangeable. This is a contract, not a kernel bus or a promise of transparent RPC.

| Component | Observed implementation (2026-09-27) | Proposed next gate |
|---|---|---|
| Local typed IPC | `LocalEndpoint::call` invokes `ipc::service_call_typed` on a nonzero TID; sender-masked request/reply, copied 4 KiB Spec-17 wire (`libs/ostd/src/cluster_endpoint.rs:51-95`). `LocalEndpoint::new` checks nonzero only, not a live generation. | Phase 02 binds service lookup to live kernel/registry identity and recipient generation; retain direct local path. |
| Tier-2 native | RV64 private-root copied IPC is available; [Spec 22](22-native-domain-cell-implementation-gate.md) denies public grant entry points naming a private root until kernel repair proves revoke/map/TLB ownership. AArch64/x86 Tier-2 admission is gated on safe root switching. | Phase 02 proves a real cross-tier request/reply on an admitted profile; no private-root `DomainGrant`. |
| Broker / remote | `net-broker` boots with K1, an opaque KMS identity if available, authenticated beacons and bounded local oracle roles. `local_runtime/request_dispatch.rs` serves Echo/Snapshot/Hold; other work returns `NotSupported`. `RemoteEndpoint::call` always returns `NotSupported` without broker contact; `RemoteEndpoint::new` checks nonzero metadata, **not** authentication (`libs/ostd/src/cluster_endpoint.rs:98-143`). | Protected relay entry and two-node call are Phases 04–06, blocked by independent protected authority/AC-012 evidence. |
| Remote foundation | V1 112-byte envelope, 3,712-byte max payload, bounded receive/dedup and boot-local server epoch have focused host tests; no authenticated broker-to-broker runtime or two-node oracle. Relay server codec evidence is server-only. | No remote/export enablement from compiled modules or a single-guest benchmark. |
| Fastpath / guest | Tier-1 SPSC ring copies message bytes (zero-trap, not general zero-copy); raw-address handle is not a general capability. Tier-3 guest has no C2C guest bridge. | Separate, opt-in Phases 08 and 09 with ownership/VM-boundary proof. |

Evidence ceiling: existing single-guest broker QEMU oracle proves local roles only. Development K1/`DEV_REFERENCE` cannot satisfy production identity; remote/private/public, physical and production claims remain distinct.

## 2. Proposed contract (not an approved public ABI)

### 2.1 Address and tier are separate decisions

`CellMethod` already supplies typed request/response, `SERVICE_ID`, `EXPORT_ID` and `RetryClass`. `CellEndpoint<M>` is explicitly `Local(LocalEndpoint<M>) | Remote(RemoteEndpoint<M>)`; callers must branch on locality. The current local descriptor contains a TID and the current remote descriptor contains `CellNetId`, `ClusterId` and boot-local `ServerEpoch`. Neither constructor authenticates its arguments. A future live binding **must** be established against the kernel-owned service registry / caller attestation or an authenticated peer session, never by trusting a caller-supplied TID, `path_hint`, tier, remote epoch or address. Retain these existing names pending an approved interface change; the v2 `CellAddr` sketch is retired.

| Target | Proposed transport and receiver authorization | Excluded implicit fallback |
|---|---|---|
| Local Tier 1 → Tier 1 | Direct copied request/reply by default; kernel-attested `(cell_id, generation, sender_tid)` and live service binding. Negotiated ring/shared buffer only after Phase-08 pair/grant/revocation proof. | No raw pointer promoted into a capability; no remote broker on local failure. |
| Local Tier 1 ↔ Tier 2 or Tier 2 ↔ Tier 2 | Kernel-validated bounded copied IPC, including `copy_from_user`/`copy_to_user` for private roots; only an admitted architecture/profile. The receiver authorizes the attested caller, not a declared tier. | No SAS identity mapping, `DomainGrant` or silent admission to Tier 1. |
| Other node, relay or direct | Same typed service/export on an **authenticated peer NodeId**; route selection is mutable session state. Receiving broker admits the peer NodeId and current export, then applies its destination's local tier policy. Protected mTLS carries opaque end-to-end Noise records for relay; direct must authenticate that same peer. | No relay certificate → local Cell authority, unencrypted TCP, public export by default or automatic local resolution. |
| Tier-3 VM guest | Separate opt-in bounded guest→host copied bridge, with guest instance identity, validated descriptors and host export policy (Phase 09); not a native `CellEndpoint::Local`. | No host CellId, SAS mapping, DomainGrant or guest-origin remote authority by implication. |

Binding lifetime: local recipient lookup returns current provider TID only (`LookupService = 206`); **`LookupServiceBound = 429` now returns the provider binding too** — designed and frozen in [ADR-0023](../decisions/0023-local-service-generation-binding.md) (Law-1 checkpoint 2, 2026-10-08) with the binding axis being the existing per-Cell `(cell_id, generation)`. Remote `ServerEpoch` is boot-local to one broker (`server_epoch.rs:10-49`), **not durable**; session/broker generation must invalidate a descriptor learned before restart. A known stale request is rejected before dedup/delivery. No old reply may satisfy a new caller/recipient generation. A local `(cell_id, generation)` is **not** a remote replay epoch.

### 2.2 Principal, ingress and export

Local request authorization uses the kernel-written Spec-17 §11 `CallerIdentity(cell_id, generation, sender_tid)` attestation, not `CellId(sender_tid)` or `path_hint`. Its absence denies authorization. `RecvTimeout`/`TryRecv` do not attest. Across nodes, Noise authenticates the **node** (static key/ordered prologue); a peer-supplied Cell name, tier or local TID is advisory only, never fed into the local Cell ACL. ClusterId and a beacon/machine ID are routing hints, not credentials; beacon identity must be bound to configured NodeId. Public KMS opcodes 9–14 remain frozen.

**Proposed receive order:** typed protected authority event / authenticated direct session → verify peer NodeId and Noise identity/prologue → decrypt and validate exact V1 envelope version, source NodeId, destination NodeId, cluster and lengths → check live export `(service_id, export_id, version, scope, retry class)` and peer allowlist → check current destination server/broker epoch and deadline → bounded ingress quota and per-source replay state → dedup → kernel-attested local service dispatch. Code today has only *local-only* `ReceiveGate` (epoch-before-dedup); an export registry can parse config but reports remote disabled. Neither component authenticates a peer when called alone. `Public` scope is separately governed, not implied by a config record or secure-looking NodeId.

**Current export-policy ceiling:** `export_registry.rs:82-89,144-189` parses at most 16 boot-provisioned records containing only `(service_id, export_id, version, retry_class, scope)`. It exposes no peer-NodeId allowlist or live destination-generation binding, and every parsed registry remains `NoSecureIdentity`/remote-disabled. A `scope=remote` line therefore authorizes **nothing** by itself. Proposed private admission must intersect an authenticated peer with an independently provisioned allowlist, the matching typed method/retry class, and a live registered destination; absence of any one factor denies dispatch. Do not turn `scope=public` on as a substitute for missing peer policy.

Relay TLS endpoint and outer framing belong solely to the protected authority ([ADR-0008](../decisions/0008-protected-relay-tls-endpoint-ownership.md)); `service-net` is a fixed-target byte carrier. [ADR-0009](../decisions/0009-correlate-relay-packet-failures.md) retires `0x08`; authority alone produces correlated `0x0d` and parses `0x0a` errors. `{session_generation, correlation}` is a bounded **transport-local** key, never the Noise-inside application request ID or an identity token. Success of TLS write/relay drain is not proof that a service executed.

### 2.3 Frames, bounds and capacity

Spec 17's 4,096-byte copied IPC, masked reply receive, byte-0 registry and 32-byte attested tail remain intact. The broker-local benchmark oracle's 10-byte request starts with an **unrestricted 8-byte `client_sequence`**, not a protocol tag; its `0x7f` reply tag is not a global remote syscall or the relay's `FT_ERROR`. `receive_once()` feeds every attested sender into that parser (`local_runtime.rs:160-175`). Choosing a new byte-0 tag while that parser is live on the same receiver would collide.

**Proposed clean cutover:** retain the existing `service::NET_BROKER = 8` and one receive owner. A dedicated, isolated *development image profile* may select either the legacy local-oracle parser **or** a versioned, strictly bounded typed-RPC parser, never both on that TID; no runtime auto-detection or decode fallback. The default image keeps legacy/local-only behavior while Phase 05 tests the remote parser disabled outside the development profile. The Phase-06 two-node oracle must use the typed profile and prove that no legacy oracle caller is packaged there; legacy single-node regression runs in its own profile. Promotion migrates/removes benchmark-only consumers and the legacy parser in one governed cutover, with a replacement behavioral oracle. If both protocols must coexist in a single image, stop for a separately registered receiver TID/service-ID and Law-1 review rather than guessing a discriminator. No profile, service-ID or byte-0 ABI is approved or implemented by this draft.

Remote V1 envelope is already canonical: 112-byte header + up to **3,712 payload bytes** (`MAX_C2C_PAYLOAD = min(local attested ingress cap, NET_TCP_INLINE_DATA_MAX - 16 Noise tag - 112 header)`); max plaintext frame 3,824 bytes. It carries source/destination NodeIds, source boot epoch, destination server epoch, cluster, service/export IDs, nonzero request ID, relative deadline, retry class and payload. No universal ~480-byte UDP cap, implicit fragmentation, stream or UDP RPC path: currently scoped relay-TCP/direct-TCP transports must enforce the same bounded record limit. A valid local 4 KiB request need not fit remotely; reject it before submission, never truncate.

The existing response cache has 16 entries, 16 per-source replay-floor slots and 30-second completed-entry retention (`c2c_dedup/types.rs`). In-flight entries are never evicted. Duplicate in-flight requests report `Busy`; completed duplicates replay only while retained. An expired `Never`/`Conditional` request is `Indeterminate`, not automatically rerun; an explicitly idempotent method may be redispatched only under the validated replay-floor rule. Full cache/session pool rejects admission (`Busy`). `source_window.rs` orders each peer's `src_boot_epoch` numerically and rejects older epochs; `BrokerNetworkState` currently seeds its **beacon** `boot_epoch` from `sys_get_time_ms()` (`local_runtime.rs:65-86`), which is not an authenticated monotonic cross-reboot C2C epoch. Do not reuse that uptime value for a remote replay floor. Phase 04/05 must prove a non-rollback source epoch bound to protected peer identity and session generation, or propose and separately ratify a changed replay model; remote ingress stays off until then. These volatile bounds are **not** exactly-once across partition/restart/retention.

**Relative deadline ownership:** V1 carries only a `u32` *duration* (`c2c_envelope.rs:108-112`), not a sender timestamp or a globally comparable absolute expiry. `RequestDeadline::from_relative(now_ms, ...)` (`c2c_deadline.rs:27-55`) is valid in the clock domain where it is constructed. At origin, establish the caller's local deadline on admission and send no more than its remaining budget; relay acceptance cannot reset that local deadline. At the destination, a received duration may bound **new local work** from its own arrival, but cannot prove the origin has not already expired during transit. After possibly submitted transport work, origin expiry without an authenticated completion remains `Indeterminate`, even if the destination's independent budget has time left. A stronger cross-node expiry guarantee would require a separately reviewed time/protocol proof, not comparing local monotonic clocks or treating each hop's fresh duration as the caller's original deadline.

### 2.4 Submission and outcome matrix

The proposed operation state is `NotSubmitted → Submitted → {AuthenticatedCompleted | DefiniteFailed | Unresolved}`; receiver work may also be `Accepted → Dispatched → Completed`. These are **different boundaries**: `Submitted` begins when the protected authority accepts a typed outbound send, or acceptance becomes uncertain. A local queue slot being accepted does not prove remote submission; an outbound accepted request does not prove peer dispatch. An authority *explicit rejection with ownership returned unchanged* leaves `NotSubmitted` (ADR-0009 §10). Request identity binds the origin caller generation, nonzero request ID, peer NodeId, broker session and target server epoch; transport correlation remains separate. No silent retry, especially for non-idempotent effects.

| Observed evidence at resolution | Proposed caller-visible result | Proof obligation |
|---|---|---|
| Local request rejected before admission (oversize, full bounded queue, missing live binding) | Existing local typed error / `Busy`; no remote submission | Ownership stays local; no target dispatch. Current `LocalEndpoint::call` still maps send/receive/decode errors to `ViError::IO` and does **not** implement these proposed finer local outcomes. |
| Remote path disabled or missing protected admission | Existing `RemoteCallError::NotSupported` | No broker contact or export attempt; current behavior. |
| Queue/session full before authority accepts | `Busy` | Admission fails with owned request unchanged; no in-flight eviction. |
| Authentication / export check fails before dispatch | `AuthFailed` / `NoService` only where the receiver can authenticate and prove rejection | Do not claim service absence from a bare timeout or unauthenticated response. |
| Protected authority explicitly rejects unchanged request; relay proves destination absent before any destination write (`0x0a/0x01`) | `Unreachable` (definite) | Match live session generation and active correlation; no possible target delivery. |
| Relative deadline expires with **proof no dispatch was possible** | `Timeout` | A timer alone is insufficient once submission/destination write is uncertain. |
| Remote dispatch may have occurred; relay `0x0a/0x04`, disconnect, lost reply, cancellation without acknowledged non-execution, or expired post-dispatch deadline | `Indeterminate` | Never infer non-execution from a partition; reconcile by request ID/application policy, not blind replay. |
| Authenticated response matches live caller/request/peer/server epoch | Typed method response | Exact source/epoch/correlation and local return owner agree. |
| Target incarnation differs (boot-local epoch mismatch, peer or caller restart) | Reject stale delivery; use existing `Indeterminate` if an old submitted request may have executed | First bounded remote API adds **no `Respawned` variant**; do not report definite `NoService` merely because an old epoch vanished. A never-submitted, independently authenticated rejection may instead use a truthful definite existing result. |

`RemoteCallError` currently declares `NoService | Unreachable | Timeout | Busy | Indeterminate | AuthFailed | ProtocolError | NotSupported` but runtime returns only `NotSupported`. This table does not change that enum or claim that all outcomes are implemented. Generic async submit/await in Phase 03 is independent of first deadline-bounded synchronous remote call; `WaitCompletion` v1 accepts only `NET_RX` and `TIMER`. A `Future` that wraps blocking `sys_send` is not nonblocking IPC. Dropping an awaiter abandons waiting, not necessarily the underlying remote effect.

### 2.6 Nonblocking local call lifecycle — the shipped primitive, its contract, and what a migration would cost

Draft, 2026-10-09. Nothing here amends Spec 17 or its ratified masked reply discipline, and
nothing here is implemented beyond what the text cites.

The kernel already ships a bounded exact-operation call primitive. Measured on the x86_64
`test-hooks` lane (`docs/evidence/c2c-async-lifecycle-x86.{txt,log}`): **eight outstanding calls
from one Tier-1 Cell to one peer, exactly one correlated completion each, 0 lost, 0
mis-correlated** (the peer answered in reverse order, so operation identity cannot be replaced by
arrival order), p50/p99 ≈ 1.56/1.59 ms on TCG, and **one** `wait` round for the whole drain. The
same lane measures `sys_try_send` as *not* a submission mechanism: it is delivered only when the
receiver is parked in `Recv` and otherwise refused **in its return value** (`usize::MAX`), with
nothing queued.

**Operation lifecycle** (`kernel/src/task/async_ipc.rs`): `Queued → Dispatched → Terminal {
Reply | PeerGone | PreDispatchTimeout | Indeterminate | Cancelled }`.

| Property | Shipped behaviour | Consequence for a caller |
|---|---|---|
| Submission | `submit` copies the request into kernel-owned storage and reserves the reply slot under one scheduler lock hold; the caller may drop its stack buffer immediately | No pinned caller buffer, no bare stack pointer |
| Identity | The token binds owner tid, peer tid, peer cell **and** peer generation; the receiver reads it from the delivered message (`IpcCurrent`) | A completion cannot cross incarnations, and a stale generation cannot settle someone else's operation |
| Terminal kinds | `Reply`, `PeerGone`, `PreDispatchTimeout`, `Indeterminate`, `Cancelled` — see the §2.4 matrix for which caller-visible outcome each maps to | A dead or vanished peer is a definite outcome, not a hang |
| Reply retention | The kernel owns the reply until `take`; an undersized `take` buffer does **not** consume the result; taking a terminal releases the slot | No silent truncation, no double execution through re-taking |
| Capacity | Bounded operations per owner and a bounded receiver queue; a full owner set or queue returns `Busy` with nothing delivered | Congestion is a refusal, never a silent drop |
| Waiting | `wait(ticks)` is a timer-bounded park; `WaitCompletion` v1 has only `NET_RX`/`TIMER` sources | A caller polls on a timer granularity; the measured drain needed one round, not a busy loop |

**The reply is per-operation, not per-sender — and that is the whole cost of migrating existing
callers.** A bounded caller's terminal is produced by `IpcReply`: only that handler reaches
`async_ipc::terminal`, which requires the operation to be `Dispatched`. An ordinary
`Send`-to-sender reply never touches the operation slot. Measured directly rather than inferred: a
peer that answered eight bounded requests with `sys_send` left one operation `Reply` (taken while it
was still alive) and terminalised the other seven `PeerGone`.

Therefore:

  * an **opt-in** async API requires both sides to opt in — the serving side must answer with
    `IpcCurrent` + `IpcReply` instead of the ratified masked `sys_send(sender_tid, …)` (Spec 17 §2,
    §6). The cross-tier fixture does exactly this and completes every operation
    (`docs/evidence/c2c-cross-tier-exchange-x86.{txt,log}`,
    `docs/evidence/c2c-named-tier2-service-x86.{txt,log}`);
  * making the **blocking** `LocalEndpoint::call` / `ServiceRef::call` stop stranding a caller whose
    provider dies mid-call forces either a service-wide reply migration — a change to a *ratified*
    IPC path, i.e. a Spec 17 §9 entry plus two Law-1 confirmations — or a kernel change that lets a
    plain masked reply terminalise a bounded operation, which is the same class of ABI/contract
    question. Neither is proposed or approved by this text.

**Open proof obligations, not settled by the measurements above:** waiting on several sources in one
park (the local drain used a timer-bounded `wait`), cancelling an already-dispatched operation, the
two-hart publication/wake race (a completion published on one hart must be seen by a waiter parked on
another, with no lost wakeup), and what happens to a retained reply when the caller drops its
interest.

### 2.5 Liveness and safety boundaries

Local death may be confirmed against kernel-owned live generation. A remote beacon, lease, TLS or Noise disconnect indicates **suspected loss/partition**, not confirmed remote Cell death, and cannot authorize physical actuation or automatic failover; [Spec 14](14-distributed.md) retains the local interlock rule. `watch(remote)` and a broker-scoped death-notification syscall from v2 are **deferred** beyond unary RPC; no SpawnCap `NotifyOnExit` grant to broker. Session capacity pressure is `Busy`, not a death event. Public/fleet-scale exports, distributed leases, hole punching, promise pipelining and Tier-3 host service access each require a separate gate.

## 3. Boundedness and runtime constraints

- The network cell shares an 18-socket budget (including DHCP/ARP/other clients); existing Noise pool caps sessions at four and must not evict in-flight work to fit another peer. Return `Busy` and retain current session state; four peers are **not** a fleet-scale guarantee.
- Current broker uses bounded local ingress/worker/reply roles under a watchdog. A future relay or direct handshake must yield, re-arm heartbeat and enforce exact frame/deadline limits rather than block the broker's receive loop. CPU, socket and queue pressure must not starve safety-critical local work.
- Existing **local benchmark** state has `LOCAL_REQUEST_QUEUE_CAP = 16`, `LOCAL_REPLY_QUEUE_CAP = 16`, `IN_FLIGHT_CAP = 32` and `PER_CALLER_WINDOW = 4` (`local_queue/state/types.rs:4-8`). These are caller-Cell/TID quotas, **not** a remote peer quota. First authenticated remote ingress needs separate bounded per-NodeId charging and a measured fairness/bytes budget; do not let a peer spend unlimited capacity in a local exported service.
- `UdpRecv`'s 512-byte packet limit and encrypted LAN beacon do not define a working UDP remote-call transport. First remote RPC is relay-only under the protected authority; direct TCP is an optimization after the isolated relay oracle. No STUN/ICE or multicast identity promotion in baseline scope.
- Local sender attestation reserves 32 bytes in its 4 KiB recv buffer. The V1 envelope max 3,712 bytes already accounts for the conservative inline TCP and Noise-tag bounds; a reply must be checked against its own local response framing too. No operation may silently truncate, drop a completion or exceed the fixed dedup/source capacity.
- Async IPC is a separate capability. `TrySend` succeeds only for a receiver already ready in matching `Recv`; `WaitCompletion` source bits are currently `NET_RX` and `TIMER`. A general IPC completion source or new syscall must not be assumed implemented or borrowed from a private kernel helper.

## 4. ABI and caller inventory (proposal, no additions)

| Existing symbol / caller | Current guarantee | Review needed before a behavioral change |
|---|---|---|
| `ViSyscall::{Send=0, Recv=1, TrySend=4, RegisterService=205, LookupService=206}`; `ostd::ipc::service_call_typed` | Spec-17 masked request/reply, per-receiver byte-0 namespace and existing live-provider TID lookup. `Recv` flag `RECV_ATTEST_CALLER` is already ratified; `RecvTimeout` is not attested. | ADR-0023 adds **one** opcode (`LookupServiceBound = 429`) returning a `{tid, cell_id, generation}` record, keeps 206 byte-compatible, and adds no send opcode. **Implemented and Law-1 FROZEN 2026-10-08**, verified end-to-end on x86_64 QEMU (`docs/evidence/local-service-lifecycle-x86-qemu.{txt,log}`); drift is caught by `scripts/check-lookupservicebound-law1-digests.sh`. `LocalEndpoint::call` still uses the synchronous path (Phase 03). |
| `ostd::cluster_endpoint::{CellMethod, LocalEndpoint, RemoteEndpoint, CellEndpoint, RemoteCallError}` | Local direct copied call and remote explicit `NotSupported`. Only `libs/ostd/tests/cluster-endpoint.rs` currently exercises remote endpoint construction/call; no native app uses it for real remote delivery. | Authentication-bound remote descriptors, definite/uncertain result mapping and any proposed `Respawned` variant must be reviewed against exported SDK consumers; no silent alias or new enum variant now. |
| `types::c2c::{RetryClass, ServerEpoch, RelativeDeadline}` and `api::services::cluster::{CellNetId, ClusterId, PeerTicket}` | V1 RetryClass wire IDs 1/2/3, nonzero volatile ServerEpoch, nonzero relative milliseconds; peer ticket lists IPv4/relay hints. | Do not change wire values or assume ticket implies authority. Tie remote descriptor to the authenticated session and incarnation before enabling. |
| `ViSyscall::WaitCompletion=242`; `api::abi::completion::{source::NET_RX, source::TIMER}` | These two event sources only; no generic RPC completion. | Phase 03 must prototype bounded ownership/wakeup before asking for a new public ABI and two Law-1 confirmations. Bounded synchronous remote call may precede it. |
| Spec-17 §3 byte-0 registry and §9 amendments | Receiver-disambiguated postcard protocol; the legacy broker oracle's first eight request bytes are unrestricted, so **no byte-0 value is collision-free while its parser shares a receiver with RPC**. | Prefer one protocol per broker image profile on existing `service::NET_BROKER = 8`, then remove the legacy-only parser and callers at cutover. Prove mutually exclusive packaging and no decode fallback. If simultaneous protocols are required, review a second registered receiver/service ID through Law-1; remote `(service_id, export_id)` are **not** byte-0 values. |
| `Grant*`, Tier-2 admission, SAS ring and Tier-3 virtio | Spec 22 containment denies private-root grants; kernel repair owns its root/revoke work. Ring copies words, guest device I/O is not C2C RPC. | C2C Phase 02 consumes proved copied IPC, not grant cutover. Phase 08/09 need separate issuer/revocation and VM-guest identity gates; no change to these kernel paths during Phase-01 preparation. |

## 5. Behavioral proof matrix (required before promotion, not a passing test report)

| Scenario | Proof / phase |
|---|---|
| Local Tier-1 typed request/reply and masked receive amid queued input; attested thread belongs to parent Cell | Existing Spec-17 consumers plus Phase-02 live-service QEMU witness; wrong sender/missing attestation denied. |
| Tier-1→Tier-2 and Tier-2→Tier-1 request/reply, invalid buffer, peer respawn | Phase-02 RV64 admitted-profile two-Cell witness after kernel-repair safe-root proof; verify no DomainGrant, SAS fallback or stale response. |
| Relative deadline on queued local work vs possibly submitted/remote dispatched work | Host state tests for exact `Timeout` versus `Indeterminate`; confirm authority ownership and receiver execution counter in Phase-06 two-node oracle. |
| Wrong peer, mismatched NodeId/cluster, unregistered or disallowed export, config-only `scope=remote` without peer policy/live target, replay, boot/server restart | Phase-05 authenticated broker-state tests, then Phase-06 relay-only isolated two-node negative oracle; reject before dedup/local dispatch. |
| Correlated destination absence vs ambiguous relay write, two concurrent outstanding requests, stale correlation | ADR-0009 host codec tests are **server-only** evidence; authority AC-012 and Phase-06 client+broker oracle must prove live end-to-end correlation. |
| Cache full, duplicate in-flight, completed replay, expired non-idempotent request | Existing `c2c_dedup` host tests cover bounded local state, not remote delivery; Phase-06 exercises actual application execution and post-retention ambiguity. |
| Direct path, Tier-1 shared bulk, guest bridge, async completion | Separate Phase-07/08/09/03 witnesses respectively; none is implied by relay-only pass or by this contract proposal. |

## 6. Open gates and ratification checklist

- [x] Inventory current exported endpoint/identity/ABI types, current callers and V1 framing/capacity, with implemented-vs-proposed table above; **no code/ABI changed**.
- [x] Retire obsolete v2 assumptions in this draft: `CellAddr` sketch, unimplemented watch as unary-RPC prerequisite, per-cell remote authority, global UDP payload cap, `Respawned` claimed as an existing enum, and claim that async is mandatory before a bounded synchronous remote RPC.
- [x] Review the exact live local binding/recipient-generation representation with kernel repair owner; no edits to kernel/syscall/Spec-17 ratified clauses while ownership overlaps. **Design decided in [ADR-0023](../decisions/0023-local-service-generation-binding.md) (2026-10-08); the kernel-repair handoff was granted, the registry record + `LookupServiceBound = 429` landed the same day, and Law-1 checkpoint 2 froze the surface. Drift is caught by `scripts/check-lookupservicebound-law1-digests.sh`.**
- [ ] Review the proposed mutually exclusive broker image profiles against actual `tests/bench` oracle consumers, boot packaging, callback masks and the eventual clean cutover; prove one parser per TID, fail-closed unknown frames and no legacy caller in the typed profile. No runtime profile is added until portfolio promotion.
- [ ] Resolve how independently provisioned peer policy and live destination binding are joined to the boot-only five-field export registry; config presence alone is never remote admission. Prove no private export or `Public` promotion bypasses the missing factor.
- [ ] Measure per-peer ingress quotas before assigning a local service budget. Resolve the source-boot replay floor: uptime-derived beacon epoch is not a protected monotonic C2C epoch; require an authority-bound nonrollback incarnation or review a new replay model. Keep relay ingress disabled in either case.
- [ ] For the first bounded remote API, prefer existing `RemoteCallError::Indeterminate` for possible old-epoch execution; do not add a speculative `Respawned` variant. Before submission, only a separately authenticated definite rejection may use a definite result. Review old broker-session invalidation, per-peer export allowlist and results against consumers before ratification.
- [ ] Prove local origin deadline is not reset by relay queueing and that a received relative duration does not imply synchronized clocks or global expiry; classify origin timeout after ambiguous submission as `Indeterminate`.
- [ ] Record separate, explicit Law-1 confirmation #1 and #2 **for each** required exported ABI/enum/syscall/discriminant change; amend Spec 17 §9 only for actual ratified wire/ABI changes. No confirmation is inferred from accepting this plan or draft.
- [ ] Ratify this spec with the owners after conflict and negative-test review. Advance the [C2C portfolio](../../.agents/plan-portfolio.md) only after kernel file ownership is handed off; Phase 04 requires the KMS/Silo protected identity/time/persistence GO and post-Build AC-012 before any relay route, Phase 06 requires retained isolated two-node evidence before development remote enablement. Physical/production admission is separately gated.

**Stop line (2026-09-27):** Document-only Phase-01 inventory/draft may continue alongside [kernel architecture repair](../../.agents/260927-0739-kernel-architecture-repair/plan.md); no C2C implementation that touches `kernel/src/task.rs`, `kernel/src/task/syscall.rs`, `kernel/src/memory/address_space.rs`, grant/pin/scheduler, public ABI or Tier-2 admission begins before the corresponding kernel-repair proof and explicit owner handoff. A draft is not Phase-01 completion.

## 7. Revision record

- **v3.2 (2026-10-09, draft amendment; no status change):** adds §2.6, the nonblocking local call
  lifecycle as the shipped primitive actually implements it, and **corrects a v3.1 clause that read as
  though moving local calls onto that primitive were a plain SDK change**. It is not: a bounded
  caller's terminal is produced by `IpcReply` — only that handler reaches `async_ipc::terminal`, which
  requires `Phase::Dispatched` — while an ordinary masked `Send` reply never touches the operation
  slot. Measured on the x86_64 lane: a peer answering eight bounded requests with `sys_send` left one
  operation `Reply` and terminalised seven `PeerGone`
  (`docs/evidence/c2c-async-lifecycle-x86.{txt,log}`). An opt-in async API therefore needs both sides
  to opt in, and the stranding fix for the blocking API costs either a service-wide reply migration
  (Spec 17 §9 + two Law-1 confirmations) or a kernel change to terminalise on a plain reply. §2.6 also
  records the operation lifecycle, submission-as-copy, identity binding, reply retention, bounded
  capacity and timer-bounded waiting, with what remains unproven. Measured numbers, not ratification.
- **v3.1 (2026-10-08, draft amendment; no status change):** §2.1 binding-lifetime text and the §5 existing-symbol row now point at [ADR-0023](../decisions/0023-local-service-generation-binding.md), which resolves the "design/ABI review" this draft deferred: the local binding axis is the existing per-Cell `(cell_id, generation)`, exactly one additive opcode (`LookupServiceBound = 429`) is proposed, `LookupService = 206` stays byte-compatible, no send opcode is added, and local service calls move onto the existing bounded exact-operation primitive. Source review also corrected a premise: TIDs are never re-issued within one boot, so a stale cached TID fails closed (`TargetGone`) rather than misdelivering; the reuse that exists is on `CellId` slots. Still a draft: not ratified, no ABI confirmed, no implementation.
- **v3 (2026-09-27, draft):** Rebased on ADR-0015's tiers, local-only broker/typed endpoint, ADR-0008 protected TLS and ADR-0009 correlated failures. Replaced the obsolete `CellAddr`/remote-watch-as-prerequisite and UDP-size assumptions with locality, identity, submission and evidence matrices. Source audit found an unrestricted legacy first-byte sequence; proposed mutually exclusive broker image profiles on the existing service ID rather than two colliding parsers. The beacon uptime epoch is not a protected cross-reboot C2C replay source; the static export registry has no peer allowlist or live binding; the V1 duration cannot establish a cross-node deadline. No ratification, new ABI, image profile or remote implementation is claimed.
- **v2 (2026-07-30, historical draft):** Node-level remote principal and partition-aware safety review; previous sketches were never ratified.

