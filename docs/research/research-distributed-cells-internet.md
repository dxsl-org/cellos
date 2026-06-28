# Research: Distributed Cells — Internet Connectivity

> **Purpose:** Protocol specs và thuật toán tham khảo từ iroh v1.0 + phân tích gaps còn lại để cell-to-cell hoạt động ở 3 tầng: local SAS (same machine), LAN, và internet (cross-NAT).
>
> **Status:** Research reference — chưa có implementation plan. Xem L.0+L.1 plan tại `.agents/260623-0907-net-broker-robot-swarm/` cho nền tảng đã build.

---

## Tầng hiện tại đã hoạt động

```
Tier 0: Local SAS (same machine)
  Cell A ──[sys_send]──► Cell B
  Zero-copy, kernel IPC, ~2µs latency
  ✅ Always worked — kernel foundation

Tier 1: LAN (same network, no NAT crossing)
  Machine A                        Machine B
  net-broker ──UDP multicast──►    net-broker  (SwarmBeacon discovery)
  net-broker ──TCP + Noise KKpsk0──► net-broker  (p2p auth + encrypted)
  ~500µs–5ms latency
  ✅ Built in L.0+L.1 (2026-06-23)

Tier 2: Internet (cross-NAT)
  Machine A (behind NAT A)         Machine B (behind NAT B)
  net-broker ──??? ──────────────► net-broker
  ❌ NOT built — requires 3 new mechanisms
```

**Vấn đề gốc:** UDP multicast không vượt router/NAT. TCP `connect(peer_ip)` cũng không work khi peer ở sau NAT và bạn không biết IP public của họ, hoặc NAT chặn inbound connections.

---

## Gap 1: Discovery qua Internet

**Vấn đề:** Làm sao tìm được địa chỉ của một Cell trên internet khi không có shared multicast network?

### Cơ chế L.0+L.1 (chỉ hoạt động trên LAN)
```
UDP multicast 224.0.0.1:XXXX
  → beacon với XChaCha20-Poly1305, machine_id, cluster_id
  → chỉ hoạt động trong 1 broadcast domain (L2 segment)
  → KHÔNG cross router, KHÔNG cross NAT
```

### Iroh's approach: 4 lớp discovery có thể plug-in

**1. Tickets (đơn giản nhất, đủ cho G1 robot swarm)**
```
Ticket = Ed25519 NodeId (32B) + relay_url + direct_socket_addrs
Encoding: base32 hoặc QR code
Use case: provisions lúc setup, share out-of-band
```
Tickets là cách Cellos G1 nên dùng: robots nhận ticket khi được provisioned, không cần DNS hay DHT.

**2. mDNS (LAN, có thể thay multicast)**
```
_iroh._udp.local. PTR <NodeId>.local.
<NodeId>.local. TXT relay=... addr=...
```
Thay thế custom multicast beacon bằng mDNS chuẩn. Benefit: tương thích với existing mDNS infrastructure. Cellos hiện không có mDNS stack.

**3. Pkarr / DNS-based (Internet scale)**
```
DNS TXT record: _iroh.<z32(NodeId)>.<origin-domain>
  Value: "relay=https://relay.example.com addr=203.0.113.5:4521"
  Signed bởi Ed25519 key của NodeId (Pkarr = public-key addressable resource records)
  Bất kỳ DNS resolver nào cũng đọc được
```
Mỗi node publish địa chỉ của mình lên DNS. Lookup = resolve DNS → ra relay URL + direct addrs. Cần:
- HTTP endpoint để publish Pkarr records (iroh-dns-server hoặc public Pkarr server)
- DNS resolution trong net cell

**4. BitTorrent DHT (decentralized, không cần server)**
```
Pkarr packet (signed) → publish vào DHT mainline
Lookup: DHT.get(NodeId) → signed addr record
```
Không cần server nào. Dùng DHT của BitTorrent (65M nodes). G2+.

### Recommendation cho Cellos

| Giai đoạn | Discovery mechanism | Lý do |
|---|---|---|
| G1 robot swarm | Tickets (out-of-band) | Robots đều được provisioned trước |
| G1 fallback | Static peer list trong `/etc/cellos/peers.cfg` | Simple, no infra |
| G2 | Pkarr + public relay | Internet-scale, chuẩn |
| G3+ | BitTorrent DHT | Decentralized, no server |

