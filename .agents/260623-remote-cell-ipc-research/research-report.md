# Remote Cell IPC Research Report
**Date**: 2026-06-23  
**Scope**: How Cells on Machine A can communicate with Cells on Machine B  
**Status**: Research complete — no implementation

---

## Current State of IPC in Cellos

### IPC Model: TID-addressed, copy-on-receive

**Verdict**: IPC is fully intra-machine today; TIDs are local integers with no cross-machine meaning.

- `sys_send(target: usize, msg_ptr, msg_len)` / `sys_recv(mask, buf_ptr, buf_len)` — kernel copies bytes directly between caller stacks in SAS (`kernel/src/task.rs:912`, `copy_nonoverlapping`)
- Identifiers: TIDs are `usize` local counters; services are looked up via `sys_lookup_service(service_id: u16)` returning the live TID (`libs/api/src/syscall.rs:574–582`)
- Message format: postcard-serialized enums over a **4 KiB buffer** (`IPC_BUF_SIZE = 4096` — `libs/api/src/ipc.rs:21`); large payloads go via `GrantAlloc/GrantShare` (up to 16 MiB zero-copy, `MAX_GRANT_PAGES = 4096` — `kernel/src/task/syscall.rs:65`)
- No message queue — sender blocks in `TaskState::Sending` until receiver calls `sys_recv`; or immediate copy if receiver is already waiting (`kernel/src/task.rs:894–938`)
- Advanced: `SendGather`/`RecvScatter` (up to 8 iovec segments), `RecvTimeout` with deadline, `NotifyOnExit` death watch
- Wire format: `postcard` (varint discriminant + varint lengths) — already serialization-friendly, no schema change needed for remote transport

**Source**: `kernel/src/task.rs:837–941`, `libs/api/src/ipc.rs`, `libs/ostd/src/syscall.rs:616–699`

---

### Network Stack: smoltcp, TLS 1.3, TCP/UDP/DNS/MQTT

**Verdict**: The network layer is already capable and TLS-authenticated. The gap is above the network layer, not within it.

- Net Cell (`cells/services/net`) drives smoltcp TCP/IPv4 stack; exposes `NetRequest::TcpConnect/Listen/Accept/Send/Recv` via typed IPC to consumer Cells
- TLS 1.3 via `embedded-tls 0.19`; cert chain verification via `ViTlsProvider` (leaf→root, expiry check against RTC, SNI validation) — `cells/services/net/src/tls/provider.rs`; fleet CA default (`tls-ca-private` feature), Amazon/Let's Encrypt opt-in
- HTTPS end-to-end verified (commit `af20757d`); `flush()` after `write()` is required (embedded-tls buffers internally)
- Cap gate: `NetworkCap` in manifest required to call Net Cell (`docs/specs/07-networking.md:§5`)
- Missing: multicast/broadcast not yet shipped; disk-loaded cert bundles deferred

**Source**: `cells/services/net/src/main.rs:1–60`, `docs/specs/07-networking.md:§1–6`

---

## Gap Analysis: What Is Missing for Cross-Machine Cell Communication

**Verdict**: Five distinct gaps exist; none is trivially small.

| Gap | Description | Severity |
|-----|-------------|----------|
| **Address space boundary** | LBI (Rust type system) does not cross machine boundaries. A remote Cell is NOT in the same SAS — pointer identity means nothing. | Critical — fundamental |
| **TID namespace** | TIDs are local counters (usize). `sys_send(3, ...)` on Machine A sends to Machine A's tid=3, not anything on Machine B. | Critical — no remote addressing |
| **Serialization contract** | Current IPC passes raw bytes + postcard. For remote transport you need: field stability, versioning, schema evolution. Postcard has no built-in schema versioning. | High |
| **Authentication & trust** | LBI proves "this Rust code is correct" but proves nothing about a remote machine. A remote Cell can be a compromised host sending correctly-formatted bytes. | High — security boundary shift |
| **Capability semantics** | Capabilities (ZST tokens, `CapSet`) are in-process type-level constructs. They cannot be serialized and re-inflated on a remote machine with the same authority semantics. | High |

