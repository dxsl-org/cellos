# Phase 06 — httpd HTTPS + E2E smoke test

## Overview
- **Priority:** P1 · **Tier:** medium · **Status:** Planned
- **Phụ thuộc:** Phase 05 (ostd `tls_listen`/`tls_accept` API)
- Wire httpd cell để serve HTTPS thật (port 443 hoặc 8443 trong QEMU). Cập nhật `http-smoke` để test client→server TLS (Cellos TLS client kết nối với Cellos TLS server).
- Cuối phase: `curl https://10.0.2.15:8443/` PASS, http-smoke TLS client→server E2E PASS.

## Context Links
- httpd cell: `cells/services/httpd/src/main.rs:29-56` — hiện accept TCP, serve HTML + REST
- http-smoke: `cells/demos/http-smoke/src/main.rs:82-128` — hiện dùng external mock TLS server
- ostd TCP API: `cells/services/httpd/src/main.rs` dùng `net_ipc::tcp_listen/tcp_accept/tcp_close`
- mock server: `tools/hypha-mock-llm/mock_proxy.py`

## Thay đổi httpd — cần restructure

**⚠️ Red team finding:** httpd hiện tại (`main.rs:42-56`) **block hoàn toàn** trên một connection trước khi accept connection tiếp theo. Không phải "sequential polling loop". Cũng dùng `#[no_mangle] pub fn main()` (DEPRECATED — CLAUDE.md cấm dùng cho code mới). Phải restructure httpd entry point.

**Chiến lược:** Migrate httpd sang `ostd::app_entry!()` pattern + polling loop. Serve HTTP và HTTPS bằng non-blocking accepts (retry với yield).

```rust
// cells/services/httpd/src/main.rs — RESTRUCTURED

ostd::app_entry!(handler = httpd_handler);

fn httpd_handler(ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            let net_ep = ctx.net_endpoint();
            let vfs_ep = ctx.vfs_endpoint();
            
            // Listen cả 2 ports
            let http_listen  = net_ipc::tcp_listen(HTTP_PORT, net_ep).expect("http listen");
            let https_listen = tls::tls_listen(net_ep, HTTPS_PORT);
            assert!(https_listen != 0, "TLS listen failed");
            
            // Store in app state
            ctx.set_state(HttpdState { http_listen, https_listen, net_ep, vfs_ep });
        }
        AppEvent::Tick | AppEvent::Message { .. } => {
            let state = ctx.state::<HttpdState>();
            
            // Non-blocking HTTP accept
            let http_stream = net_ipc::tcp_accept_nb(state.http_listen, state.net_ep);
            if http_stream > 0 {
                handle_http_connection(http_stream, state.net_ep, state.vfs_ep);
            }
            
            // Non-blocking TLS accept (net cell drives handshake internally)
            let tls_stream = tls::tls_accept(state.net_ep, state.https_listen);
            if tls_stream > 0 && tls_stream != NOT_READY {
                handle_https_connection(tls_stream, state.net_ep, state.vfs_ep);
            }
            
            sys_yield();
        }
        AppEvent::Shutdown => sys_exit(0),
        _ => {}
    }
}
```

**Canonical pattern tham khảo:** `cells/apps/hello-cell/src/main.rs` (app_entry), `cells/apps/robot-dashboard/src/main.rs` (ViUI + message loop).

### handle_https_connection

```rust
fn handle_https_connection(stream_cap: u64, net_ep: u64, vfs_ep: u64) {
    // Đọc HTTP request qua TLS
    let request_bytes = tls::tls_read(net_ep, stream_cap, 4096);
    let response = router::handle_request(&request_bytes, vfs_ep);

    // Ghi HTTP response qua TLS
    tls::tls_write(net_ep, stream_cap, &response);
    tls::tls_close(net_ep, stream_cap);
}
```

`router::handle_request()` không biết gì về TLS — nó chỉ xử lý HTTP bytes. Tái sử dụng hoàn toàn từ HTTP path.

## Trust chain cho test

Vấn đề: http-smoke dùng embedded-tls client với `roots/private.der` là CA. Server cert của httpd được ký bởi CA mới (tạo ở Phase 03). Cần đảm bảo:

```
Phase 03:
  CA cert → cells/services/net/roots/private.der  (REPLACE)
  Leaf cert → cells/services/net/certs/server.der (signed by CA)

Phase 06:
  http-smoke TLS client (embedded-tls) → load CA từ roots/private.der → verify server leaf cert
  → PASS nếu CA và leaf cert cùng chain
```

Cả net cell (server, dùng leaf cert) và ostd/embedded-tls client đều compile-time embed cùng `roots/private.der` → trust chain tự động đúng sau Phase 03.

**QEMU IP gotcha:** server cert SAN phải có `10.0.2.15` (QEMU user-mode network default IP). Đã include trong Phase 03 script.

