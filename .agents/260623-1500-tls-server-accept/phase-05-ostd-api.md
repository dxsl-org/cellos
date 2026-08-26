# Phase 05 — ostd `tls_listen` / `tls_accept` API

## Overview
- **Priority:** P1 · **Tier:** medium · **Status:** Planned
- **Phụ thuộc:** Phase 04 (net cell handlers hoạt động)
- Expose `tls_listen()` + `tls_accept()` trong `libs/ostd/src/tls.rs` — mirror đúng pattern của `tls_connect()` hiện tại.
- Sau phase này: bất kỳ cell nào (httpd, net-broker) có thể dùng TLS server chỉ bằng ostd API, không cần biết net cell internals.

## Context Links
- Existing client API: `libs/ostd/src/tls.rs:37-115` — `tls_connect()`, `tls_write()`, `tls_read()`, `tls_close()`
- Opcode constants (trong ostd): `TLS_CONNECT_OP = 0x30`, `TLS_SEND_OP = 0x31`, `TLS_RECV_OP = 0x32`

## New Opcode Constants

Thêm vào `libs/ostd/src/tls.rs` (cạnh các constant hiện có):
```rust
pub const TLS_LISTEN_OP: u8 = 0x33;
pub const TLS_ACCEPT_OP: u8 = 0x34;
// TLS_CLOSE_OP = 0x15 đã có, dùng chung cho cả client và server cap
```

## New API Functions

```rust
/// Tạo TLS server listener trên port.
/// Trả về listen_cap (u64). 0 = fail.
pub fn tls_listen(net_tid: u64, port: u16) -> u64 {
    let mut buf = [0u8; 3];
    buf[0] = TLS_LISTEN_OP;
    buf[1..3].copy_from_slice(&port.to_le_bytes());
    sys_send(net_tid, &buf);
    let mut resp = [0u8; 8];
    sys_recv_from(net_tid, &mut resp);
    u64::from_le_bytes(resp)
}

/// Accept một TLS connection đến (NON-BLOCKING).
/// Trả về:
///   - nonzero: stream_cap, handshake complete — dùng với tls_write/tls_read/tls_close
///   - 0: net cell signal error (bad listen_cap)
///   - Retry signal: net cell trả về 0xFE... (caller phải retry)
///
/// Gọi trong loop đến khi trả về nonzero != 0xFE_sentinel.
/// sys_yield() giữa các lần retry để tránh busy-spin.
pub fn tls_accept(net_tid: u64, listen_cap: u64) -> u64 {
    let mut buf = [0u8; 9];
    buf[0] = TLS_ACCEPT_OP;
    buf[1..9].copy_from_slice(&listen_cap.to_le_bytes());
    sys_send(net_tid, &buf);
    let mut resp = [0u8; 8];
    sys_recv_from(net_tid, &mut resp);
    u64::from_le_bytes(resp)
}

/// tls_write — UPDATED wire format: length prefix thay zero-scan
/// Fix: binary TLS data có thể chứa 0x00 — old zero-scan cắt payload.
pub fn tls_write(net_tid: u64, cap_id: u64, data: &[u8]) -> usize {
    let mut buf = [0u8; 512];
    buf[0] = TLS_SEND_OP;
    buf[1..9].copy_from_slice(&cap_id.to_le_bytes());
    let data_len = data.len().min(buf.len() - 13); // 1 opcode + 8 cap + 4 len
    buf[9..13].copy_from_slice(&(data_len as u32).to_le_bytes()); // length prefix
    buf[13..13 + data_len].copy_from_slice(&data[..data_len]);
    sys_send(net_tid, &buf[..13 + data_len]);
    // ... recv bytes_written response
}
```

**Note:** `tls_write()` cũng cần update để add length prefix — phối hợp commit với Phase 04 handler update.

**Note:** `tls_write()`, `tls_read()`, `tls_close()` **KHÔNG thay đổi** — chúng đã dùng cap_id làm key, hoạt động với server cap qua `TlsEntry` dispatch (Phase 04).

