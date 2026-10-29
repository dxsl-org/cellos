---
phase: 1
title: "Phân tích hiện trạng, research và thiết kế phép đo"
status: in-progress
paused: true
priority: P2
tier: thinking
dependencies: []
implementationAuthorized: false
---

# Phase 01 — Analysis/research, chưa kết thúc

## Overview

Lưu source recon và các câu hỏi cần nghiên cứu trước khi nâng cấp VFS/storage/search.
Không thiết kế sẵn disk format, schema index hay engine khi yêu cầu còn mở.

## Requirements

- Phân biệt VFS contract, filesystem on-disk format và search/index engine.
- Phân biệt directory lookup index, secondary attribute index, full-text và semantic index.
- Không chọn giải pháp chỉ vì mới, viết Rust, có CoW hoặc có tên feature giống nhau.
- Mọi benchmark phải cùng corpus, scope, query semantics, cache state và freshness requirements.
- Không suy ra production/crash guarantees từ comment, feature list hoặc host-only smoke.

## Hiện trạng đã đọc trong source

Snapshot quan sát ngày 2026-10-19; line numbers là gợi ý điều hướng, cần refresh khi resume.

| Quan sát | Evidence |
|---|---|
| P4 `/data`, P5 `/srv` dùng native backend | `cells/services/vfs/src/manager.rs:59-90`; ADR 0016 |
| VFS protocol thực tế là typed request/response | `libs/api/src/services/ipc.rs:37-349` |
| Chưa thấy public xattr/index/query/file-change subscription trong API đã khảo sát | VFS protocol trên; `cells/services/vfs/src/backend.rs`; CellosFS format/engine |
| Owner watches dọn state khi Cell chết, không phải file watches | `cells/services/vfs/src/manager/owned_state.rs:4-59` |
| File handles hiện lưu authorized path, không expose stable native object identity | `cells/services/vfs/src/file_handles/table.rs:17-24,50-87` |
| Hypha tool-fs có read/write/list; không có search tool trong đường đã khảo sát | `cells/apps/hypha/tools/fs/src/main.rs:7-33,59-125` |
| Tool read tối đa 64 KiB; output tối đa 2.048 ký tự | constants và read path trong tool-fs; không dùng output cắt này để index corpus |
| Shell có recursive content scan qua VFS, chưa phải search service | `cells/tools/shell/src/text_tools/grep/io.rs`; có logic matching trong text-engine để khảo sát tái sử dụng |
| Có embedding primitive, chưa thấy corpus retrieval nối vào Hypha | `cells/services/ai/src/main.rs:321-354`; khảo sát AiClient/consumer trong phiên |
| Path policy hiện khá rộng, chưa phải per-program/per-agent isolation hoàn chỉnh | `cells/services/vfs/src/access/rules.rs:8-19,54-58` |

Đây là bounded source recon, không khẳng định toàn repo/out-of-tree tuyệt đối không có feature.
Tài liệu cũ `docs/vfs-api.md:68-85` còn mô tả FAT16 `/data`; không dùng nó để phủ nhận native mounts hiện hành.

## Hạn chế native format cần xác minh trước corpus lớn

| Giới hạn đọc từ implementation hiện tại | Evidence / phép tính |
|---|---|
| Tên entry tối đa 46 byte, không phải 46 Unicode characters | `libs/cellos-fs/src/format.rs:41-42,348-352` |
| Directory inline tối đa 7 entry | payload 428 byte / entry 56 byte; `format.rs:26-42,338-346`; `inode.rs:80-95` |
| Native inode table tối đa 256 slot, bao gồm root | 32 block × 8 inode/block; `format.rs:26-28,61`; create guard `engine.rs:298-303` |
| Directory lookup duyệt tuyến tính | `inode.rs:71-77`; `engine.rs:229-258` |
| Replace file là functional in-place mutation, không phải crash-safe transaction | `engine.rs:369-375` |
| Hai superblock và flush không được source coi là crash-preservation proof | `engine.rs:705-770` |

Các giới hạn trên chưa được reproduce bằng runtime trong nghiên cứu này.
Không áp chúng cho FAT/RamFS/BootFS hoặc mọi VFS backend.
Không lấy mô tả tổng quan “extent B-tree/CoW/crash-resilient” làm bằng chứng bỏ qua exact code path.
Search Cell không thể chữa giới hạn capacity/storage correctness.

## Các ranh giới đang được cân nhắc, chưa chọn

- VFS: authority, file identity/lifetime, source revision/read consistency, generic metadata,
  change notification/feed nếu mọi relevant mutation thực sự đi qua VFS.
