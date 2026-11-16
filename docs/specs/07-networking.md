# Cellos Architecture: Networking Stack
**Version**: 0.4 (user-space stack Cell, copied frames)
**Status**: Definitive
**Amended 2026-11-16** (architecture remediation Phase 19): §1–§5 được sửa cho khớp code; §6 không đổi.

---

## 1. Triết lý: User-space Stack trong một Service Cell
Network Stack không nằm trong nhân (Kernel) mà là một **Service Cell** (`cells/services/net`).
Net Cell, NIC Driver Cell và App Cell là các Cell riêng; chúng chỉ nói chuyện qua IPC của kernel
([Spec 17](17-ipc-wire-contract.md)), nên mỗi bước qua ranh giới Cell là một lần chuyển ngữ cảnh.

* **Thư viện**: **smoltcp 0.11** (Native Rust, `no_std`) — `cells/services/net/Cargo.toml:73`.
* **Bộ nhớ**: Net Cell có heap riêng cố định 8 MiB (`ostd::declare_custom_heap!`,
  `cells/services/net/src/main.rs:31-34`) cho socket buffer, TLS và frame.

## 2. Các thành phần chính
1. **NIC Driver Cell** (`cells/drivers/<chip>/`, ví dụ `virtio-net`, `e1000`, `igb`):
    * Quản lý vòng đệm phần cứng (RX/TX ring) và DMA.
    * Đăng ký là provider `service::NIC_DRIVER` và phục vụ giao thức `wire::proto::nic`
      (`OP_TX`, `OP_RX`, `OP_GETMAC`; `libs/wire/src/proto/nic.rs`).
2. **Stack Cell** (`net`):
    * Chạy smoltcp: máy trạng thái TCP, IP/UDP/ICMP, ARP, DHCP, DNS, link-local.
    * Quản lý socket và port theo owner (`cells/services/net/src/socket_table.rs`).
    * Phục vụ socket IPC kiểu BSD qua message postcard `NetRequest`/`NetResponse`
      (`libs/wire/src/ipc.rs:777`).

## 3. Luồng gói tin (copy qua IPC)
1. **NIC -> RAM**: Phần cứng DMA ghi gói tin vào ring buffer của Driver Cell.
2. **Driver <-> Stack**: Net Cell gửi lệnh `OP_TX` (frame được copy vào request) hoặc `OP_RX`
   bằng `sys_try_send` có hạn rồi `sys_recv_timeout` chờ đúng driver đó; mỗi lúc chỉ có một
   request tới driver (`cells/services/net/src/interface.rs:430-489`, `:518-527`). Frame nhận
   được copy vào buffer từ pool của Net Cell (`interface.rs:266-276`).
3. **Stack -> App**: Payload đi trong reply `NetResponse`, giới hạn inline bởi
   `NET_TCP_INLINE_DATA_MAX` mỗi lần gọi; client copy vào buffer của App
   (`libs/ostd/src/clients/net.rs:272-296`).

Không có bước nào chuyển quyền sở hữu buffer qua ranh giới Cell mà không copy.

## 4. Socket API & ostd Integration
App Cell dùng client của `ostd` (`libs/ostd/src/clients/net.rs`): `TcpStream::connect(addr, port)`
trả về một `TcpStream` cài `embedded_io::Read`/`Write` (`net.rs:246-260`); TLS là
`ostd::clients::tls_stream`. API là đồng bộ: `read` gọi lại và `yield` khi chưa có dữ liệu, không trả
về `Future`. Không có trait `TcpStack`.

**Multi-Arch**: Net Cell là Rust `no_std`; kiến trúc nào có lane là bảng
[`docs/generated/arch-status.md`](../generated/arch-status.md). Code 32-bit không được build.

## 5. Bảo mật & Cô lập mạng
**Port Ownership**: Mỗi port/socket khi mở được gán owner trong Net Cell's
owner-scoped socket/port table; đây là authority của networking lifecycle.

**Capability Check**: Kernel ghi quyền mạng đã review của Cell gửi thành các bit `CALLER_FLAG_NET_*`
trong caller trailer có attestation; Net Cell kiểm quyền tương ứng trước mọi tác động (tra tên,
socket egress, port inbound, raw frame) và từ chối bằng `NetError::Denied`
(`cells/services/net/src/rights.rs:1-13`).

