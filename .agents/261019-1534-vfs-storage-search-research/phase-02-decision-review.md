---
phase: 2
title: "Review phương án và quyết định của người dùng"
status: pending
priority: P2
tier: thinking
dependencies: [1]
implementationAuthorized: false
---

# Phase 02 — Chờ quyết định, không tự triển khai

## Overview

Dùng hồ sơ research để người dùng lựa chọn phạm vi nâng cấp. Phase này chưa bắt đầu;
không buộc phải trả lời mọi câu hỏi ngay và không biến một recommendation thành Accepted ADR.

## Decision register — tất cả còn OPEN

| ID | Câu hỏi | Các hướng đang cân nhắc | Cần gì để quyết định? |
|---|---|---|---|
| D01 | Nhu cầu/scale nào phải phục vụ trước? | Workspace code; tài liệu; nhiều agent; tập nhỏ scan trực tiếp | Corpus, số file/bytes, query mix và concurrency thực |
| D02 | Storage nền có cần nâng ngay? | Nâng native capacity/format; backend khác; giới hạn scope nghiên cứu | Runtime xác minh limits, yêu cầu storage và migration/risk budget |
| D03 | Chọn cơ chế recovery nào? | Giữ scope functional; thiết kế journal; thiết kế CoW nhất quán | Failure model, durability contract, recovery tooling; không chọn theo tên |
| D04 | Search engine/API ở đâu? | Cell riêng; VFS API delegate; tích hợp chọn lọc vào VFS | Đo IPC/I/O/CPU, consistency requirements và phạm vi lỗi |
| D05 | Loại search ban đầu? | Name/path; metadata; exact/full-text; semantic bổ sung | Query set và quality criteria; chưa chốt embeddings |
| D06 | Thuộc tính thuộc về ai? | Native generic attributes; authoritative catalog riêng; chỉ derived metadata | Lifetime qua rename/copy/export, ownership và rebuild semantics |
| D07 | Freshness/consistency nào bắt buộc? | Eventual + explicit stale; wait-until-indexed; sync/atomic selected indexes | Consumer requirements và chi phí chấp nhận được |
| D08 | Primitive VFS nào thực sự cần? | Identity/revision; coherent read; change feed; batch/pagination; chưa thêm | Requirement hoặc bottleneck cụ thể; reuse desktop/authority contracts |
| D09 | Cập nhật index bằng gì? | Scan/reconciliation; volatile notifications; durable change log | Writer coverage, restart/gap/overflow semantics, durability nền |
| D10 | Engine và runtime nào? | Chưa chọn; nghiên cứu existing Rust/native engines và portability | API/runtime compatibility, license, resource/maintenance cost |
| D11 | Authority cho index/query? | Caller-scoped access, delegated directory capabilities, partitioned/shared catalog | Identity/revocation semantics, leak review và exact contract |
| D12 | Khi nào thay hoặc giữ kiến trúc? | Giữ Cell; thêm VFS primitive; tích hợp selected index | Baseline/results, ngưỡng và acceptance được chốt trước |

## Những lựa chọn chưa bị bác bỏ

- BFS là reference, không phải backend đã chọn.
- RedoxFS vẫn là nguồn code/kiến trúc để khảo sát; không có quyết định quay lại backend cũ.
- ext4/XFS/Btrfs/OpenZFS là nguồn đối chiếu, không phải tất cả đều khả thi để port hoặc đủ scope.
- Shared Search Cell là hướng đề xuất đáng đo, không phải cam kết sau này chuyển engine vào VFS.
- Scan trực tiếp vẫn là baseline và có thể là lời giải đủ dùng với workload nhỏ.

## Trình tự review khi người dùng muốn tiếp tục

1. Đọc [plan entry](plan.md), [analysis](phase-01-analysis-research.md) và [biên bản](research-notes.md).
2. Refresh code và exact authorization/locks của portfolio, desktop Files và authority owner.
3. Chốt từng requirement cần thiết; có thể để lựa chọn không liên quan tiếp tục deferred.
4. Chỉ chạy phép đo/spike sau khi xác định scope, disposable inputs và evidence ceiling.
5. Ghi recommendation + trade-offs + kết quả có nguồn; trình người dùng chọn, không auto-select.
6. Sau lựa chọn rõ ràng, lập implementation plan riêng hoặc revision có exact file/ABI/format deltas,
   migration/recovery và consumer acceptance; nhận approvals áp dụng trước khi sửa code.

## Success Criteria

- [ ] Người dùng xác nhận các requirements và phương án được chọn, ghi rõ ngày và phạm vi.
- [ ] Mỗi lựa chọn có dữ kiện, rủi ro và các phần vẫn hoãn; không giấu scope reduction.
- [ ] Nếu có storage/ABI cutover: có exact contract, caller census, preservation/migration và approvals riêng.
- [ ] Implementation handoff chỉ xuất hiện sau approval, không chạy bằng research plan này.

## Assumptions

Không giả định người dùng đã đồng ý bất cứ backend/engine/format hoặc performance target nào.
Đề xuất trong hội thoại không phải normative contract.

## Security Considerations

Không nới quyền của indexer/query caller để có demo nhanh; không expose protected names/snippets/counts.
Review theo scope và current authority implementation, không giả định per-agent isolation đã tồn tại.

## Risk Assessment

Có thể revise hoặc bỏ research mà không đổi runtime. Chọn backend/format có thể kéo theo migration
không đảo ngược; phải tách approval trước data effects. Không rollback data chỉ bằng revert code.
Không tự đánh dấu phase hoàn tất để hợp thức hóa implementation.

## Deviation Log

Chưa có quyết định kiến trúc được nhận. 2026-10-19: người dùng yêu cầu giữ plan dang dở để quyết định sau.