---

## Gap 2: NAT Traversal

**Vấn đề:** Hầu hết robot/server không có IP public. TCP connect trực tiếp bị NAT chặn.

### Tại sao L.0+L.1 không gặp vấn đề này
L.0+L.1 test trên QEMU ARM64 với shared virtual network — cả 2 VM đều có IP reach-able lẫn nhau. Không có NAT thật sự. Trên internet thật → cần NAT traversal.

### Hole-punch: UDP simultaneous open

Cơ chế chuẩn (RFC 5128, đã proven):

```
Step 1: Cả 2 peers kết nối relay server
  Peer A → relay: "Tôi muốn kết nối với B"
  Peer B → relay: "Tôi muốn kết nối với A"
  Relay thấy: A có public addr 203.0.113.5:4521, B có 8.8.9.9:7777

Step 2: Relay chia sẻ addr
  relay → A: "B ở 8.8.9.9:7777"
  relay → B: "A ở 203.0.113.5:4521"

Step 3: Simultaneous UDP send (cùng lúc)
  A → 8.8.9.9:7777   (tạo NAT entry: A đang chờ từ B)
  B → 203.0.113.5:4521 (tạo NAT entry: B đang chờ từ A)

Step 4: Packets vượt NAT
  Packet từ B đến 203.0.113.5:4521 → NAT của A thấy "đây là reply"
  → cho qua → kết nối trực tiếp thành công!
```

**Success rate thực tế (từ iroh production data):**
- Full-cone NAT: ~100%
- Port-restricted cone: ~95%
- Address-restricted cone: ~90%
- Symmetric NAT (doanh nghiệp, CGNAT): ~0% — phải dùng relay

### Iroh's QUIC NAT Traversal spec (IETF draft-seemann-quic-nat-traversal-01)

Iroh nâng cấp UDP hole-punch lên QUIC level bằng 3 frame types mới:

```
ADD_ADDRESS   (0x3d7e90): "Tôi có thể reach được tại addr này"
PUNCH_ME_NOW  (0x3d7e92): "Hãy punch đồng thời, bắt đầu bây giờ"
REMOVE_ADDRESS (0x3d7e94): "Addr cũ không còn valid"
```

Khi hole-punch thành công, QUIC connection migrate từ relay path sang direct path **không có gián đoạn** (QUIC connection migration, RFC 9000 §9).

**Cho Cellos:** Không cần QUIC để làm hole-punch. Có thể làm với TCP/UDP thuần dùng relay để coordinate, nhưng QUIC cung cấp connection migration graceful hơn.

### Symmetric NAT — problem không giải được bằng hole-punch

Một số loại NAT (corporate firewall, CGNAT của ISP) dùng **symmetric NAT**: mỗi destination IP:port tạo một NAT mapping khác nhau. Hole-punch thất bại vì không đoán được port mà NAT sẽ dùng.

Ước tính ~5-10% internet connections gặp symmetric NAT → cần relay fallback.

---

## Gap 3: Relay Fallback (DERP)

**Vấn đề:** Khi hole-punch thất bại (symmetric NAT, firewall), cần server trung chuyển traffic.

### Iroh's DERP protocol (từ Tailscale)

DERP = Designated Encrypted Relay for Packets. Key design decisions:

```
1. Relay chỉ thấy: (sender_NodeId, receiver_NodeId, ciphertext_bytes)
   → Relay KHÔNG đọc được nội dung (end-to-end encrypted)
   → Relay KHÔNG cần trust

2. Relay là stateless về crypto
   → Bất kỳ relay server nào cũng forwarding được
   → Failover tự động nếu 1 relay down

3. Connection migrate relay→direct khi hole-punch thành công
   → Relay chỉ là fallback, không là dependency

4. Relay protocol đơn giản:
   Client → Relay: SEND_PACKET frame(dest_NodeId, payload)
   Relay → Client: RECV_PACKET frame(src_NodeId, payload)
```

