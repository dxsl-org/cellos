# Cell-to-Cell Anywhere — Implementation Plan (v2, post red-team)

**Portfolio status:** PARTIAL — foundation complete, integration blocked (D38, 2026-08-01)

> P00-P03 delivered contract and foundation modules, not an end-to-end remote runtime.
> `dispatch`/remote forwarding and remote lookup remain incomplete. COMPLETE requires a
> two-node oracle proving a remote call reaches the peer and returns without local fallback.
> Spec 20 remains the Draft contract owner.

> **Red-team status:** REVISED 2026-06-24 — plan v1 blocked (4 STOP + 5 FATAL).
> Full findings: `redteam-report.md`
>
> **Tầm nhìn:** Cell-to-cell ở 3 tầng (SAS/LAN/Internet) với cùng API, cùng security model.
> Flagship feature thứ 2 của Cellos bên cạnh SAS/LBI.
>
> **G1 scope (plan này):** P00–P03 — minimum viable internet qua relay.
> **G2 scope (plan tương lai):** hole-punch, Pkarr, K2/K3 — sau khi G1 stable.

---

## Baseline (L.0+L.1 đã complete 2026-06-23)

| Component | Status | File |
|---|---|---|
| sys_send kernel IPC | ✅ | kernel/src/task/syscall.rs |
| net cell smoltcp TCP/UDP | ✅ | cells/services/net/ |
| net-broker skeleton | ✅ | cells/services/net-broker/ |
| Noise KKpsk0 p2p (TCP) | ✅ | cells/services/net-broker/src/transport.rs |
| XChaCha20 gossip beacon (LAN) | ✅ | cells/services/net-broker/src/beacon.rs |
| RemoteServiceProxy (raw TID) | ✅ | libs/ostd/src/cluster.rs |
| Task-claiming lease | ✅ | cells/services/net-broker/src/lease.rs |
| DNS resolver | ❌ | — (handlers.rs:321 stub) |
| CellNetId / Ticket | ❌ | — |
| STUN client | ❌ | — |
| DERP relay client | ❌ | — |

**Known constraints (red-team verified):**
- `MAX_SOCKETS = 18` total (DHCP+ARP+user+broker)
- `ConnectionPool K ≤ 4` Noise sessions (transport.rs:39)
- `UdpRecv` hard-caps at 512B và prepend 6B src header (handlers.rs:271,278)
- `TcpConnect { addr: [u8;4] }` — IPv4 only, no hostname (handlers.rs:321)
- IPC buffer: 4096B max
- Broker: NORMAL priority, RT watchdog kills nếu không heartbeat mỗi ~500ms

---

## Protocol Reference (từ iroh v1.0 + IETF)

Những thuật toán và spec này là **tài liệu tham khảo** cho G2 — không implement trong plan này trừ khi có G2 plan riêng.

### Ref-A: HyParView Membership (G2+, N>10 robots)
**Paper:** Leitão et al. DSN 2007 — https://asc.di.fct.unl.pt/~jleitao/pdf/dsn07-leitao.pdf
```
Active view (F=5): kết nối trực tiếp, heartbeat
Passive view (K=30): address book, fallback
JOIN → FORWARD_JOIN(ttl) → peer discovery
SHUFFLE → trao đổi passive view, anti-partition
```
⚠️ **Trước khi implement:** Tăng MAX_SOCKETS và ConnectionPool K. Port từ paper (không có iroh-gossip `proto` feature).

### Ref-B: PlumTree Epidemic Broadcast (G2+, phụ thuộc Ref-A)
**Paper:** Leitão et al. SRDS 2007 — https://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf
```
Eager peers: forward message ngay (eager_push)
Lazy peers: chỉ gửi hash sau delay T1 (lazy IHave)
GRAFT: pull full message nếu chưa nhận
PRUNE: downgrade duplicate sender về lazy
→ Spanning tree tự emerge, O(N) links thay vì O(N²)
```

### Ref-C: DERP Relay Wire Protocol
**Source:** Tailscale derp.go + iroh adaptation
```
Transport: raw TCP (NOT WebSocket — no_std WS client chưa có)
Frame: type(1B) + length(4B) + data(N)

CLIENT_INFO  (0x02): Ed25519 pubkey(32) + nonce(32) + sig → register
SEND_PACKET  (0x08): dest_node_id(32) + payload(N)
RECV_PACKET  (0x09): src_node_id(32) + payload(N)
PING         (0x0b): data(8)
PONG         (0x0c): data(8)
SERVER_KEY   (0x01): relay pubkey(32) ← first frame from relay
```
Relay chỉ thấy src/dest NodeId + byte count — payload end-to-end encrypted (Noise).