- Search Cell: catalog, index, tokenizer/extractor, ranking, snippets, pagination và query subscriptions.
- Query API ở VFS không đồng nghĩa index engine phải chạy trong VFS.
- Async index có thể nằm trong VFS; Cell riêng không tự tạo MMU isolation.
- Native attributes là dữ liệu gốc nếu user/app khai báo; postings/embedding thường là dữ liệu dẫn xuất.
- Thay đổi disk format là một quyết định riêng với chọn engine và vị trí query API.

## Research còn mở

1. [ ] Refresh source/ABI/runtime và locks mới nhất; map mọi native mutation path và writer bypass VFS/guest.
2. [ ] Xác minh capacity/filename/directory limits bằng disposable native volume, nếu người dùng cho phép chạy research scenario.
3. [ ] Chọn corpus/query set đại diện, phần cứng và scale; quyết định ngưỡng freshness/latency/resource trước khi chạy so sánh.
4. [ ] Xác định yêu cầu stable identity, rename/delete/recreate, source revision và coherent ranged read.
5. [ ] So sánh polling/reconciliation với volatile notifications và durable feed; định nghĩa gap/overflow/restart.
6. [ ] So sánh giữ/nâng native backend với backend khác; không tự chọn thay BFS/RedoxFS.
7. [ ] Khảo sát engine compatibility: no_std/std profile, I/O, threads, mmap, unsafe/FFI, license; không mặc định Tantivy/SQLite dùng ngay.
8. [ ] Thiết kế scope/authority của indexer và query caller; kiểm tra tên/snippet/count/event không vượt quyền.
9. [ ] Chỉ đề xuất exact VFS primitive khi đã có requirement hoặc bottleneck tương ứng.

## Chương trình đo đề xuất — chưa được chạy/phê duyệt

- Baseline: scan qua VFS; đối chiếu indexed search với cùng query semantics và corpus.
- Corpus: nhiều file nhỏ, file lớn, thư mục rộng/sâu; code/text trước, tài liệu có extractor nếu cần.
- Đo query p50/p95 cold/warm, result correctness/completeness, IPC count, bytes read/written.
- Đo build/rebuild, incremental update lag, CPU/RAM/index size và ảnh hưởng foreground file I/O.
- Trường hợp rename/delete/recreate, concurrent edit, revoke, backend/indexer restart và queue overflow.
- Host benchmark chỉ chứng minh host engine; không chứng minh performance native Cellos.
- Recovery test chỉ claim đúng failure model đã thực sự exercise; không suy ra physical-power-loss proof.
- Không dùng corpus vượt capacity rồi coi failure là benchmark search.

## Assumptions

- [INFERENCE] Nhiều agent truy vấn lặp trên corpus ít thay đổi có thể hưởng lợi shared index; cần đo.
- [UNVERIFIED] Chưa có engine/backend nào được chứng minh tương thích, nhanh hơn hoặc đủ production cho mục tiêu này.
- [UNDECIDED] Corpus, scale, freshness, resource budget, metadata semantics và snapshot requirements chưa được người dùng chọn.

## Success Criteria của phase research

- [x] Lưu bối cảnh, source pointers, nguồn ngoài và các lựa chọn chưa quyết định.
- [ ] Có workload/requirements được người dùng chấp nhận, không chỉ feature wishlist.
- [ ] Các giới hạn then chốt được xác minh ở evidence ceiling rõ ràng.
- [ ] Có số liệu tái lập nếu người dùng chọn tiếp tục measurement; hoặc ghi rõ quyết định không đo và lý do.
- [ ] Có option matrix đủ dữ kiện để người dùng quyết định; không tự chuyển sang implementation.

## Security Considerations

Index không cấp quyền đọc. Không dùng identity của service để mở rộng scope của query caller.
Re-authorize khi trả kết quả/fetch; xét cả path, snippet, counts và subscription events.
Scope/namespace có thể thay đổi trong vòng đời query; identity và revocation phải có semantics cụ thể.

## Risk Assessment

Research hiện chỉ ghi tài liệu. Runtime research sau này chỉ trên disposable image/volume với scope riêng.
Không format/migrate/repair dữ liệu để làm scenario pass; ghi data effect không thể được undo chỉ bằng revert code.
Nếu giả thuyết sai, sửa research record/option matrix, không thêm shim hoặc đổi filesystem ngầm.

## Deviation Log

- 2026-10-19 — Quan sát mới: native directory/inode/name capacity là vấn đề độc lập với search.
  Impact: không giả định native storage sẵn sàng cho corpus lớn; giữ cả storage và search trong research.
  Decision state: chưa phê duyệt cách nâng cấp; không đổi code hoặc format.