**Source**: `docs/specs/01-core.md:§1`, `libs/api/src/syscall.rs:274–299`, `kernel/src/task/syscall.rs:261`

---

## Prior Art Analysis

### Erlang/OTP: Transparent Node-Local PID + TCP Distribution

**Verdict**: The gold standard for transparent location, but requires immutable message semantics and a global atom table — both absent in Cellos today.

- PIDs encode `{node, process, serial}` — remote PIDs carry the node reference inline, making them globally unique without a directory
- Distribution protocol: EPMD (port mapper) + direct TCP between nodes; messages serialized to External Term Format (ETF = versioned binary format with schema)
- `send(Pid, Msg)` is location-transparent — kernel (ERTS) checks if PID is local or remote, routes accordingly; the application sees zero difference
- **Critical ingredient**: all inter-process state is immutable (functional values only) — no pointer aliasing across nodes is possible because there are no shared pointers. Cellos's owned-buffer model (`Box<[u8]>`, not `&mut [u8]`) maps closely to this
- Registered names are node-local; global process registry (`global` module) adds a distributed directory with conflict resolution
- Links and monitors work cross-node via the distribution protocol — `NotifyOnExit` has an analogue here

**Adoption risk for Cellos**: Node identity (`machine_id`) + service registry extension needed; ETF-style versioned encoding needed to replace raw postcard