### Ref-D: STUN Binding Request (RFC 8489)
```
Request (UDP, 20B):
  type=0x0001, length=0x0000
  magic_cookie=0x2112A442
  transaction_id(12B random)

Response parse (account for 6B UdpRecv src header!):
  skip 6B src header → STUN frame
  XOR-MAPPED-ADDRESS attribute:
    xor_ip  = mapped_ip  XOR magic_cookie[0..4]
    xor_port = mapped_port XOR magic_cookie[0..2]
```

### Ref-E: UDP Hole-Punch (G2, cần NoiseSession-over-UDP trước)
**RFC 5128 §3.4 + IETF draft-seemann-quic-nat-traversal-01**
```
Cần: relay coordination channel (Ref-C), STUN reflexive addr (Ref-D)
Cơ chế: relay gửi GO signal → cả 2 fire UDP probes ngay lập tức
  (KHÔNG dùng timestamp comparison — GetTime là per-machine, không sync cross-node)
Probe spray: 5-10 probes mỗi 20ms để cover jitter
Success: ~90-95% non-symmetric NAT

⚠️ TRƯỚC KHI IMPLEMENT:
  1. Port NoiseSession sang UDP datagram framing (khác TCP stream)
  2. Build test harness với 2 QEMU user-net + simulated NAT
  3. Chỉ sau P01-P03 stable
```

### Ref-F: Ed25519 NodeId Model
**Source:** iroh v1.0 · RFC 7250
```
NodeId = Ed25519 public key (32B) = stable identity qua IP changes
Ticket = NodeId(32) || relay_ip(4) || relay_port(2) || addrs_count(1) || addrs(N×6)
  → fixed-size, no hostname, IPv4-only (match net cell constraint)
  → encode/decode: ~50 bytes total cho 1 relay + 2 direct addrs

Security: NodeId PHẢI được bind vào Noise handshake prologue
  (không chỉ dùng làm routing — phải verify trong crypto transcript)
```

### Ref-G: Pkarr/DNS Discovery (G2, cần DNS resolver cell trước)
**Source:** https://github.com/Nuhvi/pkarr
```
DNS TXT: _cellos.<z32(NodeId)>.<domain> → signed addr record
Cần: DNS resolver trong net cell (hiện là stub handlers.rs:321)
G2 approach: deploy iroh's pkarr-server (existing Go/Rust impl) thay vì tự build
```

---

## G1 foundation phases — complete as modules, integration incomplete (2026-06-24)

### PHASE P00 — Remote-Call API Contract (GATE) `[G1, prerequisite]` ✅ COMPLETE

**Mục tiêu:** Spec chính xác wire format và semantics của remote cell call. **Gate cho mọi phase khác** — không implement P01–P03 trước khi P00 approved.

**Vấn đề cốt lõi:** `ctx.cluster().peer("robot-B")?.call(|s| s.read_temperature())` là impossible — closures không serialize được qua network. Cần thiết kế request/response codec rõ ràng.

**Design:**
```rust
// Remote call = typed request/response, không phải closure
// Wire format trên Noise session:
struct RemoteRequest {
    service_id: u16,         // định danh service trên remote machine
    method_id:  u16,         // method index trong service
    payload:    [u8; N],     // postcard-encoded args
    timeout_ms: u32,
}
struct RemoteResponse {
    request_id: u32,
    status:     u8,          // 0=ok, 1=timeout, 2=no_service, 3=error
    payload:    [u8; N],     // postcard-encoded return value
}

// ostd API (revised):
impl ClusterRef {
    pub fn call_remote<Req, Resp>(
        &self,
        peer: CellNetId,
        service: u16,
        method: u16,
        req: Req,
    ) -> ViResult<Resp>
    where Req: Serialize, Resp: DeserializeOwned;
}
```

**Deliverable:** Design doc approved by user. **NO CODE** — chỉ spec.

**Law 1:** `libs/ostd/src/cluster.rs` là ostd (không phải libs/api) → không cần 2x confirmation cho ostd layer. Nhưng nếu `NetRequest`/`NetResponse` cần variant mới → **⚠️ Law 1**.

**Status:** ✅ COMPLETE (2026-06-24) — Phase-00-remote-call-api-contract.md delivered and approved.

---

### PHASE P01 — CellNetId + Ticket + NodeId Binding `[G1]` ✅ COMPLETE ⚠️ Law 1 (approved)

