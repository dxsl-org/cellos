# Plan: TLS Server-Side Accept (rustls 0.23 dual-stack)

## ⚠️ STATUS: PARKED — G2, và là PHƯƠNG ÁN DỰ PHÒNG (không phải default G2)

**Quyết định 2026-06-23 (đồng bộ với roadmap §L "Transport security by tier"):** Robot swarm đã chuyển sang **Noise** cho broker-to-broker transport. TLS server không còn use case G1. Plan này giữ nguyên như foundation cho G2 — không xóa, không implement G1.

> 🧭 **Định vị plan này trong 3 use case transport (xem roadmap §L):**
> 1. **Native Cell↔Cell (cluster transport)** → **Noise** ở MỌI stage (G1 K1 PSK → G2 K3 per-node+DICE). Plan riêng `.agents/260623-0907-net-broker-robot-swarm/`. **Plan TLS-server NÀY KHÔNG liên quan tới cluster transport** — đừng nhầm "có rustls server" = "cluster dùng TLS".
> 2. **Cluster interop với hạ tầng ngoài (k8s mesh/enterprise)** → mTLS từ **Tier 3b Linux VM / external LB**, KHÔNG xây trong kernel.
> 3. **Cell Cellos serve HTTPS cho web client ngoài (httpd)** → use case của plan NÀY.
>
> ⚠️ **GATE quyết định trước khi implement (P02–P06) ở G2:** Theo nguyên tắc §L *"đừng terminate TLS trong kernel/net-cell nếu terminate được bên ngoài"*, **đường mặc định G2** để serve HTTPS ra ngoài là terminate TLS ở **external LB hoặc trong Tier 3b Linux VM** (nginx/caddy trong Alpine) — KHÔNG build rustls vào net cell. Plan này (rustls-in-net-cell) chỉ là **phương án dự phòng cho edge node bị ràng buộc**: node standalone/edge robot phải expose HTTPS trực tiếp, KHÔNG có LB và KHÔNG đủ chỗ chạy Linux VM. **Phải loại trừ external/VM termination trước, rồi mới chạy plan này.**

- **P01 spike** (compile rustls 0.23 no_std): vẫn worth chạy khi có thời gian (~1h, de-risk) — chạy *sau* khi GATE ở trên chọn in-net-cell
- **P02–P06**: defer đến G2 **và chỉ khi GATE chọn in-net-cell termination**
- **embedded-tls** (client-only): giữ nguyên mãi mãi, không đụng — là default cho mọi profile kể cả nano robot

---

**Goal:** Add TLS server-side accept vào net cell — phục vụ external HTTP clients (curl, browser) kết nối tới httpd qua HTTPS, **CHỈ cho edge node không terminate TLS được bên ngoài**. Scope này là **G2-only**: G1 robot không expose HTTPS server, mọi inter-cell transport dùng Noise.

**Strategy:** Dual-stack — giữ nguyên embedded-tls cho client (HTTPS client đang PASS, không đụng vào), thêm rustls 0.23 **optional** chỉ cho server path (`feature = "tls-server"`).

**End state (khi implement G2):** httpd phục vụ HTTPS thật (curl + http-smoke client→server TLS E2E). Nano robot builds dùng `--no-default-features --features tls-client` → zero rustls code.

---

## Decision Log (research 2026-06-23)

| Quyết định | Chọn | Lý do |
|---|---|---|
| Thư viện server | rustls 0.23 (no_std+alloc) | embedded-tls 0.19 không có server path; mbedtls TLS 1.2 only; ring/aws-lc-rs cần std |
| Crypto provider | Hand-roll `CellosRustlsProvider` từ RustCrypto | ring/aws-lc-rs bị loại; rustls-rustcrypto "DO NOT USE IN PRODUCTION"; RustCrypto đã có trong cây |
| TLS stack | Dual-stack (embedded-tls client + rustls server) | Không regress HTTPS client vừa ship 2026-06-23; YAGNI |
| Cert provisioning | rcgen (host build) ký leaf cert bằng CA ĐÃ CÓ; DER embed `include_bytes!()` | Không replace `roots/private.der` → không vỡ http-smoke external mock |
| IPC opcodes | Raw opcodes 0x33 (TLS_LISTEN), 0x34 (TLS_ACCEPT); **0x31 thêm length prefix** | Extend `handle_tls_raw`; Law 1 safe; 0x31 fix zero-scan truncation cho binary data |
| Stream cap reuse | `TlsEntry { Client, Server }` enum trong cùng `tls_table` | Cells không cần biết client/server; `tls_write`/`tls_read` hoạt động với cả hai |
| TLS_ACCEPT model | **Non-blocking** (trả về 0xFE nếu chưa xong), net cell drive handshake per main-loop iteration | Tránh net cell starvation (single-threaded service) |
| SmoltcpTlsTransport | **Không reuse** cho rustls unbuffered — server dùng own TCP I/O loop | rustls unbuffered là pull/push byte model, incompatible với embedded-io transport |
| `UnbufferedServerConnection` | Track `status.discard` sau mỗi `process_tls_records()` call | Tránh infinite reprocess loop |
| httpd entry | Migrate từ deprecated `#[no_mangle] pub fn main()` sang `ostd::app_entry!()` | CLAUDE.md cấm deprecated entry; cần non-blocking accept loop |
| Scope | httpd HTTPS E2E (G2 only) | Broker dùng Noise_KKpsk/NNpsk (plan riêng); nano-robot = TLS client only mãi mãi |
| **Default G2 cho external HTTPS** | **External LB / Tier 3b VM termination** (KHÔNG phải plan này) | §L: đừng terminate TLS trong kernel/net-cell nếu terminate được bên ngoài; rustls-in-net-cell = dự phòng edge-only |
| Priority | G2 / PARKED / dự phòng | G1 không expose HTTPS server; Noise thay thế use case swarm; TLS server in-net-cell chỉ cho edge node không có LB/VM |
| Cargo feature | `tls-server` optional; `tls-client` default | Dev chọn: nano-robot build = `--no-default-features --features tls-client` → zero rustls |