**Fault Recovery**: Kernel không reload Cell. Khi Net Cell chết, init khởi động lại nó theo
`RestartPolicy` của cấu hình boot (`cells/tools/init/src/service_table.rs`); socket thuộc instance
cũ mất theo nó, và App phải mở lại kết nối [INFERENCE: hành vi phía App không được kiểm trong lần sửa này].

## 6. TLS Server Authentication (G14)

### 6.1 Tổng quan
Trước G14, net cell sử dụng `UnsecureProvider` của embedded-tls — chứng chỉ máy chủ **không được xác minh**, dễ bị tấn công MITM. G14 thay thế bằng **`ViTlsProvider`** (`cells/services/net/src/tls/provider.rs`), bọc `embedded_tls::pki::CertVerifier` để thực hiện xác thực đầy đủ theo TLS 1.3.

### 6.2 Những gì được xác minh (build mặc định `tls-roots-embedded`)
- **Chuỗi chứng chỉ (chain)**: `pki::CertVerifier` duyệt leaf → intermediates → root anchor được cấu hình.
- **Thời hạn hiệu lực**: `notBefore`/`notAfter` so sánh với đồng hồ thực từ RTC (`GetTime` op=3, epoch seconds). Giá trị được kẹp tại `Cellos_MIN_UNIX` (build-time floor) để đồng hồ chưa đặt (epoch 0) không phá vỡ kiểm tra.
- **Hostname (RFC 6125)**: khớp với `SubjectAltName` dNSName. Giới hạn của `pki.rs`: tối đa 3 dNSName, hostname ≤ 64 ký tự — endpoint vượt quá cần build `tls-roots-full`.
- **SNI rỗng bị từ chối** trước khi bắt tay (handshake).

### 6.3 Build flavors (chọn tại OS-image build time)

| Feature flag | CA anchor | Chi phí ảnh |
|---|---|---|
| `tls-roots-embedded` + `tls-ca-private` *(mặc định)* | Self-signed ECDSA P-256 fleet CA | +21 KB |
| `tls-roots-embedded` + `tls-ca-amazon` | Amazon Root CA 3 (ECDSA P-256) | +21 KB |
| `tls-roots-embedded` + `tls-ca-letsencrypt` | ISRG Root X2 (ECDSA P-384) | +100 KB |
| `tls-roots-embedded` + RSA roots | RSA opt-in (nặng) | +135 KB |
| `tls-roots-full` *(dự kiến, chưa triển khai)* | rustls-webpki multi-root cho PC/server | — |
| `tls-insecure` | `UnsecureProvider`, không xác minh | dev/lab ONLY |

`tls-insecure` in banner **`INSECURE TLS BUILD`** tại runtime. CI không được ship build này.

### 6.4 Xử lý kết nối thất bại
- Chứng chỉ không chain về anchor → kết nối thất bại (cap 0), net cell log: `connect REJECTED — certificate verification failed`.
- Timeout transport → log riêng: `transport I/O` — hai lỗi **không bao giờ bị lẫn lộn**.

### 6.5 Rủi ro còn lại (đã chấp nhận)
1. **Revocation (OCSP/CRL) ngoài phạm vi** — chuẩn ngành cho embedded (AWS/Azure IoT áp dụng tương tự). Biện pháp giảm thiểu: short-lived certs, SPKI pinning, OTA trust-anchor update.
2. **RTC manipulation**: kiểm tra hết hạn chứng chỉ chỉ đáng tin khi đồng hồ đáng tin. Clamp `Cellos_MIN_UNIX` chỉ bảo vệ khỏi đồng hồ quá sớm (epoch 0), không bảo vệ khỏi đồng hồ bị đẩy sai về phía tương lai.
3. **Không có Certificate Transparency / name-constraint / path-length** ngoài những gì `pki.rs` thực hiện.
4. **DER parser là attack surface** trên chứng chỉ thù địch; được parse bởi stack `der`/RustCrypto dưới `#![forbid(unsafe_code)]`.
5. **Chỉ TLS 1.3** — embedded-tls không hỗ trợ TLS 1.2, nên tấn công downgrade giao thức là **không thể** (đây là lợi thế tích cực).