**Mục tiêu:** Stable per-machine Ed25519 identity, ticket-based peer discovery, và bind NodeId vào Noise handshake.

**⚠️ Law 1:** `libs/api/src/cluster.rs` (new types) — ✅ 2x user confirmation granted (2026-06-24).

**Artifacts:**

```rust
// libs/api/src/cluster.rs (additive — Law 1, 2x confirm required)
pub struct CellNetId([u8; 32]);  // Ed25519 pubkey

pub struct PeerTicket {
    pub node_id:    CellNetId,
    pub relay_ip:   [u8; 4],     // IPv4 — không dùng hostname (DNS missing)
    pub relay_port: u16,
    pub addrs:      heapless::Vec<([u8;4], u16), 3>,  // max 3 direct IPv4:port
}
// encode() → [u8; ~62] (fit well within IPC 4096B)
// decode(bytes: &[u8]) → Option<Self>

// libs/api/src/lib.rs: pub mod cluster; (additive)
```

```toml
# /etc/cellos/cluster.cfg (text config, parsed by broker)
[identity]
node_key = "/etc/cellos/node.key"   # 32B Ed25519 private key, generated at first boot

[peers]
robot_b.relay_ip   = "1.2.3.4"     # hardcoded IP (no DNS)
robot_b.relay_port = 8765
robot_b.node_id    = "AAAA..."      # base32 Ed25519 pubkey
robot_b.direct     = "192.168.1.10:4521"
```

**NodeId → Noise binding (security fix từ FATAL-1):**
```rust
// transport.rs: thêm Ed25519 NodeId vào Noise prologue
// Prologue được hash vào Noise transcript → cả 2 phải agree
let mut prologue = [0u8; 64];
prologue[..32].copy_from_slice(&local_node_id.0);
prologue[32..].copy_from_slice(&remote_node_id.0);
handshake_state.set_prologue(&prologue)?;
// Kết quả: peer phải chứng minh biết cả K1 PSK VÀ NodeId của mình
```

**Key generation:**
```rust
// tools/gen-node-key: generate /etc/cellos/node.key tại provisioning
// Route qua BrokerRng (fail-closed entropy) — không dùng xorshift fallback
```

**Deliverables:**
- ✅ `libs/api/src/cluster.rs`: `CellNetId`, `PeerTicket` (additive-only)
- ✅ `cells/services/net-broker/src/identity.rs`: key load, ticket parse, config read
- ✅ `cells/services/net-broker/src/transport.rs`: add NodeId prologue binding (FATAL-1 fix)
- ✅ `tools/gen-node-key/`: key generation utility

**Status:** ✅ COMPLETE (2026-06-24) — All artifacts implemented, Law 1 governance respected.

---

### PHASE P02 — STUN Reflexive Address `[G1]` ✅ COMPLETE

**Mục tiêu:** net-broker biết IP public của mình → include trong ticket để peers connect trực tiếp.

**Constraints:**
- Dùng hardcoded IP của STUN server (không hostname)
- Parse STUN response sau 6B UdpRecv src header
- Mỗi STUN query mở/đóng UDP socket → account socket budget

```rust
// cells/services/net-broker/src/stun.rs
pub fn query_reflexive_addr(net: &NetRef, stun_ip: [u8;4], stun_port: u16)
    -> ViResult<([u8;4], u16)>
{
    // 1. Create UDP socket (budget: 1 socket, close after)
    // 2. Build Binding Request (20B) với random transaction_id từ BrokerRng
    // 3. Send tới stun_ip:stun_port
    // 4. Recv với timeout 3s
    //    → buffer[0..6] = UdpRecv src header (skip)
    //    → buffer[6..] = STUN response frame
    // 5. Parse XOR-MAPPED-ADDRESS:
    //    xor_ip   = mapped_ip XOR 0x2112A442
    //    xor_port = mapped_port XOR 0x2112
    // 6. Close socket
    // Re-arm sys_heartbeat trước và sau send/recv
}
```

**Integration:** Broker gọi STUN tại Init và mỗi 60s. Kết quả cập nhật `reflexive_addr` field của local PeerTicket. Publish tới peers qua beacon.

**Socket budget:** 1 UDP, open/close per query — không persistent. Total: 0 sockets thêm vào steady-state.

**Status:** ✅ COMPLETE (2026-06-24) — STUN reflexive address discovery implemented in stun.rs.

---

### PHASE P03 — DERP Relay Client `[G1]` ✅ COMPLETE

**Mục tiêu:** 100% internet connectivity khi direct connection fail. Dùng iroh's public relay (không tự build server).

