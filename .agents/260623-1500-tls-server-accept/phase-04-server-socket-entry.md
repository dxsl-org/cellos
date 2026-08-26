# Phase 04 — `TlsServerSocketEntry` + net cell handlers

## Overview
- **Priority:** P1 · **Tier:** thinking · **Status:** Planned
- **Phụ thuộc:** Phase 02 (CellosRustlsProvider) + Phase 03 (cert files)
- Core implementation: tạo `TlsServerSocketEntry`, thêm opcodes 0x33/0x34, hợp nhất tls_table thành `TlsEntry` enum.
- ⚠️ **Law 1 safe**: không thay đổi `libs/api/src/ipc.rs`

## Context Links
- Client path (template): `cells/services/net/src/tls/socket.rs:63-121`
- Raw opcode handler: `cells/services/net/src/handlers.rs:353-524`; zero-scan tại `:366`
- TCP non-blocking accept: `cells/services/net/src/handlers.rs:202-233` (returns 0xFE if not ready)
- SmoltcpTlsTransport global context: `cells/services/net/src/tls/transport.rs:34-52` (AtomicPtr)
- tls_table: `cells/services/net/src/handlers.rs:96`
- TLS_BUF constant (client): `cells/services/net/src/tls/socket.rs:40` — 16640 bytes

## Red Team Findings Applied

Các vấn đề từ adversarial review đã được tích hợp vào thiết kế này:

| Finding | Mức | Fix trong phase này |
|---------|-----|---------------------|
| TLS_ACCEPT blocking starvation (30s spin-loop trong net cell) | BLOCKER | Non-blocking design (return 0xFE) |
| Zero-scan truncation trên binary TLS records | BLOCKER | Length prefix `[len:4LE]` trong wire format TLS server opcodes |
| SmoltcpTlsTransport không reuse được cho rustls unbuffered | MAJOR | Server dùng own TCP I/O loop với manual smoltcp read/write |
| UnbufferedServerConnection discard tracking bị bỏ qua | MAJOR | Explicit `status.discard` tracking trong poll loop |
| Buffer size: 16384 thiếu overhead → dùng 16640 | MINOR | `SERVER_TLS_BUF = 16640` khớp với `TLS_BUF` client |

---

## Architecture

### TlsEntry enum

```rust
pub enum TlsEntry {
    Client(TlsSocketEntry),      // existing — KHÔNG thay đổi
    Server(TlsServerSocketEntry), // mới
}
```

`tls_table` (`BTreeMap<u64, TlsEntry>`) chứa **cả client và server** caps. Opcodes 0x31/0x32 dispatch qua `TlsEntry::send()`/`recv()`. Cells không phân biệt client/server cap.

### Non-blocking TLS_ACCEPT (BLOCKER fix)

`TcpAccept` trong net cell là **non-blocking** (trả về 0xFE "not ready" nếu chưa có connection, handlers.rs:212). `TLS_ACCEPT_OP` phải theo cùng model.

**TLS_ACCEPT_OP (0x34) state machine:**
```
net cell nhận 0x34 với listen_cap →
  Lookup TCP socket cho listen_cap:
  
  CASE 1: TCP state ≠ Established → return 0xFE (client chưa connect)
  
  CASE 2: TCP Established, không có entry trong tls_handshaking_table →
    allocate stream_cap, tạo TlsServerSocketEntry(Handshaking),
    lưu vào tls_handshaking_table[stream_cap]
    renew TCP listener (như TcpAccept:224-228)
    advance handshake một bước (drain available TCP data)
    return 0xFE (handshake đang tiến hành)
  
  CASE 3: tls_handshaking_table[listen_cap] còn đang Handshaking →
    advance một bước → return 0xFE
  
  CASE 4: tls_handshaking_table có stream_cap cho listen_cap với state Connected →
    move entry từ tls_handshaking_table → tls_table (TlsEntry::Server)
    return stream_cap  ← DONE
```

**Tại sao stream_cap tracking:** Mỗi listen_cap có thể có tối đa một TLS handshake đang tiến hành tại một thời điểm (single-threaded net cell). Khi TLS_ACCEPT được gọi, nó vừa drive handshake vừa check xem handshake trước đó (nếu có) có xong chưa không.

### Net cell main loop: drive handshaking entries

Ngoài xử lý IPC, main loop cũng advance các handshake đang tiến hành khi smoltcp có data mới:
```
loop {
    receive_and_handle_one_message();
    smoltcp_poll();
    advance_all_handshaking_entries(); // ← mới
}

fn advance_all_handshaking_entries() {
    for entry in tls_handshaking_table.values_mut() {
        if entry.state == Handshaking {
            entry.poll_one_step(smoltcp_context);
        }
    }
}
```

