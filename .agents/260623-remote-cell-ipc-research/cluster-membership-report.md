# Cluster Membership Model — Research Report
**Topic:** Cross-machine Cell communication with eusocial colony topology
**Date:** 2026-06-23
**Stage:** G2 design (no code yet; zero codebase surface exists for this feature)

---

## 1. Current Codebase Baseline

**Verdict:** No cluster/remote-IPC surface exists today; all IPC is intra-machine SAS. The design
must be additive — it slots into the `net-broker` Cell pattern without touching `libs/api/` traits.

- `libs/api/src/manifest.rs` — manifest is exactly 8 bytes: `magic u32 | version u8 | flags u8 | _pad [u8;2]`. All 8 bits of `flags` are consumed (`MANIFEST_FLAGS_MASK = 0xFF`). No room for cluster data in `flags`. A manifest version bump (v1→v2, Law 1 bump) is required to carry cluster metadata.
- `kernel/src/task/tcb.rs:132–267` — `Task` struct has no cluster field. Adding one is additive (no ABI surface in `libs/api/`).
- `libs/api/src/syscall.rs:577–591` — service IDs 1–7 registered; 6 is free. No `NET_BROKER` service ID defined.
- `libs/api/src/ipc.rs` — `NetRequest` covers TCP/UDP/DNS/multicast; no remote-IPC or cluster envelope type.
- `cells/services/net/src/` — TLS 1.3 via embedded-tls is shipped and functional (commit `af20757d`). `GetRandom = 214` provides entropy. These are the cryptographic primitives needed for ClusterAuth.
- The TODO comment at `manifest.rs:53–56` explicitly calls out that `flags: u8` must expand to `u16` (Law 1 bump) when P5 partition support is needed. Cluster metadata is a second driver for that same bump.
**Source:** `d:\Cellos\libs\api\src\manifest.rs:1–56`, `d:\Cellos\kernel\src\task\tcb.rs:132–267`, `d:\Cellos\libs\api\src\syscall.rs:577–591`

---

## 2. Routing Decision Matrix

**Verdict:** Four membership modes with clean asymmetric rules; Public is the only mode that accepts
unsolicited cross-machine calls.

### 2.1 Mode Definitions

```rust
/// Embedded in __ViCell_cluster ELF section (new, additive — no Law 1).
#[repr(C)]
pub struct CellClusterDecl {
    pub magic:      u32,   // 0x434C5354 "CLST"
    pub version:    u8,    // 1
    pub mode:       u8,    // ClusterMode discriminant
    pub _pad:       [u8; 2],
    pub cluster_id: u64,   // stable hash of cluster name; 0 for Isolated/Public
}

#[repr(u8)]
pub enum ClusterMode {
    Isolated = 0,   // default — no cross-machine IPC
    Public   = 1,   // accepts calls from any machine; announces itself
    Private  = 2,   // cross-machine only within same cluster_id + shared key
}
```

### 2.2 Routing Matrix

| Caller mode     | Callee mode        | cluster_id match? | Decision    | Reason                                      |
|-----------------|--------------------|-------------------|-------------|---------------------------------------------|
| Isolated        | any remote         | N/A               | REJECT      | Isolated cells have no cross-machine IPC    |
| Public          | Public (remote)    | N/A               | ALLOW       | Public↔Public: open mesh, no auth          |
| Public          | Private(X) remote  | N/A               | REJECT      | Private cells gate on cluster membership    |
| Public          | Isolated remote    | N/A               | REJECT      | Isolated never receives remote calls        |
| Private(X)      | Public (remote)    | N/A               | ALLOW       | Public cells accept from anyone             |
| Private(X)      | Private(X) remote  | match             | ALLOW (HMAC)| Same cluster — HMAC challenge required      |
| Private(X)      | Private(Y) remote  | no match          | REJECT      | Cross-cluster blocked at broker             |
| Private(X)      | Isolated remote    | N/A               | REJECT      | Isolated never receives remote calls        |
| any local       | any local          | N/A               | ALLOW       | Intra-machine: existing kernel IPC rules    |

