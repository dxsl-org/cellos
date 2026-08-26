# Red-Team Report — Cell-to-Cell Anywhere Plan

> **Verdict: BLOCKED** — 4 STOP + 5 FATAL. Plan phải được viết lại trước khi implement.
> Date: 2026-06-24 · Reviewers: Security, Systems, Protocol, Scope (4 adversarial agents)

---

## STOP-level Bugs (phải fix trước khi tiếp tục)

### STOP-1: Flagship API không tồn tại và không thể tồn tại như mô tả
**Reviewer:** Scope · **Phase:** Toàn bộ plan

`ctx.cluster().peer("robot-B")?.call::<SensorCell, _>(|s| s.read_temperature()).await`

API thực tế trong `libs/ostd/src/cluster.rs`: chỉ có `ClusterRef::lookup_remote(service_id: u16) -> Option<usize>` — một raw TID. Không có `peer(name)`, không có `call(closure)`, không có async proxy.

Nghiêm trọng hơn: `call(|s| s.read_temperature())` serialize **Rust closure qua network** — về cơ bản là impossible. Không có serialization format nào có thể encode arbitrary closures.

**Fix:** Phải spec remote-call API contract ở P00 (gate phase) trước tất cả phases khác. Contract cần định nghĩa: codec wire format, service dispatch, timeout/error semantics, type boundaries.

---

### STOP-2: iroh-gossip `proto` feature không tồn tại
**Reviewer:** Systems + Scope · **Phase:** P04

Plan viết: `iroh-gossip = { features = ["proto"] }` → "NO tokio, NO net"

Reality: iroh-gossip 0.34 không có feature nào tên `proto`. Available features: `metrics`, `net`, `examples`, `rpc`, `simulator`, `test-utils`. Default bao gồm `net` → pulls tokio + quinn. Không có documented no_std support.

"Extraction path" là fiction. Fallback "port thủ công ~800 LOC" là kế hoạch thực, nhưng bị underestimate và chưa verified.

**Fix:** Drop P04 iroh-gossip extraction approach. Nếu muốn HyParView, phải port từ paper (không phải từ crate). Cần spike trước.

---

### STOP-3: Relay URL không dial được — DNS missing
**Reviewer:** Systems + Protocol · **Phase:** P03, P06

`relay_url: heapless::String<128>` = "https://relay.cellos.io"

Nhưng `NetRequest::TcpConnect { addr: [u8;4], port }` chỉ nhận IPv4 raw. `NetRequest::Resolve` → `Err(0xFF)` — DNS resolver chưa implement (`handlers.rs:321`). STUN cũng bị ảnh hưởng (stun.l.google.com không resolve được).

**Fix:** Cho G1, hardcode IP thay vì hostname trong config. Hoặc ship DNS resolver trước. Pkarr (P06) bị block hoàn toàn.

---

### STOP-4: HyParView F=5 xung đột ConnectionPool K=4 và MAX_SOCKETS=18
**Reviewer:** Systems + Protocol · **Phase:** P04

- `MAX_SOCKETS = 18` (socket_table.rs:15) — shared DHCP+ARP+tất cả
- `ConnectionPool K ≤ 4` (transport.rs:39) — Noise session pool
- HyParView active view F=5 → cần 5 Noise sessions đồng thời
- Baseline đã consume: 1 DHCP + 1 ARP + 1 beacon UDP + 4 Noise TCP = 7 sockets
- P02 STUN thêm: 1 UDP
- P03 relay: 1 TCP persistent
- P04 HyParView active view: 5 Noise sessions — **vượt cả ConnectionPool và gần MAX_SOCKETS**

HyParView về kiến trúc là impossible với các ràng buộc hiện tại.

**Fix:** Drop P04 từ plan này. Nếu N>10 robots trong tương lai, tăng MAX_SOCKETS và K pool trước khi thiết kế gossip.

---

## FATAL-level Bugs

### FATAL-1: CellNetId không được bind vào Noise handshake
**Reviewer:** Security · **Phase:** P01 + P07

Plan thêm `CellNetId` (Ed25519 pubkey) như identity mới, nhưng handshake vẫn auth bằng K1 PSK (`transport.rs:124`). Ed25519 NodeId không bao giờ được verify trong Noise session.

Kết quả: bất kỳ peer nào có K1 đều pass auth — NodeId chỉ là routing hint, không phải authentication.

**Fix:** Thêm Ed25519 NodeId signature vào Noise prologue (bound vào transcript) tại P01. Không phải G2.

---

### FATAL-2: WebSocket không có trong no_std Cellos
**Reviewer:** Systems · **Phase:** P03

Plan viết DERP qua WebSocket "vì vượt corporate firewall." Cellos không có no_std WebSocket client — cần HTTP Upgrade + Sec-WebSocket-Key SHA1 + masking + framing. Không có gì trong codebase.

**Fix:** Dùng raw TCP framing cho DERP (không có WS). Corporate firewall evasion là G3+ concern.

---