→ Handshake tiến nhanh hơn ngay cả khi cell không gọi TLS_ACCEPT mỗi vòng.

### Zero-scan Fix: length prefix (BLOCKER fix)

Hiện tại `handle_tls_raw` dùng zero-scan (`:366`) để tìm payload end — sẽ cắt binary TLS records.

**Wire format mới cho opcodes 0x33/0x34:**
```
TLS_LISTEN (0x33):  [0x33][port_lo][port_hi]             (3 bytes, no binary payload)
TLS_ACCEPT (0x34):  [0x34][listen_cap: 8LE]              (9 bytes, no binary payload)
```
→ 0x33/0x34 không có binary payload → zero-scan issue không áp dụng cho control opcodes.

**Wire format 0x31/0x32 hiện tại:**
```
0x31 (SEND): [0x31][cap:8LE][data:*]  ← zero-scan để tìm end-of-data
0x32 (RECV): [0x32][cap:8LE][len:4LE] ← fixed format, không bị ảnh hưởng
```

**Đối với server `send()` path (0x31):** zero-scan trên TLS APP_DATA (sau handshake) vẫn bị truncate nếu application data có `0x00`. Đây là bug tồn tại CÙNG CHO CLIENT PATH — không phải chỉ server. Phải fix cùng lúc:
```
0x31 NEW: [0x31][cap:8LE][len:4LE][data:len bytes]  ← length-prefixed
```
**→ Thay đổi wire format 0x31 là breaking change với ostd `tls_write()`!**

**Chiến lược:** Fix 0x31 wire format ở Phase 04 + Phase 05 cùng lúc:
- Phase 04: update handler TLS_SEND_OP (0x31) để đọc length prefix
- Phase 05: update `tls_write()` trong ostd để gửi length prefix
- Thay đổi này là **backward-compatible** nếu không có code nào khác dùng raw 0x31 ngoài ostd

### SmoltcpTlsTransport KHÔNG reuse được (MAJOR fix)

`SmoltcpTlsTransport` (transport.rs) dùng AtomicPtr global context + embedded-io `Read`/`Write` traits. rustls `UnbufferedServerConnection` là **pull/push byte model** — feed raw TCP bytes in, get raw TLS bytes out. Hai model không tương thích.

**Server cần own TCP I/O loop:**
```rust
impl TlsServerSocketEntry {
    fn poll_one_step(&mut self, ctx: &mut SmoltcpCtx) -> HandshakeState {
        // 1. Đọc raw TCP bytes vào input_buf
        let tcp_in = ctx.tcp_recv(self.tcp_handle, &mut self.input_buf);
        
        // 2. Feed vào rustls, lấy state + discard count
        let (state, status) = self.conn.process_tls_records(&self.input_buf[..tcp_in]);
        
        // 3. CRITICAL: advance input cursor bởi status.discard
        self.input_cursor += status.discard;
        
        // 4. Flush output ra TCP ngay — TRƯỚC process_tls_records lần sau
        if let ConnectionState::EncodeTlsData { ref tls_data } = state {
            let encoded = tls_data.encode();
            ctx.tcp_send(self.tcp_handle, encoded);
        }
        
        match state {
            ConnectionState::WriteTraffic { .. } => HandshakeState::Connected,
            ConnectionState::Closed => HandshakeState::Failed,
            _ => HandshakeState::Handshaking,
        }
    }
    
    // Application data send (sau handshake)
    fn send(&mut self, data: &[u8], ctx: &mut SmoltcpCtx) -> usize { ... }
    fn recv(&mut self, len: usize, ctx: &mut SmoltcpCtx) -> &[u8] { ... }
}
```

**Note:** `ctx.tcp_recv()` và `ctx.tcp_send()` gọi trực tiếp smoltcp SocketSet functions, không qua AtomicPtr global. Cần extract smoltcp I/O helpers từ transport.rs hoặc handlers.rs vào shared utility.

### `TlsServerSocketEntry` struct

```rust
const SERVER_TLS_BUF: usize = 16640; // match TLS_BUF client (tls/socket.rs:40)

pub struct TlsServerSocketEntry {
    tcp_handle: SocketHandle,
    conn: UnbufferedServerConnection,
    input_buf:  Box<[u8; SERVER_TLS_BUF]>,
    output_buf: Box<[u8; SERVER_TLS_BUF]>,
    input_cursor: usize, // tracks discard from process_tls_records
    state: ServerState,
}

pub enum ServerState { Handshaking, Connected, Closed }
```

## Wire Format Summary (updated)