**Decision point:** enforcement is at `net-broker` Cell on both sides (sending and receiving machine).
The kernel is not involved in cross-machine routing — it sees only local IPC to/from the broker Cell.

**Source:** derived from requirements + architecture analysis; no existing code path.

---

## 3. Cluster Identity

**Verdict:** Two-field identity: a public `ClusterId` (u64 hash, safe to transmit) and a secret
`ClusterKey` ([u8; 32] HMAC key, never leaves the machine except via enrollment protocol).

```rust
/// Public cluster identity — safe to include in beacon and wire headers.
/// Computed as: FNV-1a-64(cluster_name.as_bytes()) with domain prefix "cellos.cluster."
/// Using FNV-1a because it is no_std, no_alloc, and collision risk is negligible
/// for fleet sizes < 10^6 (birthday bound at 2^32 clusters).
pub type ClusterId = u64;

/// Secret cluster key — never leaves the machine.
/// Used only to derive per-session HMAC tokens.
pub type ClusterKey = [u8; 32];

/// Compute ClusterId from a human name.
pub fn cluster_id_of(name: &str) -> ClusterId {
    let prefix = b"cellos.cluster.";
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset basis
    for &byte in prefix.iter().chain(name.as_bytes().iter()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3); // FNV prime
    }
    hash
}
```

### 3.1 Manifest Expansion — Version 2

The current manifest is 8 bytes; `flags` is fully saturated at v1. Cluster metadata requires a
version 2 layout. This IS a Law 1 bump: it touches `libs/api/src/manifest.rs`. The TODO at
`manifest.rs:53` already acknowledges this is coming. The bump is **additive** (v1 manifests
remain valid; kernel falls back to `ClusterMode::Isolated` when no `__ViCell_cluster` section
exists), so no existing cell breaks.

**Strategy: separate ELF section** — cluster declaration lives in `__ViCell_cluster`, not in
`__ViCell_manifest`. This avoids the v2 bump entirely for the manifest struct itself. The kernel
reads an additional ELF section at spawn time (same pattern as `__ViCell_syscalls`). Zero Law 1
violation — `libs/api/src/manifest.rs` is not modified; a new `libs/api/src/cluster.rs` file is
added. The `declare_cluster!` macro embeds the `CellClusterDecl` struct.

```rust
// libs/api/src/cluster.rs  (NEW FILE — no Law 1)
#[macro_export]
macro_rules! declare_cluster {
    (public) => {
        #[used]
        #[link_section = "__ViCell_cluster"]
        pub static VICELL_CLUSTER: $crate::cluster::CellClusterDecl =
            $crate::cluster::CellClusterDecl::public();
    };
    (private = $name:expr) => {
        #[used]
        #[link_section = "__ViCell_cluster"]
        pub static VICELL_CLUSTER: $crate::cluster::CellClusterDecl =
            $crate::cluster::CellClusterDecl::private($crate::cluster::cluster_id_of($name));
    };
}
```

The kernel reads `__ViCell_cluster` at spawn (same path as `__ViCell_syscalls`, `kernel/src/loader.rs:167`),
stores `ClusterMode` + `ClusterId` in the `Task` struct (two new fields, additive).

**Source:** `d:\Cellos\libs\api\src\manifest.rs:53–56`, `d:\Cellos\kernel\src\loader.rs:164–170`

---

## 4. Discovery Protocol (SwarmBeacon)

**Verdict:** mDNS for LAN (zero-config, standard) + static seed list for WAN. Two-phase:
announce-then-verify. Private cluster membership is proven by HMAC response without revealing key.

### 4.1 Transport

