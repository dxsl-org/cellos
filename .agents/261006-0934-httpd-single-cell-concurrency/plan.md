---
title: "HTTPD: concurrent requests in one cell"
description: "Bounded single-owner reactor, real asynchronous IPC and 256-connection qualification"
status: pending
priority: P1
tier: thinking
tags: [backend, api, feature]
created: 2026-10-06
blockedBy: [260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc]
---

# Kế hoạch nâng cấp httpd một cell

## Quyết định
Một cell httpd, một reactor sở hữu socket/request/buffer; không spawn cell hoặc thread theo request. Request chờ mạng/VFS/AI chuyển sang Pending để reactor phục vụ request khác. Không bọc synchronous IPC trong async fn. Chưa triển khai code.

“Không chờ” = không có head-of-line blocking do một client/backend chậm; không phải zero latency, vô hạn tài nguyên hoặc CPU chạy song song. Đặt mục tiêu qualification **256 kết nối HTTP đang hoạt động**, đo theo các mốc 8/32/64/128/256; không tự động coi các mốc nhỏ là hoàn thành yêu cầu hàng trăm.

## Scope và bất biến
- Giữ CLI, port, route, payload thành công, GET file động và POST AI hiện có; vẫn Connection: close, một request/kết nối.
- Request chưa đủ body tuyệt đối không dispatch. Giới hạn framing 4 KiB hiện tại phải được áp dụng chính xác; malformed/oversize/unsupported framing trả lỗi rồi đóng.
- Không thêm HTTP/2, WebSocket, keep-alive, TLS termination, autoscaling, cell-per-request, generic CPU worker pool hoặc auth framework.
- Không tăng MAX_CELLS; concurrency là state trong cùng cell, không số cell.
- Chỉ một chủ sở hữu allocation/receive trong httpd; không dựa vào allocator ostd thread-safe.
- Net vẫn một chủ sở hữu SocketSet. Backend state/DB/AI service riêng hiện có không tính là cell request mới.

## Giai đoạn
| # | Deliverable | Dependency | Status |
|---|---|---|---|
| 01 | [Contracts, ngân sách và oracle](phase-01-contracts.md) | none | pending |
| 02 | [Async IPC adapter và net readiness](phase-02-transport.md) | 01 + shared async prerequisite | pending |
| 03 | [Net progress, capacity, cleanup](phase-03-network.md) | 01, 02 contracts | pending |
| 04 | [HTTP reactor và handler state machines](phase-04-httpd.md) | 02, 03 | pending |
| 05 | [Qualification tải và cutover](phase-05-qualification.md) | 04 | pending |

02 và 03 có thể làm song song sau khi chốt contract, nhưng một integration owner giữ net runtime/ABI. Không fork kế hoạch async IPC đã có; chưa đủ transport thì không tuyên bố httpd đã nonblocking.

## Tiêu chí kết thúc
1. 256 client có request in-flight, fast route hoàn thành trong khi slow body/slow reader/backend khác vẫn Pending; không sinh thêm httpd cell/task theo client.
2. Mọi route hiện có đi qua scheduling hữu hạn; không giữ reactor trong receive/send-all/generate/read-file loop.
3. Dữ liệu response đúng từng client, partial write không mất/lặp byte, EOF/reset/timeout có cleanup.
4. Quá tải: bounded memory/queue, 503 khi đã accept và có budget gửi lỗi; hết transport slot thì từ chối/đóng có định nghĩa, không hứa mọi client nhận HTTP 503.
5. Sau disconnect/timeout/restart, cap/socket/backend session trở về baseline trong deadline cleanup; idle không busy-spin.
6. QEMU chứng minh correctness RV64 + AArch64; P95/P99/RPS công bố kèm profile và workload, không suy rộng QEMU sang production hardware.

## Gate, assumptions và rủi ro
- Law 1: thiết kế ABI cần approval trước edit; approval lần hai sau exact delta + evidence. Yêu cầu lập kế hoạch này không được coi là hai approval.
- 256 là mục tiêu đề xuất, chưa được đo. Ước lượng tối thiểu 256×(4 KiB request + 8 KiB TCP RX/TX) ≈3 MiB, chưa tính listener, TX application, metadata, file/session, heap overhead.
- Async prerequisite đang pending; socket readiness/correlation mới phải review owner-generation, lost-wakeup và completion capacity. 32 completion slots không thể ngầm hiểu thành 256 operations.
- Backend có capacity thấp hơn frontend phải trả Busy/503, không quảng cáo 256 AI inference chạy song song.
- Bằng chứng hiện tại và phân tích: [scout-report.md](scout-report.md). Chưa có benchmark tải; smoke đã phát hiện partial body bị dispatch sớm trên image hiện có.

## Validation / adversarial self-review
Đã đối chiếu: accept thực tế immediate probe, IPC vẫn blocking; allocator không an toàn cho workers; DNS/TLS/driver có thể chặn net; close không chứng minh TX drained; reply timeout không phải cancel; ABI cần gate; cap chết cần reap; incomplete body không được tạo side effect. Đây là self-review, không phải independent approval.

## Handoff
`$hc-cook /home/dmin/cellos/.agents/261006-0934-httpd-single-cell-concurrency/plan.md`
Bắt đầu Phase 01; không sửa ABI trước approval và không bỏ các route chậm để thu hẹp nghiệm thu.