**Wire protocol (từ Tailscale DERP spec, iroh dùng cùng format):**
```
Frame structure: type(1B) + length(4B) + data(N)

Frame types:
  0x01 SERVER_KEY    relay pub key (X25519)
  0x02 CLIENT_INFO   client Ed25519 pubkey + nonce
  0x03 SERVER_INFO   encrypted channel info
  0x08 SEND_PACKET   dest[32] + payload
  0x09 RECV_PACKET   src[32] + payload
  0x0b PING          latency probe
  0x0c PONG          latency reply
```

Transport: WebSocket hoặc plain TCP. Iroh dùng HTTPS/WebSocket vì relay servers cần vượt corporate firewall (port 443/80 thường mở).

**Cho Cellos:** Cần 1 relay cell (hoặc external relay server). Protocol đơn giản — có thể implement trong net-broker hoặc tách thành relay-cell riêng.

---

## Protocol Specs để tham khảo (Reference Index)

### NAT Traversal
| Spec | URL | Dùng cho |
|---|---|---|
| IETF draft-seemann-quic-nat-traversal-01 | https://www.ietf.org/archive/id/draft-seemann-quic-nat-traversal-01.html | Hole-punch qua QUIC frames |
| RFC 5128 — NAT traversal techniques | https://datatracker.ietf.org/doc/html/rfc5128 | UDP hole-punch fundamentals |
| RFC 8445 — ICE (Interactive Connectivity Establishment) | https://datatracker.ietf.org/doc/html/rfc8445 | Full ICE protocol (WebRTC dùng) |

### Gossip / Membership
| Spec | URL | Dùng cho |
|---|---|---|
| HyParView paper (Leitão et al. 2007) | https://asc.di.fct.unl.pt/~jleitao/pdf/dsn07-leitao.pdf | Partial-view membership protocol |
| PlumTree paper (Leitão et al. 2007) | https://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf | Epidemic broadcast tree |
| iroh-gossip::proto source | https://github.com/n0-computer/iroh-gossip | Pure IO-less state machine, no_std extractable |

### Relay
| Spec | URL | Dùng cho |
|---|---|---|
| Tailscale DERP protocol | https://github.com/tailscale/tailscale/blob/main/derp/derp.go | Wire format reference |
| STUN RFC 8489 | https://datatracker.ietf.org/doc/html/rfc8489 | IP:port reflection (biết IP public của mình) |

### Transport / QUIC
| Spec | URL | Dùng cho |
|---|---|---|
| QUIC RFC 9000 | https://datatracker.ietf.org/doc/html/rfc9000 | Core QUIC, connection migration §9 |
| draft-ietf-quic-multipath-21 | https://datatracker.ietf.org/doc/html/draft-ietf-quic-multipath | Multipath QUIC (relay + direct cùng lúc) |

### Identity / Discovery
| Spec | URL | Dùng cho |
|---|---|---|
| Pkarr spec | https://github.com/Nuhvi/pkarr | DNS TXT với Ed25519 signing |
| RFC 7250 — raw public keys in TLS | https://datatracker.ietf.org/doc/html/rfc7250 | TLS auth bằng Ed25519 pubkey, không cần X.509 |
| Ed25519 RFC 8032 | https://datatracker.ietf.org/doc/html/rfc8032 | EdDSA signing algorithm |

---

## Thuật toán HyParView + PlumTree (chi tiết để extract)

Đây là gossip algorithm trong `iroh_gossip::proto` — thuật toán đúng đắn nhất cho robot swarm, và là phần DÙNG ĐƯỢC nhất từ iroh vì không có tokio dependency.

### HyParView — Partial View Membership

Mỗi node duy trì 2 sets:
```
Active view (size 5):
  - Kết nối trực tiếp, bidirectional
  - Dùng để forward messages ngay lập tức
  - Maintained bằng heartbeat (JOIN/FORWARD_JOIN)

Passive view (size 30):
  - Address book — biết địa chỉ nhưng không kết nối
  - Fallback khi active view peer chết
  - Refreshed bằng SHUFFLE với random peers
```

**Transitions:**
```
JOIN → gửi đến bootstrap peer → FORWARD_JOIN được relay tới random node
DISCONNECT → xóa khỏi active, thêm vào passive của peers
NEIGHBOR → nâng từ passive lên active (khi active view trống)
SHUFFLE → trao đổi passive view subset để tránh partition
```