| Scope | Mechanism | Rationale |
|-------|-----------|-----------|
| LAN (G2 robot fleet) | mDNS (UDP 5353, 224.0.0.251) | Zero-config; standard RFC 6762; smoltcp supports multicast join (already in `NetRequest::MulticastJoin`) |
| WAN / static config | `/etc/cellos/cluster.toml` seed list | Robots behind NAT cannot use mDNS; seed list is simpler and correct for fleet |
| Cloud / dynamic | DNS-SD TXT record `_cellos-cluster._tcp.local` | Future G3 — not needed for G2 robot fleet |

The net cell's `MulticastJoin` / `MulticastLeave` IPC already exists (`libs/api/src/ipc.rs:105–107`).
A `net-broker` Cell joins `224.0.0.251:5353` and handles beacon TX/RX without any kernel change.

### 4.2 Beacon Wire Format

```
SwarmBeacon (UDP, 64 bytes fixed):
  [0..4]   magic:       u32  = 0x43454C4C "CELL"
  [4]      version:     u8   = 1
  [5]      mode:        u8   = ClusterMode (0=Isolated never sends, 1=Public, 2=Private)
  [6..7]   port:        u16  (big-endian) — broker listen port (default 7777)
  [8..16]  cluster_id:  u64  — 0 for Public cells
  [16..48] machine_id:  [u8; 32] — SHA-256 of (boot_id || cluster_id)
  [48..56] timestamp:   u64  — epoch_ns from GetTime (anti-replay window)
  [56..64] beacon_hmac: [u8; 8] — HMAC-SHA256(cluster_key, beacon[0..56])[0..8]
                                   all-zeros for Public cells (no key)
```

**Public beacon:** `mode=1`, `cluster_id=0`, `beacon_hmac=[0;8]`. Any machine that receives it
learns "there is a Public cell at this address." No authentication required.

**Private beacon:** `mode=2`, `cluster_id=X`. A machine that shares `cluster_id=X` and the
corresponding key can verify `beacon_hmac`. A machine with a different key or no key sees
`cluster_id=X` but cannot verify — it knows a private cluster exists but learns nothing useful.
This is a **zero-knowledge signal of existence without key disclosure**.

**Isolated cells:** never send a beacon. The broker does not announce them.

### 4.3 Machine-ID Stability

`machine_id` = SHA-256(hardware entropy from `GetRandom(214)`, persisted to `/etc/cellos/machine-id`
on first boot, never rotated). This is the stable cross-session identity used in HMAC inputs.

---

## 5. Auth Protocol (ClusterAuth)

**Verdict:** HMAC-SHA256 challenge-response appended to the `net-broker` envelope. Public-to-Public
sends nothing; Private-to-Private does a 1.5-RTT HMAC exchange before first IPC payload.

### 5.1 RemoteIpcEnvelope

This is a new type owned entirely by the `net-broker` Cell — not a kernel type, not in `libs/api/`.

```rust
/// Outer envelope framing a cross-machine IPC message.
/// Serialized with postcard, sent over a TLS 1.3 TCP connection to the remote broker.
#[derive(Serialize, Deserialize)]
pub enum RemoteIpcMsg<'a> {
    /// Announce cluster membership + request connection.
    Hello {
        sender_machine_id: [u8; 32],
        sender_cluster_id: u64,        // 0 for Public
        sender_mode:       u8,         // ClusterMode
        nonce:             [u8; 16],   // random, used in HMAC challenge
    },
    /// Prove cluster membership (Private mode).
    /// hmac = HMAC-SHA256(cluster_key, nonce || dest_machine_id || dest_service_id)[0..8]
    Challenge {
        nonce:     [u8; 16],           // echoed from Hello
        hmac_tag:  [u8; 8],
    },
    /// Carry the actual IPC payload.
    Payload {
        dest_service_id: u16,          // api::service::* constant
        data:            &'a [u8],     // postcard-encoded request (existing IPC types)
    },
    /// Reply from remote service.
    Reply {
        data: &'a [u8],
    },
    /// Broker rejection.
    Rejected { reason: u8 },
}
```

### 5.2 HMAC Construction

