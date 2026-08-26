# Scout Report — TLS Server Accept

Reconnaissance chạy 2026-06-23 trước khi viết plan.

## TLS Module Map (cells/services/net/src/tls/)

| File | Nội dung |
|------|----------|
| `socket.rs:63-121` | `TlsSocketEntry::handshake()` — 32KB Box buffers, ViTlsProvider hoặc UnsecureProvider |
| `transport.rs:109-187` | `SmoltcpTlsTransport` — embedded-io bridge, spin-poll 30s timeout |
| `rng.rs:20-40` | `ViRng` — ChaCha20 seed từ `sys_get_random()`, không fallback |
| `provider.rs:38-52` | `ViTlsProvider` — single pinned CA, hostname check |
| `clock.rs:43-52` | `ViTlsClock` — `sys_get_wall_secs()` clamp floor |
| `roots.rs` | CA DER bytes via `include_bytes!()` + feature flags |

## TCP Server Foundation (đã có, hoạt động)

- `handlers.rs:188-233`: `TcpListen` + `TcpAccept` + `TcpClose` — httpd dùng thật
- `socket_table.rs:15`: MAX 18 sockets (16 user + DHCP + ARP)
- `socket_table.rs:66-79`: `insert_with_state()` — accept tạo socket mới, renew listener

## TLS IPC Path

- Opcodes raw (bypass NetRequest postcard): `handle_tls_raw()` `handlers.rs:353`
- 0x30 TLS_CONNECT, 0x31 TLS_SEND, 0x32 TLS_RECV, 0x15 TLS_CLOSE
- `tls_table: BTreeMap<u64, TlsSocketEntry>` tại `main.rs:86`
- ostd wrapper: `libs/ostd/src/tls.rs` — `tls_connect()`, `tls_write()`, `tls_read()`, `tls_close()`

## embedded-tls Version

```
embedded-tls = { version = "0.19", default-features = false, features = ["alloc"] }
```
CA features: `rustpki/p384/rsa` mapped to `tls-ca-*` Cargo features.

## Critical Finding: embedded-tls 0.19 là CLIENT-ONLY

- Không có `TlsAcceptor`, `ServerConfig`, server handshake state machine
- GitHub issue #51 ("Server Support") mở từ 2022, không có tiến triển
- → Bắt buộc dùng thư viện khác cho server path

## Library Decision

| Option | Verdict |
|--------|---------|
| embedded-tls server | IMPOSSIBLE — không có server API |
| mbedtls | BLOCKED — TLS 1.2 only (TLS 1.3 abandoned) |
| ring + rustls | BLOCKED — ring không compile riscv64gc-unknown-none-elf |
| aws-lc-rs + rustls | BLOCKED — aws-lc-rs requires std |
| **rustls 0.23 + hand-roll CryptoProvider** | **VIABLE** — UnbufferedServerConnection no_std+alloc, pure-Rust RustCrypto |
| rustls-rustcrypto | Risky ("DO NOT USE IN PRODUCTION") |

## Existing Tests

- `cells/demos/http-smoke/` — TLS client → external Python mock server (port 8443)
- `cells/services/net/src/tls/` — unit tests cho provider, clock, roots (không test server)
- `cells/services/httpd/` — plaintext HTTP server, không test TLS

## Silo IPC (G2 extension point)

- `cells/apps/silo-test/src/main.rs` — `SiloHandle::connect()` + `sign()` + `GetPub()`
- Silo `Sign` opcode: output DER ECDSA sig (max 72B) — compatible với rustls ECDSA scheme
- `rustls::sign::SigningKey` + `sign()` blocking → OK cho synchronous net cell