**Properties:**
- Self-healing: mất node → tự reconnect từ passive view trong O(1) rounds
- Partition-resistant: SHUFFLE liên tục cập nhật passive view
- Bounded state: O(log N) total active connections per node

### PlumTree — Epidemic Broadcast

Xây dựng spanning tree tự động trên active view:

```
Eager push (default):
  Khi nhận message M lần đầu:
    → forward ngay tới tất cả active view peers (trừ sender)
    → mark sender là "eager" (đang trên spanning tree)

Lazy push (IHave):
  Với passive view peers:
    → gửi hash(M) after delay T1 ("tôi có message này nếu bạn cần")
    
Pull (GRAFT):
  Nếu nhận IHave nhưng chưa có M sau T2:
    → gửi GRAFT → upgrade peer đó lên eager
    → nhận full message, forward tiếp

PRUNE:
  Nếu nhận duplicate M từ 2 eager peers:
    → gửi PRUNE tới sender muộn hơn
    → downgrade đó về lazy
```

**Kết quả:** Spanning tree tự emerge, tự optimize theo latency. Không cần central coordinator. Mỗi message đi qua O(N) links thay vì O(N²).

### Extraction path cho Cellos (no_std)

```rust
// iroh-gossip::proto public interface (IO-less):
pub struct State<N: NodeId> {
    // HyParView active/passive views
    // PlumTree eager/lazy/missing state
}

impl<N: NodeId> State<N> {
    pub fn handle(&mut self, input: InEvent<N>) -> Vec<OutEvent<N>>;
    pub fn tick(&mut self, now: Instant) -> Vec<OutEvent<N>>;
}

// InEvent = network receive hoặc app-level publish
// OutEvent = "gửi packet này tới peer X" hoặc "deliver message M lên app"
// Cellos net-broker làm transport layer, State chỉ tính toán
```

**Thay đổi cần thiết để port sang no_std:**
1. `HashMap` → `hashbrown::HashMap` (có no_std support)
2. `std::time::Instant` → kernel `GetTime op=1` (monotonic ms)
3. Random peer selection → dùng `BrokerRng` đã có
4. Remove `net` feature → drop tokio dependency hoàn toàn

---

## Ed25519 NodeId Model (iroh → Cellos mapping)

Iroh dùng `NodeId = Ed25519 public key (32 bytes)` làm stable identity. Cellos đã có Ed25519 qua Silo.

**Proposed mapping:**

```rust
// Cellos có (Silo):
pub struct SiloHandle { /* Ed25519 key pair, hardware-backed */ }

// Proposed addition: CellNetId
pub struct CellNetId([u8; 32]); // Ed25519 pubkey = network identity

// Net-broker derives CellNetId from machine's Silo key:
// CellNetId = Silo::public_key()
// This means: network identity = hardware-attested Ed25519 key
// No separate key management needed
```

**Benefits của model này:**
- Identity stable qua IP changes (DHCP, roaming)
- NodeId có thể dùng như địa chỉ trong discovery (Pkarr DNS)
- Compatible với iroh's relay protocol (relay dùng NodeId để route)
- Cellos's CapSet có thể reference NodeId (cross-machine capability delegation)

---

## Roadmap Gap Analysis: L.2 Internet Connectivity

Dựa trên L.0+L.1 foundation đã build, gaps còn lại để internet hoạt động:

### L.2a: Ticket-based Peer Discovery (G1 internet prerequisite)

**Scope:** Thay thế/bổ sung UDP multicast bằng static peer list + ticket bootstrap.

```rust
// /etc/cellos/cluster.cfg
[peers]
machine_b = "ticket:ABC123..."  // NodeId + known addr
relay = "https://relay.cellos.io"
```

**Implementation:**
- Decode ticket: `NodeId (32B) || relay_url (varlen) || socket_addrs (varlen)`
- net-broker đọc config tại Init, attempt TCP connect trực tiếp hoặc qua relay
- Giữ Noise KKpsk0 handshake y nguyên (không thay đổi)

**Effort:** ~1 phase, không đụng Law 1.

### L.2b: STUN — biết IP public của mình