```
tag = HMAC-SHA256(cluster_key, nonce || dest_machine_id || dest_service_id_le)[0..8]

where:
  nonce            = 16-byte random from GetRandom(214) on the caller side
  dest_machine_id  = 32-byte stable ID of the target machine
  dest_service_id  = 2-byte little-endian service ID
  [0..8]           = first 8 bytes only (64-bit MAC) — sufficient for authentication;
                     reduces beacon overhead; full 32-byte HMAC stored internally for audit
```

Verification is constant-time (`ct_eq` from subtle crate, already used in ed25519-compact).

### 5.3 Handshake Flow

```
Public → Public (no auth):
  Caller sends Payload directly over established TLS channel.
  No HMAC, no nonce exchange.

Private(X) → service on Private(X) machine:
  1. Caller broker sends Hello { nonce, cluster_id=X, mode=Private }
  2. Remote broker echoes nonce in Challenge { nonce, hmac_tag=HMAC(key_X, nonce||...) }
  3. Caller verifies tag. If ok: sends Payload.
  4. Remote broker verifies caller's tag (mutual). If ok: delivers to local service.
  Total: 1.5 RTT overhead per NEW connection (connection is then kept alive / pooled).

Private(X) → Public service on any machine:
  Same as Public→Public once the channel is open.
  The Public service does not challenge.
```

**TLS layer (mandatory):** All broker-to-broker TCP is wrapped in TLS 1.3 (embedded-tls already
shipped). The HMAC auth is an application-layer cluster-membership check ON TOP of TLS, not a
replacement. TLS prevents passive eavesdropping; HMAC proves cluster membership.

---

## 6. Key Distribution — Three Options

**Verdict:** K1 (PSK baked into image) is the right choice for G2 robot fleet. K2 and K3 are
deferred to G2-late and G3 respectively.

### 6.1 K1 — Deployment-Time PSK

```
cluster_key: [u8; 32] provisioned at image build time.
Stored in: /etc/cellos/cluster.key (RamFS, not on disk — loaded from image).
Access: only net-broker Cell can read it (capability-gated path via VFS cap).
Distribution: baked into OS image; same tooling as /POLICY.BIN (fat16_insert.py).
```

**Pros:** Zero infrastructure. Correct for a closed robot fleet where the operator controls all
machines. Consistent with the existing POLICY.BIN deployment pattern.
**Cons:** Key is static until image re-flash. Compromised key = entire cluster compromised.
**Risk:** Medium. Rotation requires re-image. Acceptable for G2 embedded robots; unacceptable for
open internet deployments.

### 6.2 K2 — Enrollment Token (Bootstrap Node Issues Tokens)

```
Bootstrap node: one machine in the fleet runs a KMS Cell.
New machine: generates ephemeral keypair, sends public key + enrollment token to KMS.
KMS: verifies enrollment token (operator-signed), returns ECDH-encrypted cluster_key.
```

**Pros:** Rotation without re-flash. Supports dynamic fleet growth.
**Cons:** Requires KMS Cell (not yet built — see roadmap §G KMS Cell item). Bootstrap node is a
new single point of failure.
**Risk:** High adoption cost for G2. Defer to G2-late when KMS Cell ships.

### 6.3 K3 — Silo-Backed DICE Attestation

```
Each machine: CDI chain from boot → Silo holds AliasKey → signs EAT (RFC 9711).
Fleet verifier: Veraison or custom policy server validates EAT + issues cluster_key
                encrypted to the machine's AliasKey.
```

**Pros:** Cryptographically proven identity. Rotation is automatic. Survives compromised OS.
**Cons:** Requires Silo hardware (ARM64/x86 G2 only, not RISC-V G1). DICE/RIoT is on the G3
roadmap — not yet built.
**Risk:** Very high adoption cost. Correct long-term architecture but unreachable before G3.

### 6.4 Recommendation for G2

**Use K1.** The robot fleet use case is a closed, operator-controlled network. The existing
`fat16_insert.py` + POLICY.BIN pattern already demonstrates deployment-time secrets. K1 is
consistent, zero-infrastructure, and correct for the G2 graduation demo (Radxa ROCK 5B+ fleet).
Design the key-loading interface so K2/K3 can slot in without changing the broker API.

