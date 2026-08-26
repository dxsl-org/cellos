# Scout Report — ostd::http + ostd::json

> Codebase facts grounding the plan. Read by `/hc-cook`, `/hc-review`, `/hc-debug` to skip re-scouting.

## Crate under change: `libs/ostd`

- `libs/ostd/src/lib.rs` — `#![no_std]` (L6), `extern crate alloc` (L9). **No `#![forbid(unsafe_code)]`** (ostd is a library, not a Cell; Law 4 applies to Cells). Modules declared L13–L93.
- `pub use embedded_io` (lib.rs:26) — clients impl traits without a direct dep.
- `pub use heapless` (lib.rs:30); `pub mod clients` (lib.rs:90) hosts NetClient/VfsClient/InputClient.
- No `http` module, no `json` module today.

## `libs/ostd/Cargo.toml` deps (current)
```
types, api (path) · spin, fontdue (workspace) · linked_list_allocator 0.10
hashbrown 0.15 (no-default, default-hasher) · embedded-io 0.7 (no-default)
heapless 0.8 (no-default) · serde 1 (no-default)
```
Absent: `serde_json`, `httparse`, `embedded-tls` (TLS lives in net cell).

## Precedent — `cells/services/httpd/Cargo.toml`
```
httparse  = { version = "1", default-features = false }   # L14
serde     = { version = "1", default-features = false, features = ["derive"] }  # L15
serde_json = { version = "1", default-features = false, features = ["alloc"] }  # L16
```
→ Both deps already build no_std+alloc in this workspace. Versions proven.

## Transport surface

### TCP — `libs/ostd/src/clients/net.rs`
- `NetClient`: `tcp_connect([u8;4],u16)->ViResult<SocketId>` (L39), `tcp_send` (L50), `tcp_recv(id,buf_len)->Vec<u8>` (L61), `tcp_close` (L73), `dns_lookup` (L84), `local_ip` (L95).
- `TcpStream { client, id }` (L126): `connect(addr,port)->ViResult<Self>` (L135); **impls `embedded_io::Read` (L152, blocking + `yield_now`) + `Write` (L176, chunks at MAX_TCP_WRITE = 4096-256 = 3840 B)**; `Drop` closes (L142).
- ErrorType = `crate::io::OstdError`.

### TLS — `libs/ostd/src/tls.rs` (GAP: no embedded_io impl)
- Raw IPC funcs, each takes `net_tid: usize`:
  - `tls_connect(net_tid, addr, port, hostname)->u64 cap_id` (L37); hostname ≤497 B (L40).
  - `tls_write(net_tid, cap_id, data)->usize` (L62); **payload ≤503 B/call, may be partial** (L64,L74).
  - `tls_read(net_tid, cap_id, buf)->usize` (L84); 0 if no data yet (L97).
  - `tls_close(net_tid, cap_id)` (L107).
- Opcodes: 0x30 CONNECT, 0x31 SEND, 0x32 RECV, 0x15 CLOSE (L26-29).
- **No struct wraps these as embedded_io::Read+Write** → HTTPS cannot use a generic transport client until one is added (Phase 04).

## embedded_io impls in ostd
`Stdin` (io.rs:136), `Stdout` (io.rs:153), `File` R/W/Seek (fs.rs:202+), `TcpStream` (net.rs:148). Error bridge `OstdError(ViError)` (io.rs:13) impls `embedded_io::Error` (io.rs:23).

## Code to generalize — `cells/apps/hypha/llm-gateway/src/http.rs` (102 lines)
| Fn | Lines | Note |
|---|---|---|
| `build_chat_body(model,prompt)` | 12-18 | hypha-specific (stays in hypha) |
| `build_post(host,path,body)` | 22-31 | HTTP/1.0 + `Connection: close` → generalize to RequestBuilder |
| `json_escape(s)` | 34-48 | superseded by serde_json |
| `http_body(resp)` | 51-55 | find body after `\r\n\r\n` → superseded by header parse |
| `extract_content(body)` | 60-102 | single-key extractor (P0 hack) → superseded by serde_json |

Header self-documents os-gaps **G1** (no HTTP lib) + **G4** (no_std JSON). Hypha is OUT of this plan's scope (excluded by request); migration is a follow-up.

## Test harness note (open question — see phase 01 risk)
ostd is `#![no_std]`. Pure byte-manipulation fns (request builder, chunked decoder, json parse over `&[u8]`) are host-testable via `#[cfg(test)]` if the crate compiles for the host target. Verify; otherwise fall back to a QEMU smoke cell (pattern: `cells/tools/shell/src/shell_test.rs`, or `cells/demos/https-demo`).