### FATAL-3: NoiseSession TCP-only, không migrate sang UDP
**Reviewer:** Protocol · **Phase:** P05

`NoiseSession` trong `transport.rs:106-208` hardwire vào TCP stream framing. P05 "upgrade lên direct UDP" sau hole-punch là một transport mới từ đầu, không phải reuse.

**Fix:** P05 cần spec NoiseSession over UDP riêng (datagram framing ≠ stream framing). Significant additional work.

---

### FATAL-4: Hole-punch timing qua relay không có synchronized clock
**Reviewer:** Protocol · **Phase:** P05

"Cùng thời điểm" hole-punch cần time synchronization. `GetTime op=1` là per-machine monotonic reset on reboot — không compare được cross-node. Relay RTT variable.

**Fix:** Đúng cơ chế là relay gửi "GO" signal → cả 2 bắt đầu fire probes ngay khi nhận. Không dùng timestamp comparison. Spray 5-10 probes mỗi 20ms để cover jitter.

---

### FATAL-5: Law 1 mislabeled trong P01
**Reviewer:** Systems · **Phase:** P01

Plan header P01 viết "Không đụng Law 1" nhưng deliverables đặt `CellNetId` + `PeerTicket` vào `libs/api/src/cluster.rs` và Law 1 table cuối plan liệt kê `libs/api/src/cluster.rs` + `libs/api/src/ipc.rs`.

**Fix:** P01 VÀ bất kỳ phase nào thêm variant vào `NetRequest`/`NetResponse` đều cần 2x user confirmation. Phải flag rõ ràng.

---

## WARN-level Issues

### WARN-1: HyParView là YAGNI cho G1 2-robot swarm
**Reviewer:** Scope · **Phase:** P04

HyParView được thiết kế cho N=100-1000 nodes. G1 milestone = 2 robots. Active view=5 > total nodes=2. Hoàn toàn không cần thiết.

---

### WARN-2: Ba internet servers là SaaS commitment, không phải sprint task
**Reviewer:** Scope · **Phase:** P03, P06

DERP relay server (~200 LOC) + Pkarr/DNS server (~500 LOC) + STUN server → vận hành TLS certs, uptime, DDoS protection, abuse handling. Không phải "infrastructure footnote."

**Fix:** G1 point client tới iroh's existing public relay (https://relay1.fly.dev của n0). Không tự build relay server.

---

### WARN-3: STUN response parsing ignore 6-byte src-header của UdpRecv
**Reviewer:** Systems · **Phase:** P02

`handlers.rs:278` inject 6-byte src addr header vào UDP recv buffer. STUN parse sketch không account cho điều này.

---

### WARN-4: P04 trước P05 — sai thứ tự priority
**Reviewer:** Scope · **Phase:** ordering

Direct connectivity (P01→P02→P03→P05) là critical path cho internet. HyParView (P04) chỉ cần khi N>10. Resequence: P01, P02, P03, P05, rồi P04 nếu cần.

---

### WARN-5: P07 K2 + P08 K3 out of scope cho plan này
**Reviewer:** Scope · **Phase:** P07, P08

K2 KMS Cell là whole subsystem. K3 DICE cần hardware không có ở G1 QEMU. Cả 2 thuộc plan security riêng.

---

## Những gì GIỮ LẠI từ plan gốc

| Component | Verdict | Lý do |
|---|---|---|
| P01 CellNetId concept | ✅ Keep (fix) | Stable identity đúng. Thêm NodeId→Noise binding |
| P01 Ticket format | ✅ Keep | Simple, no infra needed |
| P02 STUN client | ✅ Keep | Small, RFC-defined, critical |
| P03 DERP relay CLIENT | ✅ Keep (scope down) | Dùng iroh public relay, raw TCP (no WS) |
| HyParView algorithm docs | ✅ Keep as reference | Reference spec cho tương lai |
| PlumTree algorithm docs | ✅ Keep as reference | Reference spec cho tương lai |
| DERP wire format docs | ✅ Keep | Needed for client |
| P04 HyParView build | ❌ Drop | YAGNI, socket conflict, feature fiction |
| P05 hole-punch | ⚠️ Defer | Valid but needs test harness + NoiseUDP first |
| P06 Pkarr/DNS server | ❌ Drop | Doctrine violation + DNS missing |
| P07 K2 | ❌ Move | Separate security plan |
| P08 K3 DICE | ❌ Move | Separate, hardware-gated |

---

## Re-scoped Plan: G1 Minimum Viable Internet

```
P00: Remote-call API contract spec (GATE — phải approve trước khi implement)
P01: CellNetId + Ticket + NodeId→Noise binding (⚠️ Law 1)
P02: STUN reflexive address (hardcoded IP server)
P03: DERP relay client (iroh public relay, raw TCP, no WebSocket)
→ STOP: G1 internet milestone shipped
(P05 hole-punch: separate plan, sau khi P01-P03 stable + test harness ready)
```

**Effort:** ~8-10 pts (giảm từ 30+ pts trong plan gốc)