---

## 7. Security Analysis

### 7.1 Public Cell Threat Model

**DDoS risk: HIGH.** A Public cell that accepts from any machine has no rate-limiting at the cluster
layer. Any machine that discovers it via beacon can flood it with IPC.

**Mitigations:**
- Rate-limiting at the `net-broker` Cell (per-source IP, sliding window token bucket). No kernel
  change needed — broker is userspace.
- The existing heartbeat + watchdog mechanism (`Heartbeat = 207`) detects a hung broker Cell and
  supervisor restarts it. DDoS resilience at OS level already exists.
- Public cells should be stateless where possible (idempotent handlers). The SAS + Cell restart
  model means a flooded-then-killed cell restarts cleanly.
- **Not a kernel concern** — the kernel only sees local IPC between the broker Cell and app Cells.

**Information leakage:** A public beacon reveals the machine's IP address, port, and the set of
public service IDs. This is intentional — public cells are discoverable by design. No private
cluster membership is revealed by the public beacon format.

### 7.2 Private Cluster Threat Model

**Compromised node = compromised cluster key (K1).**

This is the central weakness of symmetric PSK. If one machine's flash is read, `cluster.key` is
exposed and the entire cluster's HMAC auth is broken.

**Mitigations for G2 (before K3 is available):**
1. Net-broker Cell holds the key via a VFS cap — the key is NOT in a globally-readable path.
   A Cell without the correct VFS cap cannot read `/etc/cellos/cluster.key`.
2. TLS 1.3 wraps all broker-to-broker communication — passive capture yields ciphertext only.
3. Key rotation requires re-flash (K1) but the broker connection pool must re-authenticate on
   key change; stale connections are dropped.
4. Physical security of the robot hardware is a deployment responsibility, not an OS one.

**Lateral movement:** A compromised machine holding `cluster_key` can impersonate any cluster
member. This is the known PSK weakness; the K2/K3 path addresses it. For G2 robot fleet (physically
secured, operator-controlled) the risk is acceptable.

### 7.3 Public Cell as Topology Probe

**Risk: MEDIUM.** A compromised Public cell on machine A can probe Private cluster topology:
- It can observe WHICH cluster IDs appear in beacons (cluster_id field is public in Private beacons).
- It CANNOT verify beacon_hmac without the cluster key.
- It CANNOT make IPC calls into Private cells (broker rejects without valid HMAC).
- It CAN enumerate that "cluster ID 0xABCD1234 has N machines" from beacon traffic.

**Mitigation:** Private machines should not broadcast to machines they don't recognize. The beacon
protocol should be augmented with an "accept list" in the net-broker config (by machine_id or
subnet CIDR). This prevents arbitrary public-cell machines from receiving private cluster beacons.

---

## 8. Open Questions

**OQ1 — Service ID collision on remote lookup.**
`LookupService(206)` today returns a local TID. Cross-machine, the caller needs to route to a
remote service, not a local TID. What is the cross-machine service address type? Options: a
`(machine_id, service_id)` tuple; a synthetic local TID assigned by the broker Cell that proxies
to the remote. The broker-as-proxy approach avoids any change to `LookupService` semantics, but
adds a proxy hop. The tuple approach requires callers to be cluster-aware. Neither is designed yet.

**OQ2 — net-broker Cell capability gap.**
The broker Cell must hold `NetworkCap` (to use the net stack) AND must be able to read
`cluster.key` (requires a VFS cap for a specific path). It must also be able to look up which
local Cells are Public/Private (needs access to the service registry or a new kernel query). The
exact capability set for the broker Cell is unspecified and likely requires a new kernel query
syscall (`QueryCellClusterMode(tid) → ClusterMode`) — whether this is a new syscall or an IPC
to a cluster registry service is unresolved.

