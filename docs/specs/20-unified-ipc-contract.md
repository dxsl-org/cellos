# Spec 20 — Unified IPC Contract: One Backbone at Three Ranges (DRAFT v2)

> **Status**: Draft v2 2026-07-30 — revised after two adversarial reviews (security
> adversary + distributed-systems failure analyst). **Not normative until ratified.**
> Extends Spec 17 (IPC wire contract) and Spec 14 (distributed); depends on midori-lessons
> phase 02 (kernel-attested sender) and the Cell-to-Cell Anywhere stack
> (`.agents/260624-cell-to-cell-anywhere/plan.md`).
>
> **v1→v2 changes**: remote identity demoted to node granularity (was: per-cell path
> "verbatim" into local ACL — refuted); §2.4 death-watch split by safety class and moved
> off the Noise-session layer (was: one collapsed handling shape — refuted, breaks Spec 14
> physical-safety); respawn epoch added; partition handling forced into the return type;
> fleet claim gated to G2; foundation status corrected (remote forwarder is a stub today).

## 1. Context — why Cellos needs a declared backbone

Linux's de-facto service backbone (D-Bus) is four things: a name system, a typed message
protocol, lifecycle signals, and a who-may-call-whom policy. Cellos has all four — twice,
in two disconnected worlds (local SAS vs remote federation). A cell calls the local VFS
with `sys_send(tid, …)` and a remote sensor with `call_remote(CellNetId, service, method,
…)` — two address schemes, two identity models, two failure vocabularies. The backbone is
the single **contract** that removes that split.

It is a *contract*, not a component: no bus daemon (the D-Bus daemon is a bottleneck and a
CVE magnet). Kernel keeps mechanism, cells keep policy, this spec is the shared shape.

**Foundation status (corrected, verified):**

| Piece | Local (SAS) | Remote |
|---|---|---|
| Transport / NodeId / relay / Noise KKpsk0 | n/a | ✅ built (`net-broker/src/{transport,relay,identity}.rs`) |
| Typed request/response forwarding | ✅ Spec 17 | ⚠️ **stub** — `main.rs:151` `dispatch()` is TODO, `routing.rs:154` returns `self_tid` ("forward via Noise" deferred) |
| Kernel-attested sender | ⚠️ **phase 02 not landed** | — |

The remote `call`/`watch` paths in §2 therefore have **no runtime today**. This spec
designs the contract the forwarder must satisfy; the §6 prototype gate builds against it,
not against a claim that it already works.

## 2. Decision (proposed)

### 2.1 One address: `CellAddr` — range pinned at construction

```
CellAddr        = Local(LocalAddr) | Remote(RemoteAddr)
LocalAddr       = service: ServiceName
RemoteAddr      = node: CellNetId, service: ServiceName, epoch-aware
ServiceName     = the cell's install path (e.g. "/bin/vfs"), NOT a tid
```

- **Range is part of the type, not a runtime `is_local()` probe.** A local-only cell holds
  a `LocalAddr` that *cannot silently become remote*. This is the primary defense against
  code that is correct in test (resolves local) and broken in the field (resolves remote).
- **tid never appears in the contract.** Tids are unstable across respawn and forgeable as
  names (the `path_hint` hole). Path is the durable key.
- **Resolution binds to the kernel/registry entry, never the caller-supplied `path_hint`**
  (`loader.rs:177` is attacker-influenced). `RegisterService` is SpawnCap-gated
  (`syscall.rs:1903-1908`), so a non-privileged cell cannot register under a victim's name;
  the resolver trusts that registry, not the spawn-time hint.
- **Respawn epoch (required).** A resolved binding carries an incarnation epoch (init's
  restart count for that path). Replies and death notices carry the epoch. This is the same
  field that serves replay-dedup (§2.4). "Valid across respawns" means *the name resolves*,
  **not** that instance N−1 and N are interchangeable — they are distinguishable by epoch,
  and a mismatched-epoch reply is `Err(Respawned)`, never silently accepted.

### 2.2 Identity — node-granular across the trust boundary

Local and remote identity are **different principal types and do not share an ACL
namespace.**

```
local principal:  (SELF_NODE, path)   — kernel-attested (phase 02); trustworthy
remote principal: (peer NodeId)       — Noise-authenticated machine identity
                                         (prologue binds NodeId: transport.rs:144-147)
```