**Hostname:** http-smoke phải dùng hostname matching SAN. Nếu connect qua IP, SAN phải có IP entry. Phase 03 đã add `10.0.2.15` và `127.0.0.1` vào SAN.

## http-smoke update

Hiện tại: `http-smoke` dùng external Python mock server (`tools/hypha-mock-llm/mock_proxy.py`) làm TLS server.

Sau Phase 06: thêm test case **Cellos TLS server** (kết nối tới httpd HTTPS thật):

```rust
// cells/demos/http-smoke/src/main.rs

// Existing: TLS client → external mock (8443)
// New: TLS client → Cellos httpd (8443 internal)

const HTTPD_IP:   [u8; 4] = [10, 0, 2, 15];  // QEMU default
const HTTPD_HTTPS: u16    = 8443;

fn test_httpd_https(net_ep: u64) {
    let cap = tls::tls_connect(net_ep, HTTPD_IP, HTTPD_HTTPS, "cellos-httpd.local");
    assert!(cap != 0, "TLS connect to httpd failed");

    tls::tls_write(net_ep, cap, b"GET / HTTP/1.0\r\nHost: cellos-httpd.local\r\n\r\n");
    let resp = tls::tls_read(net_ep, cap, 2048);
    assert!(resp.starts_with(b"HTTP/1."), "unexpected response");
    tls::tls_close(net_ep, cap);

    log::info!("httpd HTTPS E2E: PASS");
}
```

**init spawn order:** init cần spawn httpd TRƯỚC http-smoke, và http-smoke cần đợi httpd sẵn sàng (sleep ngắn hoặc retry).

## Manual test với curl

Sau khi QEMU đang chạy:
```powershell
# Từ host Windows
curl --insecure https://10.0.2.15:8443/
# hoặc nếu muốn verify cert:
curl --cacert <(openssl x509 -in cells/services/net/roots/private.der -inform DER) https://cellos-httpd.local:8443/
```

`--insecure` dùng để quick sanity check; verify cert đầy đủ dùng CA file.

## Related Code Files

**Sửa:**
- `cells/services/httpd/src/main.rs` — thêm HTTPS listen loop
- `cells/services/httpd/Cargo.toml` — add ostd TLS dep (nếu chưa có)
- `cells/demos/http-smoke/src/main.rs` — thêm httpd HTTPS test case

**Không thay đổi (Law 1):**
- `libs/api/src/ipc.rs`

## Implementation Steps

1. Sửa `httpd/src/main.rs`: thêm `tls_listen(net_ep, HTTPS_PORT)` tại Init
2. Thêm `handle_https_connection()` trong httpd (reuse router)
3. Thêm HTTPS accept trong httpd main loop (sequential poll)
4. Build + boot: verify httpd không crash khi có TLS listen
5. Sửa `http-smoke/src/main.rs`: thêm `test_httpd_https()` test case
6. Run trong QEMU: `run.ps1` (hoặc `run-arm64.ps1`)
7. Verify `http-smoke` log "httpd HTTPS E2E: PASS"
8. Optional: verify với `curl --insecure https://10.0.2.15:8443/` từ host

## Todo
- [ ] httpd: tls_listen tại Init
- [ ] httpd: handle_https_connection() (reuse router)
- [ ] httpd: HTTPS accept trong main loop
- [ ] build + boot test (no crash)
- [ ] http-smoke: test_httpd_https() case
- [ ] boot QEMU + verify E2E PASS
- [ ] Optional: curl verify từ host

## Success Criteria
- `http-smoke` log: `httpd HTTPS E2E: PASS`
- httpd trả về HTTP 200 qua TLS cho client kết nối đến port 8443
- TLS handshake hoàn thành với cert verification (không phải `tls-insecure` mode)
- HTTP (port 8080) vẫn hoạt động bình thường (không regress)
- Client TLS path (tls_connect → external mock) không bị regress

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| httpd accept loop blocking HTTP khi chờ HTTPS | M×M | Sequential poll với timeout ngắn; demo scope chấp nhận được |
| http-smoke không đợi httpd ready → connect fail | M×M | Retry loop hoặc sleep 1-2s trước test_httpd_https |
| QEMU IP (10.0.2.15) thay đổi theo QEMU version | L×M | Hardcode trong script + cert SAN; kiểm tra nếu ping fail |
| router không parse request đúng qua TLS (encoding) | L×M | Router nhận raw bytes — transparent; test với đơn giản GET / trước |
| curl không có trong test environment | L×L | Dùng http-smoke là primary test; curl là optional manual |

## Validation Gate
Phase 06 PASS khi:
1. `http-smoke` chạy trên Cellos, log rõ `httpd HTTPS E2E: PASS`
2. TLS handshake dùng **cert verification thực sự** (không insecure mode)
3. HTTP plaintext path không bị ảnh hưởng