**OQ3 — mDNS in smoltcp.**
smoltcp 0.11 does not ship a full mDNS responder/querier. `MulticastJoin` adds the Cell to an
IGMP group, but mDNS packet construction/parsing must be implemented in the net-broker Cell or
as a new net-cell extension. The effort is ~200–400 LOC for minimal probe/announce; full RFC 6762
compliance is significantly larger. The alternative (UDP broadcast on port 7777 to 255.255.255.255)
is simpler but limited to the local broadcast domain and blocked by most managed switches.

**OQ4 — Clock synchronization for anti-replay.**
The beacon `timestamp` field (epoch_ns from `GetTime`) is used for an anti-replay window. RTC-backed
time is available (`GetTime = 120`, op=2/3 shipped), but two machines on the same LAN may have
clocks out of sync by seconds or minutes if NTP is not running. The replay window must be wide
enough to tolerate clock skew, but a wide window weakens anti-replay. The design must specify
the window size and the behavior when a machine has no RTC (epoch=0 after boot). NTP is not
currently a Cellos service.

**OQ5 — Hot-swap and cluster re-authentication.**
When a net-broker Cell is hot-swapped (the system's zero-downtime upgrade path), the replacement
Cell needs to re-establish all cluster sessions. The existing `StateStash / StateRestore (410/411)`
mechanism can preserve the session state, but the TLS sessions with remote machines must be
re-negotiated (TLS session resumption is not implemented in embedded-tls). The design must specify
whether in-flight cross-machine IPCs are dropped, buffered, or transparently resumed across a
broker hot-swap.

---

## 9. Codebase Anchors (File:Line)

| Claim | Location |
|-------|----------|
| Manifest is 8 bytes, all 8 flag bits consumed | `libs/api/src/manifest.rs:66–69` |
| `__ViCell_manifest` section read at loader.rs spawn | `kernel/src/loader.rs:95–96` |
| `__ViCell_syscalls` section read pattern (model for `__ViCell_cluster`) | `kernel/src/loader.rs:167–170` |
| Service IDs 1–7 (6 is free for NET_BROKER) | `libs/api/src/syscall.rs:577–591` |
| `MulticastJoin` / `MulticastLeave` already in NetRequest | `libs/api/src/ipc.rs:105–107` |
| TLS 1.3 shipped; `GetRandom(214)` for entropy | `docs/specs/07-networking.md:56–86` |
| Task struct has no cluster field (additive extension point) | `kernel/src/task/tcb.rs:132–267` |
| Manifest TODO: flags must expand for new caps | `libs/api/src/manifest.rs:53–56` |
| HMAC key delivery pattern (POLICY.BIN precedent) | `kernel/src/loader.rs:93–104` (policy loading) |
| ed25519-compact (ct_eq) already in kernel | `docs/project-roadmap.md:345` |

---

## 10. Limitations of This Research

1. **mDNS implementation cost not verified.** smoltcp 0.11 multicast support was confirmed via the
   existing `MulticastJoin` IPC, but whether a full mDNS responder fits within the net-broker Cell's
   complexity budget was not measured. A spike is needed.

2. **HMAC-SHA256 no_std crate not identified.** The design assumes HMAC-SHA256 is available in
   no_std. `hmac` (RustCrypto) + `sha2` (RustCrypto) both support no_std and are already in use
   indirectly (via embedded-tls), but direct use in the broker Cell was not verified against
   Cargo.toml.

3. **net-broker Cell architecture is assumed, not specified.** The design assumes "net-broker Cell"
   as the cross-machine IPC proxy (referenced in project memory as "Option A from prior research")
   but the prior research document was not located in this session. The broker Cell's internal
   architecture, connection management, and failure modes are out of scope here.

4. **G1 embedded applicability.** The design is framed for G2 (TCP/IP capable machines). On G1
   embedded QEMU or real RV64/ARM64 SBCs, mDNS and TCP broker cells require network stack presence.
   Cellos-Nano (RV32, no net) cannot participate in any cluster. This is a known scope boundary.