## Wire Format

**TLS_LISTEN_OP (0x33):**
```
Request:  [0x33][port_lo][port_hi]
Response: [cap_id: 8 bytes LE]  (0 = fail)
```

**TLS_ACCEPT_OP (0x34):**
```
Request:  [0x34][listen_cap: 8 bytes LE]
Response: [stream_cap: 8 bytes LE]  (0xFE... = not ready, retry; 0 = error; nonzero = cap)
```

**TLS_SEND_OP (0x31) — UPDATED with length prefix:**
```
Request (old): [0x31][cap:8LE][data:*]          ← zero-scan end, BUGGY for binary data
Request (new): [0x31][cap:8LE][len:4LE][data:*] ← explicit length, fixes 0x00 truncation
```
**P05 phải update `tls_write()` cùng lúc Phase 04 updates handler — commit cùng nhau.**

TLS_RECV_OP (0x32) và TLS_CLOSE_OP (0x15) không thay đổi.

## Usage Pattern (sẽ được httpd dùng ở Phase 06)

```rust
// Typical TLS server cell pattern
let net_tid = sys_lookup_service(service::NET).expect("net online");

// Một lần tại Init
let listen_cap = tls_listen(net_tid, 443);
assert!(listen_cap != 0, "TLS listen failed");

// Accept loop
loop {
    let stream_cap = tls_accept(net_tid, listen_cap);
    if stream_cap == 0 { continue; } // timeout, retry

    // stream_cap dùng giống client cap
    let data = tls_read(net_tid, stream_cap, 4096);
    tls_write(net_tid, stream_cap, &response);
    tls_close(net_tid, stream_cap);
}
```

## mTLS Extension Point (G2 / broker)

Khi net-broker cần mTLS (Phase 04 robot swarm upgrade), net cell cần accept client cert. Không cần Law 1 change — dùng thêm raw opcode hoặc flag trong TLS_LISTEN payload:

```
// G2 option (KHÔNG implement trong plan này):
// TLS_LISTEN_OP extended: [0x33][port:2][flags:1]
//   flags bit 0: require_client_cert
// Khi set: ServerConfig dùng with_client_auth_required(ca_verifier)
```

**Quan trọng:** API `tls_listen(net_tid, port)` hiện tại sẽ KHÔNG cần thay đổi signature cho G1. G2 có thể thêm `tls_listen_mtls(net_tid, port)` variant hoặc flags parameter. Document trong code.

## Related Code Files

**Sửa:**
- `libs/ostd/src/tls.rs` — thêm 2 opcode constants + 2 functions

**Không thay đổi:**
- `libs/api/src/ipc.rs` ← Law 1 safe

## Implementation Steps

1. Thêm `TLS_LISTEN_OP = 0x33`, `TLS_ACCEPT_OP = 0x34` constants vào `ostd/src/tls.rs`
2. Implement `tls_listen(net_tid, port) -> u64`
3. Implement `tls_accept(net_tid, listen_cap) -> u64`
4. Thêm doc comment mô tả mTLS extension point (G2)
5. `cargo check` ostd → no errors

## Todo
- [ ] Opcode constants 0x33 + 0x34
- [ ] tls_listen() implementation + doc
- [ ] tls_accept() implementation + doc
- [ ] mTLS extension point comment
- [ ] cargo check ostd xanh

## Success Criteria
- `tls_listen()` + `tls_accept()` exist và compile trong ostd
- Wire format match chính xác Phase 04 handler expectations
- `tls_write`/`tls_read`/`tls_close` KHÔNG thay đổi — backward compatible
- ostd unit tests (nếu có) còn xanh

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Wire format mismatch với Phase 04 handler | M×H | Align bảng opcode map Phase 04 và Phase 05; test E2E ở Phase 06 |
| sys_recv_from blocking format mismatch | L×M | Follow exact pattern của tls_connect() response parsing |