**Source**: [Erlang Distributed Docs](https://www.erlang.org/doc/system/distributed.html), [The BEAM Book](https://blog.stenmans.org/theBeamBook/)

---

### Plan 9 / Inferno Styx: Everything-Is-a-File Over 9P

**Verdict**: Elegant for resource sharing but maps poorly to Cellos's capability-service model; requires a fundamentally different service abstraction.

- 9P lifts Unix syscalls (`open/read/write/stat`) into RPCs sent over any transport (pipe, shared memory, TCP)
- Location transparency: a process's namespace can contain mount points from remote machines; `cat /net/tcp/1234/data` works identically whether local or remote
- Key mechanism: `mount(fd, path, flags)` — bind a 9P server to a local namespace path; the kernel interposes on all syscalls to that path
- **Lesson for Cellos**: the "mount" abstraction could map to "attach a remote Cell's service endpoint into the local service registry" — but Cellos services speak typed postcard enums, not the file R/W model
- Inferno's Styx variant confirmed: "behavior over a network is identical to behavior locally" — achieved by making the network transport stateless and the protocol self-describing

**Source**: [Styx Architecture Paper](https://www.inferno-os.org/inferno/papers/styx.pdf), [9P Protocol](https://9p.io/sys/doc/9.html)

---

### Cap'n Proto RPC: Promise Pipelining + Capability Transport

**Verdict**: Closest fit to Cellos's capability model; the CapTP-inspired "vat" model maps directly to "Cell."

- Cap'n Proto introduces **vats** (isolated execution contexts) that can hold object references (capabilities) and communicate via bilateral connections
- Promise pipelining: `A.foo().bar()` — sends `bar()` without waiting for `foo()` to return; the runtime pipes the promise through. Critical for latency in capability chains
- **Object capability transport**: a capability reference can be included in a message payload and re-inflated on the remote side as a live proxy object — this is the key primitive missing from Cellos IPC
- Three-way introductions: A holds cap to B and cap to C; A sends C a reference to B; C can now talk to B directly without re-routing through A. This maps to `GrantShare` extended to remote targets
- SturdyRefs: long-lived capability references that survive connection drops and can be re-established
- **What it requires**: a stable serialized capability token (not a ZST type), a connection layer, and a broker that holds the mapping from token → live Cell reference

**Source**: [Cap'n Proto RPC](https://capnproto.org/rpc.html), [DeepWiki capnproto](https://deepwiki.com/capnproto/capnproto/3.3-rpc-system)

---

### Singularity / Midori: Typed Channel Contracts

**Verdict**: Directly confirmed Cellos's IPC model is correct; distributed extension uses channel contracts over TLS — copyless message passing over network is feasible.

- Singularity SIPs use bidirectional typed channels with contracts; "Typing Copyless Message Passing" paper shows this is compatible with network transport
- Midori (extended Singularity) used the same channel model for cross-machine RPC; channels over TLS were the remote variant
- **Critical finding**: the "channel contract" (typed message enum) is ALREADY in Cellos via `VfsRequest`/`NetRequest` enums. The serialization format (postcard) just needs versioning
- Midori's remote channel retained ownership semantics — a message is owned by exactly one side at all times, even in transit (the network buffer holds ownership). This maps perfectly to Cellos's owned-buffer law

**Source**: [Singularity MSR](https://www.microsoft.com/en-us/research/project/singularity/), [Singularity: Rethinking the Software Stack](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/osr2007_rethinkingsoftwarestack.pdf)

---

### seL4/Genode: Capability Delegation via Kernel Objects

**Verdict**: Hardware-capability model; valuable for the security design but impractical to extend over network without a broker.

- seL4 capabilities are kernel objects protected by hardware; minting creates a weaker derived cap; delegation goes through `CNode` operations
- Genode's capability-passing uses the kernel's IPC mechanism: capabilities are opaque kernel-managed integers, delegated only under strict authority rules
- **For remote**: seL4 has no native cross-node capability transport; research projects (CapDL) describe static capability distribution. Distributed seL4 is an open research problem
- **Lesson**: capability tokens for remote use need to be serialized as signed tokens (e.g., HMAC or P-256 signature), not raw kernel objects. Cellos's Silo (hardware key isolation) is the right backing for remote capability tokens

**Source**: [seL4 Manual](https://sel4.systems/Info/Docs/seL4-manual-latest.pdf), [Genode IPC Architecture](https://genode.org/documentation/genode-foundations/21.05/architecture/Inter-component_communication.html)

---

### Orleans Virtual Actors: Transparent Location via Directory

**Verdict**: Best reference for the "named endpoint" approach; grain identity = service name, location is an implementation detail.

- Grain identity is a stable string or GUID; a distributed directory maps identity → physical node:activation
- `ActorProxy.GetGrain<IFoo>("my-sensor")` — location-transparent call; runtime resolves and routes
- Key design: grains are VIRTUAL — they always "exist" even when not activated; the runtime activates on first call and deactivates when idle
- Cache hit rate >90% in production — local directory cache avoids most remote lookups
- **Lesson for Cellos**: `sys_lookup_service(service_id)` is already a directory lookup. Extending it to return a `(machine_id, tid)` pair instead of just `tid` is the minimal change needed for location transparency

**Source**: [Orleans MSR](https://www.microsoft.com/en-us/research/project/orleans-virtual-actors/), [Orleans Wikipedia](https://en.wikipedia.org/wiki/Orleans_(software_framework))

---

## Architecture Options: Ranked by Fit

### Option A — Gateway Cell (recommended for G2)

**Summary**: A dedicated `net-broker` Cell on each machine handles serialization, routing, and auth. Local Cells speak normal TID-addressed IPC. The broker translates to/from a wire protocol over TLS TCP.

```
Machine A                              Machine B
[Cell X] --sys_send(broker_tid)--> [NetBroker A]
                                         |
                               TLS TCP (mTLS, DICE cert)
                                         |
                                   [NetBroker B] --sys_send(target_tid)--> [Cell Y]
```

**Mechanics**:
1. Cell X calls `sys_lookup_service(service::REMOTE_BROKER)` → gets broker TID
2. Sends `RemoteIpc { dest_machine: MachineId, dest_service: u16, payload: &[u8] }` to broker
3. Broker serializes (postcard + framing), opens TLS conn to Machine B's broker (or reuses pooled conn)
4. Machine B broker calls `sys_lookup_service(dest_service)` → local TID, forwards
5. Response is the reverse path

**Fit with Cellos**:
- ✅ No kernel changes (pure userspace Cell)
- ✅ Owned-buffer law respected: broker copies into `Box<[u8]>` before sending over net
- ✅ TLS 1.3 already ships — mTLS with fleet CA is one feature flag away
- ✅ Capability model: NetworkCap gates the broker Cell; local Cells need no new caps
- ✅ Hot-swap / never-die: broker crash → supervisor respawns; local Cells use `RecvTimeout` + retry
- ⚠️ Not location-transparent: Cell X must know it is talking to a remote service
- ⚠️ Extra 1-2 IPC hops per request (local send → broker → net → remote broker → local recv)

**Serialization**: extend postcard schema with a `RemoteIpcRequest` wrapper in `libs/api/src/ipc.rs`. No Law 1 change (adding a new enum variant is additive). Schema version field required.

**Security**: mTLS with `tls-ca-private` fleet CA authenticates machine identity. Per-service authorization: broker checks caller's `CapSet` before forwarding; remote machine policy blob (`POLICY.BIN` pattern) controls which services are accessible from which machines.

**Adoption risk**: Low. All primitives exist. Effort: ~3-4 weeks for a production-quality broker Cell.

---

### Option B — Named Endpoint / Service Registry Extension

**Summary**: Extend `sys_lookup_service` to return `(machine_id, tid)` pairs. The kernel (or a thin shim in ostd) transparently routes `sys_send(remote_tid)` via the Gateway Cell.

**Mechanics**:
1. Add `MachineId` (u64, e.g. MAC-derived or DICE cert hash) to the service registry
2. `sys_lookup_service(service::SENSOR_AGGREGATOR)` returns `RemoteTid { machine: MachineId, local_tid: usize }`
3. `sys_send` with a remote TID is intercepted by the kernel (or ostd shim) and routed to the Gateway Cell
4. From the caller's perspective: no code change except `lookup` + `send` as normal

**Fit with Cellos**:
- ✅ Location-transparent for the application Cell
- ✅ Supervisor-tree can manage remote Cells via `NotifyOnExit` extended to remote death
- ⚠️ Requires kernel change (extending TID namespace) OR an ostd shim that wraps `sys_send` — kernel path touches Law 1 (`libs/api`); ostd shim is safer first step
- ⚠️ Machine identity needs a MachineId allocation/distribution mechanism
- ⚠️ "Remote TID" semantics differ from local TID (blocking behavior, timeout behavior) — must document clearly

**Adoption risk**: Medium. The ostd shim variant avoids kernel changes entirely; the kernel-native variant needs careful Law 1 review.

---

### Option C — Capability Token Transport (G3 / high-security)

**Summary**: Capabilities can be minted as signed tokens (HMAC or ECDSA P-256 over Silo) and sent to remote machines. The remote machine's kernel verifies the token before granting access.

**Mechanics**:
1. Machine A generates a capability token: `sign(CapToken { service_id, allowed_ops, expiry }, silo_key)`
2. Token is sent to Machine B in-band (e.g., embedded in a TLS message)
3. Machine B kernel verifies signature against the fleet root key, checks expiry, issues a local ephemeral cap handle
4. Remote Cell holds the handle and uses it for subsequent IPC — no further round-trips to Machine A

**Fit with Cellos**:
- ✅ Strong security: compromised Machine B cannot forge caps from Machine A
- ✅ Silo already provides ECDSA P-256 signing (`ostd::silo::SiloHandle`) — hardware-backed on G2
- ✅ Extends the existing DICE/RIoT attestation roadmap (`docs/project-roadmap.md:§G.2`)
- ⚠️ Complex: requires token format design, revocation (short-lived certs or CRL), key distribution
- ⚠️ Depends on Silo (G2 only) and DICE attestation (roadmap item, not yet built)
- ⚠️ Over-engineered for G1/G2 use cases (robot fleet, cloud inference)

**Adoption risk**: High for G1/G2. Right call for G3 multi-tenant or healthcare/critical-infra scenarios.

---

### Option D — Shared-Address-Space Cluster (REJECTED)

**Summary**: Extend the SAS to span multiple machines via RDMA, making all Cells globally addressable.

**Why rejected**:
- LBI relies on Rust's type system enforcing memory safety within ONE compiler's type-checked SAS. Remote machines have separate compilation units — the Rust type system proves nothing about remote code
- RDMA requires specialized hardware (InfiniBand/RoCE) — incompatible with the G1 robot SBC hardware target
- The fundamental premise (SAS + LBI = safety) breaks at a machine boundary; you would need hardware memory isolation (MMU/IOMMU) at the network level, which is exactly what Cellos avoids locally
- This is the approach of Barrelfish's Multikernel — a research result, not production-proven

**Source**: docs/specs/01-core.md:§1 (LBI philosophy)

---

## Key Design Decisions That Must Be Made

1. **MachineId format**: MAC address hash vs DICE CDI-derived ID vs operator-assigned UUID. DICE is the right long-term answer (maps to attestation); MAC is the fast path for G2.

2. **Wire protocol**: postcard + length-prefix framing vs. Cap'n Proto vs. custom. Recommendation: postcard (already in use, zero new deps) + explicit 4-byte little-endian length prefix + schema version byte. Cap'n Proto is 50+ KLOC dependency — YAGNI.

3. **Connection management**: per-request TLS handshake (simple, ~2ms latency) vs. persistent mTLS connection pool (complex, ~10µs latency). For robot fleet (G1): per-request acceptable. For G2 inference server: pooled connections needed.

4. **Capability boundary at the machine edge**: Does the remote machine trust the caller's capability set? Options: (a) trust the sending machine (based on mTLS cert) and grant `CapSet::ALL`; (b) each service declares a remote access policy; (c) DICE-attested caller capability forwarding. Recommendation: (b) for G2 — service declares `remote_allow: &[MachineId]` in its manifest.

5. **Failure semantics**: when the remote machine goes down, the broker returns `ViError::Timeout` or a new `ViError::RemoteUnreachable`. The caller's supervisor must handle this identically to a local Cell death (`NotifyOnExit` pattern).

---

## Risks and Open Questions

| Risk | Severity | Mitigation |
|------|----------|------------|
| **Latency cliff**: local IPC is ~2µs (direct copy); remote is ~500µs-5ms (TCP RTT). Applications that assume sub-10µs latency will break silently if a service is moved remote | High | Document latency tier in service manifest; never make remote routing transparent for RT Cells |
| **Serialization schema evolution**: postcard has no built-in versioning; a remote Machine B running Cellos v1.1 may have a different `VfsRequest` enum than Machine A running v1.2 | High | Add `schema_version: u8` to `RemoteIpcRequest`; broker rejects mismatched versions; semantic versioning on IPC types |
| **mTLS cert distribution**: fleet CA `tls-ca-private` is a self-signed cert compiled into the OS image. Rotating it requires a firmware update. | Medium | Short-lived leaf certs signed by the embedded CA; rotation is leaf-cert-only (no firmware update) |
| **TID stability across respawns**: a service that crashes and restarts gets a new TID. Remote broker caches the old TID. | Medium | Remote broker uses `sys_lookup_service` (not cached TID) for every forward; the service registry handles the respawn-and-reregister transparently |
| **Grant API cross-machine**: `GrantShare` shares physical frames between tasks in the SAME SAS. Zero-copy is impossible across machines — the broker MUST copy. This is a performance regression vs. local IPC. | Medium | Accept the copy for G2; design the wire protocol so copy is done once (sender → broker → net — no intermediate copy in broker) |
| **SAS identity invariant**: freed frames must stay identity-mapped. Broker's receive buffers must use `GrantAlloc` (not stack allocation) to avoid fragmentation and SAS identity violations | Medium | Broker uses `GrantRegister` for persistent receive buffers (pattern already used by the TLS transport) |

---

## Recommended Approach

**For G2: Gateway Cell (Option A) as the first implementation.**

**Rationale**:
1. Requires zero kernel changes — everything is a userspace Cell, which is philosophically correct for Cellos's architecture
2. The TLS 1.3 transport is already proven working (commit `af20757d`); only the `RemoteIpcRequest` message format and the broker routing logic need to be written
3. Cellos's own reliability primitives (supervisor restart, `NotifyOnExit`, `Heartbeat`) protect the broker transparently
4. The owned-buffer law (`Box<[u8]>`) is naturally satisfied: the broker owns the message while it is in transit
5. An explicit non-transparent API forces application Cells to handle remote latency explicitly — the right default for a real-time OS (no silent degradation)
6. Extension path: once the Gateway Cell is proven, add an ostd shim that makes it semi-transparent (Option B) without changing the broker internals

**Concrete first steps**:
1. Define `MachineId` as a `[u8; 8]` (first 8 bytes of SHA256 of the DICE CDI, or MAC+timestamp for dev) in `libs/api/src/syscall.rs`
2. Add `service::REMOTE_BROKER: u16 = 6` to the service registry
3. Add `RemoteIpcRequest { dest_machine: MachineId, dest_service: u16, payload: &'a [u8] }` and `RemoteIpcResponse` to `libs/api/src/ipc.rs` (additive, no Law 1 breaking change)
4. Write `cells/services/net-broker/` — a Tier-1 Rust Cell that manages one TLS connection per remote machine and forwards IPC

**Not yet / YAGNI**:
- Capability token transport (Option C): defer to DICE attestation (§G.2)
- Kernel-native remote TID (Option B kernel path): add only if application ergonomics demand it after the Gateway Cell ships
- Promise pipelining (Cap'n Proto style): defer until latency benchmarks show the extra round-trip is the bottleneck

---

## Limitations of This Research

1. **No latency data for Cellos's TLS stack across QEMU**: the claim "~500µs-5ms RTT" is based on general TCP/TLS measurements. Cellos's smoltcp-based net cell may have different characteristics, especially under QEMU TCG (no hardware acceleration). A benchmark Cell is needed before committing to latency SLAs.

2. **Schema evolution strategy not validated**: the recommendation to use `schema_version: u8` with postcard is untested. An alternative (CBOR, which postcard is inspired by, has schema evolution via optional fields) was not evaluated.

3. **Multi-node service discovery not covered**: how Machine A discovers that Machine B exists and offers service X is out of scope. mDNS, a Kubernetes-style control plane, or a static config file are all options; none was evaluated here.

4. **RPC cancellation**: if a Cell sends a remote IPC and then calls `sys_exit`, the in-flight request on Machine B has no cancellation path. This is left as an open problem.

---

*Research sources: codebase (file:line refs above) + [Erlang Distributed Docs](https://www.erlang.org/doc/system/distributed.html) + [Cap'n Proto RPC](https://capnproto.org/rpc.html) + [Styx Architecture](https://www.inferno-os.org/inferno/papers/styx.pdf) + [Singularity MSR](https://www.microsoft.com/en-us/research/project/singularity/) + [Orleans MSR](https://www.microsoft.com/en-us/research/project/orleans-virtual-actors/) + [Genode IPC](https://genode.org/documentation/genode-foundations/21.05/architecture/Inter-component_communication.html) + [seL4 Manual](https://sel4.systems/Info/Docs/seL4-manual-latest.pdf)*