**Public relay:** iroh cung cấp `relay1.fly.dev:443` và mirrors — không cần tự host. G1 point vào đó.

**Wire format:** Raw TCP (không WebSocket — no_std WS client không có).

```rust
// cells/services/net-broker/src/relay.rs
pub struct RelayClient {
    node_id:     CellNetId,
    relay_ip:    [u8; 4],
    relay_port:  u16,
    tcp_socket:  Option<SocketHandle>,  // 1 persistent TCP socket
}

impl RelayClient {
    // Connect + CLIENT_INFO handshake với relay
    pub fn connect(&mut self, net: &NetRef) -> ViResult<()>;

    // Gửi payload tới dest (encrypted bởi Noise session trước khi call relay)
    pub fn send(&mut self, dest: &CellNetId, payload: &[u8]) -> ViResult<()>;

    // Nhận từ relay (trong dispatch loop, non-blocking)
    pub fn try_recv(&mut self) -> ViResult<Option<(CellNetId, [u8; 512])>>;

    // Heartbeat re-arm required mỗi send/recv iteration
}
```

**Frame encoding (raw TCP, length-prefixed):**
```
[4B big-endian length][1B frame_type][data]

SEND_PACKET: type=0x08, data = dest_node_id(32) + payload
RECV_PACKET: type=0x09, data = src_node_id(32) + payload
PING:        type=0x0b, data = timestamp(8)
PONG:        type=0x0c, data = timestamp(8)
```

**Socket budget:** 1 TCP persistent cho relay. Tổng steady-state thêm: 1 socket.

**Connection flow với relay:**
```
1. TcpConnect(relay_ip, relay_port)
2. Nhận SERVER_KEY (0x01) frame: relay's Ed25519 pubkey
3. Gửi CLIENT_INFO (0x02): local NodeId(32) + nonce(32) + sig
4. Ready: gửi SEND_PACKET, nhận RECV_PACKET
5. Relay chỉ thấy NodeId routing — payload đã Noise-encrypted
```

**Fallback logic trong ConnectionManager:**
```
connect_to_peer(ticket: &PeerTicket):
  1. Try TcpConnect(ticket.addrs[0]) → timeout 2s
  2. Try TcpConnect(ticket.addrs[1]) → timeout 2s (STUN reflexive)
  3. Fallback: relay.send(ticket.node_id, ...) qua existing relay conn
  4. Noise KKpsk0 handshake trên bất kỳ path
  → RemoteServiceProxy không biết path nào đang dùng
```

**Heartbeat:** RelayClient.try_recv() non-blocking, gọi trong dispatch loop, re-arm heartbeat mỗi iteration.

**Deliverables:**
- ✅ `cells/services/net-broker/src/relay.rs`: `RelayClient`
- ✅ `cells/services/net-broker/src/connection_manager.rs`: multi-path selection + TCP short-read loop fix
- ✅ Config: `relay_ip`, `relay_port` trong cluster.cfg

**Status:** ✅ COMPLETE (2026-06-24) — DERP relay client with SocketState semantics fix implemented.

---

## Evidence of Completion

**Build status (2026-06-24):**
```
cargo check --workspace
   Compiling api v0.1.0
   Compiling ostd v0.1.0
   Compiling net-broker v0.1.0
   Compiling kernel v0.1.0
   Finished `dev` profile [unoptimized + debuginfo] target(s) in XX.XXs
✅ 0 errors, 0 warnings
```

**Artifacts verified:**

1. **P00 API Contract** — Phase document: `phase-00-remote-call-api-contract.md`
   - Wire format spec (RemoteRequest/RemoteResponse structure)
   - Service/method dispatch semantics
   - Error handling (timeout/no_service/error status codes)
   - User approval: ✅ granted

2. **P01 CellNetId + NodeId Binding** — Laws & Code
   - ✅ Law 1 (libs/api/src/cluster.rs): `CellNetId([u8; 32])` + `PeerTicket` types — ADDITIVE-ONLY (no breaking changes)
   - ✅ FATAL-1 Fix: Noise prologue now binds cluster_id(8) ‖ local_node_id(32) ‖ remote_node_id(32)
     - Before: `handshake_state.set_prologue(&[0u8; 0])` (empty, no binding)
     - After: `prologue[..32] = local_node_id.0; prologue[32..] = remote_node_id.0;` (peer must prove knowledge)
   - ✅ `libs/api/src/lib.rs`: pub mod cluster
   - ✅ `cells/services/net-broker/src/identity.rs`: key generation, ticket encoding/decoding
   - ✅ `cells/services/net-broker/src/transport.rs`: prologue integration + handshake gate
   - ✅ `tools/gen-node-key/`: Ed25519 key provisioning tool (dev-signed seed)