- The Noise prologue authenticates **only the machine**. A remote peer's broker is, by the
  trust model (Spec 18: remote ≤ Tier-2), potentially adversarial — it can assert any
  per-cell origin path it likes. Therefore **a remote origin authorizes at NodeId
  granularity.** Any per-cell path a peer reports is an *advisory label*, usable by a rule
  only after that rule explicitly declares it trusts that node to self-report — never fed
  "verbatim" into the local per-path ACL. (v1's verbatim-reuse claim is withdrawn.)
- `machine_id` (Spec 14 split-brain tiebreak) MUST be derived from the NodeId
  (truncated hash of the static X25519 public key) and checked against the
  Noise-authenticated `remote_node_id` — never accepted from the wire
  (`enrollment.rs:48,68-76` currently decodes it unbound → spoofable to win Primary).

### 2.3 One API — range in the type, partition in the return

```rust
// libs/ostd — sketch; names to bikeshed at implementation
fn call<Req, Resp>(addr: &RemoteAddr, req: Req, t: Timeout) -> Result<Resp, RemoteErr>;
fn call_local<Req, Resp>(addr: &LocalAddr, req: Req)         -> Result<Resp, LocalErr>;
fn send(addr: &CellAddr, msg: impl Serialize)               -> ViResult<()>;

enum RemoteErr { Timeout, Unreachable, Respawned, NoService, Remote(u8) }
```

- **The caller of a remote `call` MUST match `Timeout | Unreachable | Respawned`** — the
  paper Waldo 1994 warns against a single call signature that hides the range; v2 answers by
  making the *failure modes that only exist remotely* unrepresentable in the local path and
  unignorable in the remote path.
- A mandatory `timeout` converts partition into a typed error, **not** into a silent retry.
  Automatic retry is not in the contract (it double-executes non-idempotent methods — see
  §2.4 dedup).
- Same postcard-typed messages both ranges; Spec 17's byte-0 discriminant registry is the
  **single** schema registry (remote `method_id` allocated from the same governed table).
  Local fast path keeps today's rendezvous cost — this spec adds **zero** overhead to
  SAS-local IPC.
- Payload size across the range boundary is normatively capped at the UDP-safe size
  (≈480 B, matching VFS in Spec 17 §5) **unless** the relay-TCP path is guaranteed; there is
  no implicit fragmentation layer (none exists). A near-4 KiB local request has no automatic
  remote form.

### 2.4 Failure vocabulary — two liveness sources, safety class in the signal

`watch(addr) -> WatchHandle` gains a remote sibling, but liveness is sourced by **class**,
not by one collapsed mechanism:

```
WatchFired { addr, epoch, seq, reason }
reason ∈ {
  peer_confirmed_dead,   // peer's own local NotifyOnExit fired for that service
  peer_indeterminate,    // lease/beacon loss or session death — MIGHT be a partition
  transport_evicted,     // LOCAL session eviction (K exhaustion) — NOT a peer statement
}
```

- **A `WatchFired` never authorizes actuation.** Restart-local vs failover vs safe-stop is
  the supervisor's decision, and the Spec 14 §PHYSICAL-SAFETY local interlock
  (`14-distributed.md:76-90`) is the only thing that authorizes driving an actuator.
  `peer_indeterminate` maps to SAFE/STOP (the unreachable peer may still be self-granting its
  role for up to `PEER_LOSS_MS = 9000`); only `peer_confirmed_dead` permits failover, and
  only after the local interlock re-checks. "Identical handling shape" from v1 applies to
  *plumbing* (cleanup/reconnect) exclusively — the safety decision is exactly the
  distinction v1 wrongly erased.
- **Machine-reachability watches ride the beacon/lease layer**
  (`BEACON_INTERVAL_MS = 1000`, `PEER_LOSS_MS`), which is **not** bounded by the K≤4 Noise
  pool. Only per-service `peer_confirmed_dead` needs a session. This decouples watch count
  from K (see §3) and supplies `peer_indeterminate` for free.
- `WatchFired` carries a per-watch monotonic `seq`; `transport_evicted` MUST NOT be
  delivered as a peer death. LRU eviction of a live session (`transport.rs:273-289`) is a
  local resource event, never a statement about the peer.
- **Remote watch requires a broker-scoped, non-SpawnCap kernel primitive** that can observe
  only services the broker itself brokers — not arbitrary tids. Granting the broker full
  SpawnCap `NotifyOnExit` (syscall 204, gated at `syscall.rs:1866-1873`) would make it a
  confused-deputy supervisor and violate §2.5. This primitive is a Law-1 addition and a hard
  prerequisite; §2.4 does not work without it.

### 2.5 Explicit non-goals

- No transparent RPC that hides the range (§2.1/§2.3 pin it in the type).
- No bus daemon; no broker in the kernel. Kernel's new duties are exactly two, both bounded:
  phase-02 sender attestation (local) and the broker-scoped watch primitive (§2.4).
- No automatic retry, no exactly-once, no cross-node time/order/consensus (Spec 14 leases
  stay optimistic hints).
- No change to Spec 17's local framing, recv-mask rule, or blocking discipline.

## 3. Constraints the design must survive (verified)

IPC buffer 4096 B; `UdpRecv` 512 B + 6 B header; `MAX_SOCKETS = 18` (shared DHCP/ARP/user);
Noise `MAX_SESSIONS = 4` with **LRU eviction of live sessions** (`transport.rs:40,273-289`);
broker is NORMAL priority under a ~500 ms RT watchdog (`main.rs:91`) on a **single dispatch
thread** (`main.rs:126`).

**Two structural ceilings the "fleet backbone" claim must respect:**

1. **K≤4 caps live remote peers at ~4.** Full-mesh watch-all survives only **N≤5**; the G1
   plan itself gates N>10 behind raising `MAX_SOCKETS` + K (`plan.md:434`), and Spec 14 scope
   is 2 nodes. → The contract claims a **backbone at ranges**, not a **fleet at scale**;
   fleets beyond K are explicitly deferred to G2 gossip. A new watch at K-exhaustion returns
   `Err(Busy)` (fail-loud, Spec 17 §7) — it MUST NOT evict a session with an in-flight
   request or an active watch.
2. **Single-thread blocking connect trips the watchdog.** A blocking multi-second
   `TcpConnect` on the dispatch thread misses the 500 ms heartbeat → watchdog kills the
   broker → every session drops at once → correlated fleet-wide false death. Connect/
   handshake MUST be a bounded state machine that yields and re-arms `sys_heartbeat` inside
   the watchdog window. Broker death must be distinguishable by watchers from peer death
   (local-synthesis source vs remote).

## 4. Resolved questions (were open in v1)

| # | v1 question | v2 answer |
|---|---|---|
| 1 | respawn epoch | **Required.** Epoch in binding + reply + WatchFired; mismatch → `Err(Respawned)` (§2.1/§2.4). |
| 2 | remote per-cell origin meaningful? | **No** at cell granularity across an untrusted machine — node-granular only; per-cell path is an advisory label (§2.2). |
| 3 | backpressure across ranges | Payload capped UDP-safe unless relay-TCP guaranteed; no implicit fragmentation; first thing dropped under fan-out is an in-flight reply via session eviction — forbidden, returns `Err(Busy)` (§2.3/§3). |
| 4 | watch scalability | Reachability on beacon/lease (unbounded by K); per-service death on session (≤K); fleet >K → G2 (§2.4/§3). |
| 5 | replay/nonce | **Required.** Caller-scoped request id (shares the epoch field); ingress dedups in a bounded window — otherwise "at-most-once" is false for retried calls (§2.4). |
| 6 | registry governance | One method_id table (Spec 17 registry); allocation governed there; version skew → typed `NoService`/`Remote(err)`, never silent. |
| 7 | ingress quota | Charged to the local receiving service's quota; remote caller cannot spend a local cell's budget beyond its own rate limit — **open for prototype measurement**. |

## 5. Relationship to existing work

| Depends on | Why |
|---|---|
| midori-lessons phase 02 | kernel-attested local sender — the *only* trustworthy half of §2.2 |
| midori-lessons phase 07 | async reactor — `call`+timeout wants completion queues, not thread-block (and §3 constraint 2) |
| CTC-Anywhere stack | transport/NodeId/relay built; **forwarder still a stub** (§1) |
| Spec 18 tiers | remote caller ≤ Tier-2; domain cells use this same contract |
| Spec 14 | §2.4 safety classes are the missing per-service signal; physical-safety mandate is load-bearing, not decoration |

Amends **Spec 17** (becomes the local profile; registry shared) and **Spec 14** (adds the
per-service failure signal; doctrine unchanged).

## 6. Ratification checklist

- [ ] Fold red-team v2 (done in this draft) — remaining: §4 Q7 quota needs measurement
- [ ] Law 1 inventory: `CellAddr` types, `RemoteErr`, the broker-scoped watch primitive,
      `machine_id`-from-NodeId — each with a 2× confirmation plan
- [ ] Land phase 02 first (identity root) — this spec cannot be prototyped honestly without it
- [ ] Prototype gate on 2-node QEMU: `CellAddr` resolution + epoch + `watch` safety classes,
      **before** any cell migrates off raw-tid addressing, and **before** the fleet claim is
      made for N>K
- [ ] Convert broker connect/handshake to a yielding state machine (§3 constraint 2) as a
      prerequisite, not a follow-up