---

## Phase Overview

| Phase | Tên | Tier | Status | Phụ thuộc |
|-------|-----|------|--------|-----------|
| [P01](phase-01-compile-spike.md) | rustls compile spike (go/no-go) | fast | Planned | — |
| [P02](phase-02-crypto-provider.md) | `CellosRustlsProvider` (RustCrypto glue) | thinking | Planned | P01 PASS |
| [P03](phase-03-cert-provisioning.md) | rcgen cert tooling + disk image | medium | Planned | P01 PASS |
| [P04](phase-04-server-socket-entry.md) | `TlsServerSocketEntry` + net cell handlers | thinking | Planned | P02 + P03 |
| [P05](phase-05-ostd-api.md) | ostd `tls_listen`/`tls_accept` API | medium | Planned | P04 |
| [P06](phase-06-httpd-https-e2e.md) | httpd HTTPS + E2E smoke test | medium | Planned | P05 |

**Parallelizable:** P02 và P03 có thể chạy song song sau khi P01 PASS.

---

## Key Files (sẽ thay đổi)

| File | Thay đổi |
|------|----------|
| `libs/cellos-rustls-provider/` | **TẠO MỚI** — crate RustCrypto CryptoProvider |
| `cells/services/net/src/tls/server.rs` | **TẠO MỚI** — TlsServerSocketEntry + ServerConfig loading |
| `cells/services/net/src/tls/mod.rs` hoặc `tls.rs` | Thêm `TlsEntry` enum, export server module |
| `cells/services/net/src/handlers.rs` | Thêm handler cho 0x33/0x34; `tls_table` → `BTreeMap<u64, TlsEntry>` |
| `cells/services/net/Cargo.toml` | Thêm rustls dep + cellos-rustls-provider |
| `libs/ostd/src/tls.rs` | Thêm `tls_listen()`, `tls_accept()`, opcode constants 0x33/0x34 |
| `cells/services/httpd/src/` | Wire HTTPS path dùng `tls_listen`/`tls_accept` |
| `scripts/gen-server-cert.ps1` | **TẠO MỚI** — rcgen host script sinh CA + leaf cert DER |
| `cells/services/net/certs/server.der` + `server-key.der` | **TẠO MỚI** — generated at build time |
| `cells/services/net/roots/private.der` | **REPLACE** — new CA cert (từ rcgen script) |

**Không thay đổi:**
- `libs/api/src/ipc.rs` (Law 1 safe — không add `NetRequest`/`NetResponse` variant)
- `cells/services/net/src/tls/socket.rs` (client path giữ nguyên)
- `cells/services/net/src/tls/provider.rs` (client ViTlsProvider giữ nguyên)

---

## Critical Risk

| Risk | Mức | Xử lý |
|------|-----|-------|
| rustls 0.23 không compile `riscv64gc-unknown-none-elf` | HIGH | P01 spike trước mọi thứ; fallback documented |
| `rustls::unbuffered` API churn giữa 0.23.x patch | MED | Pin version chính xác; đọc CHANGES.md trước dùng |
| Server private key embed plaintext trong binary | MED | Acceptable G1 LAN; `// SECURITY(dev):` comment rõ; G2 → SiloSigningKey |
| 18-socket budget (net cell) bị cạn khi có TLS listen | LOW | 1 listen + N stream; max concurrent gated nhỏ hơn budget |

---

## Fallback nếu P01 fail

Nếu rustls 0.23 không compile no_std riscv64:
1. Kiểm tra xem std dep nào chặn → patch `rustls/Cargo.toml` tắt feature đó
2. Nếu không patch được: giữ Phase 04 robot swarm ở HMAC plain TCP; defer TLS server sang G2 khi target là hosted (linux-gnu)
3. Report kết quả cho user trước khi tiếp tục P02

---

## Cook Invocation

> ⚠️ **PARKED — G2.** Không cook plan này cho G1.
>
> **GATE bắt buộc trước khi cook (xem STATUS ở đầu file):** chỉ implement plan này nếu đã LOẠI TRỪ external-LB / Tier-3b-VM termination cho node đó (per §L, external/VM termination là default G2; rustls-in-net-cell chỉ cho edge node bị ràng buộc). Nếu node có LB hoặc chạy được Linux VM → KHÔNG cook plan này, dùng nginx/caddy ngoài.

```
/hc-cook .agents/260623-1500-tls-server-accept/plan.md
```

Trước khi cook: chạy P01 spike trước để confirm rustls 0.23 compile trên `riscv64gc-unknown-none-elf`.