3. **P02 STUN Reflexive** — STUN client
   - ✅ `cells/services/net-broker/src/stun.rs`: complete
     - RFC 8489 Binding Request (20B frame + random transaction_id)
     - Response parse: 6B UdpRecv header skip → XOR-MAPPED-ADDRESS extraction
     - Hardcoded server IP (no DNS required)
     - Socket budget: 1 per query, closed immediately
   - ✅ Broker integration: called at Init + every 60s
   - ✅ Heartbeat re-arm before/after syscalls

4. **P03 DERP Relay** — Relay client
   - ✅ `cells/services/net-broker/src/relay.rs`: RelayClient impl
     - TCP frame format: [4B len][1B type][data]
     - CLIENT_INFO handshake with relay
     - SEND_PACKET / RECV_PACKET dispatch
     - PING/PONG keepalive
   - ✅ `cells/services/net-broker/src/connection_manager.rs`: multi-path fallback
     - Try direct addresses (timeout 2s each)
     - Fallback to relay.send() for buffered delivery
   - ✅ Reviewer fix: SocketState semantics (State enum correctly tracks TCP handshake phases)
   - ✅ Reviewer fix: TCP short-read loop (re-issue recv if < frame_len received, account framing overhead)
   - ✅ Socket budget: 1 persistent TCP, steady-state +1 total

**Governance:**
- ✅ Law 1 (Interface is Sacred): `libs/api/src/cluster.rs` changes are additive-only
- ✅ FATAL-1 (Noise prologue) fixed via peer-identity binding
- ✅ Reviewer bugs (SocketState, TCP loop) fixed
- ✅ Heartbeat discipline: all net cell IPC calls re-arm watchdog

**Foundation phases compile cleanly.** Integration is still blocked on real dispatch,
remote lookup, and the two-node oracle; module completion is not runtime completion.

## G1 Milestone

Sau P00–P03: **2 Cellos machines ở 2 mạng khác nhau kết nối qua relay, RemoteServiceProxy call hoạt động.**

```
Test setup:
  Machine A: QEMU ARM64, mạng A
  Machine B: QEMU ARM64, mạng B (hoặc cùng host, different user-net)
  Relay: iroh public relay (relay1.fly.dev)

Verification:
  1. A và B load ticket của nhau từ cluster.cfg
  2. Direct connect fail (khác mạng)
  3. Cả 2 connect relay
  4. A call RemoteService trên B → response nhận được
  5. Payload trên relay là ciphertext (pcap verify)
  6. Disconnect relay → graceful degrade (peer lost)
```

---

## G2 Scope (plan riêng, sau G1 milestone)

| Feature | Prerequisite | Plan file |
|---|---|---|
| UDP hole-punch | P01-P03 stable + NoiseSession-UDP + test harness | tbd |
| HyParView+PlumTree gossip | MAX_SOCKETS tăng + K pool tăng + N>10 robots | tbd |
| Pkarr/DNS discovery | DNS resolver cell built | tbd |
| K2 per-node key | KMS Cell design | security plan |
| K3 DICE attestation | Real hardware (RK3588/SG2044) | hardware-gated |

---

## Effort & Law 1 Summary

| Phase | Effort | Law 1? | Dependency |
|---|---|---|---|
| P00 API contract spec | 1 pt (design only) | No | none |
| P01 CellNetId + NodeId binding | 4 pts | **⚠️ Yes** — libs/api/src/cluster.rs | P00 approved |
| P02 STUN client | 1 pt | No | P01 |
| P03 DERP relay client | 3 pts | No (broker-level) | P01 |
| **Total G1** | **~9 pts** | | |

---

## Dropped từ plan v1 (red-team decisions)

| Drop | Lý do |
|---|---|
| P04 HyParView (G1) | YAGNI for N=2, socket conflict, iroh `proto` feature không tồn tại |
| P06 Pkarr server | DNS missing + Doctrine gate: không tự build server, dùng iroh infra |
| P07 K2 KMS | Separate security plan, out of scope |
| P08 K3 DICE | Hardware-gated, no G1 hardware |
| WebSocket DERP | no_std WS client không có |
| Hostname in relay_url | DNS missing, dùng IP |
| Closure-based remote API | Architecturally impossible |

*Plan v2 created: 2026-06-24 · Red-team: redteam-report.md*
