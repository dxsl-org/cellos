---
title: "Nâng cấp VFS/storage và tìm kiếm cho AI agent — analysis/research"
description: "Lưu nghiên cứu BFS, RedoxFS và filesystem hiện đại; xác định nhu cầu và bằng chứng trước khi quyết định kiến trúc Cellos."
status: in-progress
stage: analysis-research
paused: true
pauseReason: "Chờ người dùng suy nghĩ và quyết định các lựa chọn kiến trúc; chưa có implementation approval."
implementationAuthorized: false
priority: P2
tier: thinking
tags: [vfs, cellosfs, storage, search, bfs, redoxfs, research, deferred]
created: 2026-10-19
blockedBy: []
blocks: []
---

# Nâng cấp VFS/storage và tìm kiếm cho AI agent

## Trạng thái và quyền thực hiện

**Plan đang dang dở tại analysis/research, chủ động tạm dừng để quyết định sau.**
Người dùng yêu cầu lưu lại trao đổi, không yêu cầu chốt giải pháp hoặc triển khai.
`in-progress` mô tả nghiên cứu chưa kết thúc, không phải admission cho coding.
Không sửa ABI, VFS, native disk format, image hoặc dữ liệu theo plan này.
Không tự chạy benchmark/spike hay `$hc-cook` chỉ vì mở lại plan.

## Mục tiêu

AI agent cần tìm file và nội dung nhiều, nhanh, đúng scope; phân tích độc lập:
1. Khả năng lưu trữ và mở rộng của CellosFS/VFS.
2. Search/index service, metadata và cập nhật incremental.
3. Consistency, authority, recovery và chi phí tài nguyên của từng phương án.

## Giai đoạn

| Phase | Deliverable | Trạng thái |
|---|---|---|
| 01 | [Phân tích hiện trạng và chương trình research](phase-01-analysis-research.md) | in-progress; paused; source recon có, runtime evidence chưa có |
| 02 | [Review lựa chọn và ghi quyết định của người dùng](phase-02-decision-review.md) | pending; chưa chọn backend/engine/format |

Không có phase implementation được phê duyệt. Chỉ lập sau quyết định rõ ràng.

## Hồ sơ đã lưu

- [Biên bản trao đổi, so sánh kiến trúc và nguồn tham khảo](research-notes.md).
- [Hiện trạng source, hạn chế và tiêu chí đo](phase-01-analysis-research.md).
- [Decision register và điều kiện mở lại](phase-02-decision-review.md).

## Ranh giới với kế hoạch khác

- [Portfolio](../plan-portfolio.md): đăng ký parked research, không mở execution lane.
- [Desktop Files/VFS phase](../261019-1310-unified-desktop/phase-07-files-vfs.md): pagination, metadata và thao tác Files có overlap; không nhân đôi owner hoặc biến local search thành full-disk indexing.
- [Cell/Kernel authority plan](../261011-1200-cell-contract-authority/plan.md): đọc trạng thái và exact locks mới nhất khi resume; plan này không nới CP1/CP2, frozen-format hoặc production gates.
- [ADR 0016](../../docs/decisions/0016-cellosfs-native-replaces-redoxfs.md): native backend hiện hành; không mở lại quyết định backend bằng một ghi chú research.

## Evidence ceiling và bước tiếp theo

Đã đọc source/tài liệu trong cuộc trao đổi ngày 2026-10-19; không chạy filesystem runtime,
crash test hoặc benchmark so sánh. Mọi nhận xét tốc độ và tính phù hợp chưa đo đều là giả thuyết.
Khi người dùng yêu cầu resume: đọc cả hồ sơ, refresh source và authority hiện hành,
rồi bổ sung research/đề xuất lựa chọn; không coi các đề xuất trong hội thoại là quyết định đã nhận.