| Opcode | Format | Notes |
|--------|--------|-------|
| 0x33 TLS_LISTEN | `[0x33][port:2LE]` | No binary payload |
| 0x34 TLS_ACCEPT | `[0x34][listen_cap:8LE]` | Non-blocking; returns 0xFE or stream_cap |
| **0x31 TLS_SEND (updated)** | `[0x31][cap:8LE][len:4LE][data:len]` | **Breaking: add length prefix** |
| 0x32 TLS_RECV | `[0x32][cap:8LE][len:4LE]` | Unchanged |
| 0x15 TLS_CLOSE | `[0x15][cap:8LE]` | Unchanged, works for client+server cap |

## Tables in NetState

```rust
// cells/services/net/src/main.rs — NetState struct
struct NetState {
    socket_table: SocketTable,
    tls_table: BTreeMap<u64, TlsEntry>,           // connected (client + server)
    tls_handshaking_table: BTreeMap<u64, TlsHandshakingEntry>, // handshake in progress
    server_config: Arc<ServerConfig>,             // built once at Init
    next_cap_id: u64,
}
```

## Related Code Files

**Tạo mới:**
- `cells/services/net/src/tls/server.rs` — `TlsServerSocketEntry`, `TlsHandshakingEntry`, `build_server_config()`

**Sửa:**
- `cells/services/net/src/tls/mod.rs` — `TlsEntry` enum
- `cells/services/net/src/handlers.rs`:
  - `tls_table` → `BTreeMap<u64, TlsEntry>`
  - Thêm `tls_handshaking_table: BTreeMap<u64, TlsHandshakingEntry>`
  - `handle_tls_raw`: thêm 0x33, 0x34; update 0x31 length prefix
  - Main loop: `advance_all_handshaking_entries()` sau smoltcp poll
- `cells/services/net/src/main.rs` — `server_config` trong NetState; init tại `Init`
- `cells/services/net/Cargo.toml` — rustls + cellos-rustls-provider + rustls-pki-types

**Không thay đổi:**
- `libs/api/src/ipc.rs` ← Law 1 safe
- `cells/services/net/src/tls/socket.rs` (client path)

## Implementation Steps

1. Tạo `tls/server.rs`: `TlsServerSocketEntry` + `build_server_config()` + `poll_one_step()` (discard tracking + output flush)
2. Thêm `TlsEntry` enum vào `tls/mod.rs`
3. Thêm `tls_handshaking_table` vào NetState; init tại `Init` kèm `server_config`
4. `handlers.rs`: migrate `tls_table` type; update 0x31 handler (length prefix); thêm 0x33 (TLS_LISTEN, non-blocking); thêm 0x34 (TLS_ACCEPT, non-blocking state machine)
5. `handlers.rs`: thêm `advance_all_handshaking_entries()` sau smoltcp poll
6. `Cargo.toml`: add rustls + provider deps
7. `cargo check` → fix errors
8. `cargo build` → fix lifetime/borrow issues

## Todo
- [ ] tls/server.rs: TlsServerSocketEntry + build_server_config
- [ ] poll_one_step với discard tracking + flush-before-next-call ordering
- [ ] TlsEntry enum
- [ ] tls_handshaking_table trong NetState
- [ ] 0x33 handler (TLS_LISTEN, non-blocking)
- [ ] 0x34 handler (TLS_ACCEPT, non-blocking state machine)
- [ ] 0x31 handler update (length prefix — phối hợp với Phase 05)
- [ ] advance_all_handshaking_entries() trong main loop
- [ ] cargo build xanh

## Success Criteria
- `cargo build` net cell PASS
- TLS_LISTEN (0x33) trả về listen_cap, socket state = Listening
- TLS_ACCEPT (0x34) trả về 0xFE khi chưa có connection / handshake chưa xong; trả về stream_cap khi handshake complete
- Net cell KHÔNG bị starvation trong khi handshake tiến hành (main loop tiếp tục xử lý messages khác)
- Client TLS path (embedded-tls) không bị regress

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| `UnbufferedServerConnection::process_tls_records()` API shape khác với kỳ vọng | M×H | Đọc rustls source (server_conn.rs + unbuffered mod) + examples trước khi implement |
| Discard tracking sai → infinite reprocess loop | M×H | Unit test handshake với echo TCP backend trước khi E2E |
| 0x31 length prefix breaking change sync giữa P04 + P05 | M×M | P04 và P05 commit cùng lúc; cargo check xác minh |
| tls_handshaking_table leak (handshake timeout không cleanup) | M×M | Timeout 30s với cleanup; test timeout path |
| smoltcp poll không được gọi đủ trong handshake loop | M×M | advance_all_handshaking_entries() gọi smoltcp poll trước khi advance |