**Scope:** net-broker query STUN server để biết reflexive address (IP public + port).

```
net-broker → STUN server: Binding Request
STUN server → net-broker: "Bạn đang đến từ 203.0.113.5:4521"
net-broker: lưu reflexive_addr, include trong ticket/beacon
```

STUN là stateless request-response — rất đơn giản, không cần persistent connection. RFC 8489.

**Effort:** ~0.5 phase. Cần thêm 1 UDP message type vào net-broker.

### L.2c: UDP Hole-punch Coordinator

**Scope:** net-broker thực hiện simultaneous hole-punch khi cần kết nối cross-NAT.

**Cần relay server để coordinate:**
```
Machine A                 Relay/Coordinator             Machine B
    │                           │                           │
    │──── "want to connect B" ──►│                           │
    │                           │◄─── "want to connect A" ──│
    │◄──── "B at 8.8.9.9:7777" ─│                           │
    │                           │──── "A at 203.0.113.5:4521" ──►│
    │──── UDP punch ──────────────────────────────────────────────►│
    │◄──── UDP punch ────────────────────────────────────────────── │
    │                    (simultaneous)                             │
    │◄══════════════ Direct UDP connection ═══════════════════════►│
```

Sau khi direct path setup → upgrade Noise KKpsk0 lên direct TCP (hoặc giữ UDP).

**Effort:** ~2 phases. Cần coordinator protocol + timing synchronization.

### L.2d: DERP Relay Cell

**Scope:** Simple relay cell (hoặc external server) cho Symmetric NAT fallback.

```rust
// relay cell đơn giản:
// Nhận: SEND_PACKET(dest_node_id: [u8;32], payload: [u8])
// Forward: tới net-broker của dest_node_id nếu connected
// Không decrypt, không inspect
```

Cho G1: 1 relay server chạy trên Linux (ngoài Cellos) là đủ, vì số robot nhỏ. Relay cell trong Cellos là G2.

**Effort:** ~1 phase cho relay server (Linux process). ~2 phases cho Cellos relay cell.

### L.2e: Gossip Upgrade — HyParView + PlumTree (optional)

**Scope:** Nâng gossip hiện tại (custom UDP multicast) lên HyParView + PlumTree.

**Hiện tại:** L.0+L.1 gossip chỉ dùng để:
- SwarmBeacon discovery (multicast)
- Task-claiming lease (unicast Noise session)

**Benefit của upgrade:**
- Hoạt động qua internet (không cần multicast)
- Tự-heal partition tốt hơn
- O(log N) thay vì O(N) cho large swarm

**Cách extract iroh-gossip::proto:**
```toml
# broker Cargo.toml — thêm:
iroh-gossip = { version = "0.34", default-features = false }
# feature "net" = OFF → không pull tokio
# feature "proto" = ON → chỉ lấy state machine
```

Hoặc port thủ công: ~500 LOC theo spec HyParView + PlumTree papers.

---

## Tổng kết: 3-tier connectivity roadmap

```
Tier 0: Local SAS     ✅ sys_send (kernel IPC)
Tier 1: LAN           ✅ UDP multicast + TCP Noise KKpsk0 (L.0+L.1)
Tier 2: Internet      ❌ → cần L.2a–L.2d

L.2a: Ticket bootstrap        (đơn giản, đủ cho G1 robot provisioned)
L.2b: STUN reflexive addr     (biết IP public của mình)
L.2c: UDP hole-punch          (~90-95% direct connection)
L.2d: DERP relay fallback     (~5-10% symmetric NAT)
L.2e: HyParView gossip        (optional, cho large swarm / cross-internet)
```

**Minimum viable internet (G1):** L.2a (tickets) + L.2d (relay server trên Linux).
- Robot swap tickets lúc provisioning
- Kết nối trực tiếp nếu không có NAT
- Relay nếu bị NAT
- Không cần hole-punch cho 2-robot G1 swarm

**Full internet (G2):** L.2a + L.2b + L.2c + L.2d + Pkarr discovery.

---

*Nghiên cứu này dựa trên iroh v1.0 (release 2026-06-15), IETF draft-seemann-quic-nat-traversal-01, và DERP protocol từ Tailscale.*
