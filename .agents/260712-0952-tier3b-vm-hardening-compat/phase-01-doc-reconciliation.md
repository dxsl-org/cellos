# Phase 01 — Doc Reconciliation (doc-vs-reality)

**Cửa sổ:** do-now (docs, hợp Mythos analysis-only) · **Priority:** P1 · **Status:** ✅ done (2026-07-12) · **Tier:** fast · **Law 1:** no

## Context Links
- Research §1, §4 finding #4, §"Đề xuất bước tiếp" #1
- Target: `docs/guides/tier3b-linux-vm.md`, `docs/system-architecture.md`, `docs/project-roadmap.md`

## Overview
Doc hiện hứa nhiều hơn code chạy. Sửa để phản ánh đúng hiện trạng — tránh người dùng/agent tin nhầm x86 đã chạy.

## Key Insights
- `tier3b-linux-vm.md:25` ghi x86_64 "✅ Working (G2)" nhưng **0 dòng code** (chỉ plan `.agents/260711-1917`). Đây là sai lệch nghiêm trọng nhất.
- `:13, :218` dùng "apt" — Alpine dùng `apk`, không có apt.
- `:191` ghi "~9K lines" — thực tế ~2.9K LOC đã ship (cell ~1.72K + kernel ~0.66K + stage2 474).
- `:166-174` số hiệu năng (10-20µs trap, 80% I/O, boot 2-5s) chưa có benchmark — phải ghi rõ "ước lượng, chưa đo".
- `:89,:122` rootfs read-only — cần nêu rõ giới hạn persist (P04 sẽ sửa thực tế).

## Requirements
**Functional:**
- x86_64 status: "✅ Working" → "🚧 Planned (plan 260711-1917, chưa code)".
- Mọi "apt" trong ngữ cảnh Alpine → "apk"; nếu nói apt thì gắn với guest Debian (P05).
- LOC number → số thực + ghi chú "kế hoạch ~9K".
- Số hiệu năng → gắn nhãn "ước lượng chưa đo (xem P07 benchmark)".
- Bảng "VirtIO Devices" và "Limits" phản ánh: block read-only hiện tại, writable đang ở P04.

**Non-functional:** không đổi cấu trúc doc; giữ format/anchor cũ để không vỡ link chéo.

## Related Code Files
- Modify: `docs/guides/tier3b-linux-vm.md`, `docs/system-architecture.md` (§Tier 3 nếu có số liệu trùng), `docs/project-roadmap.md` (trạng thái Tier 3b x86).
- Không tạo file mới.

## Implementation Steps
1. Grep toàn repo `docs/` cho "Working (G2)", "apt", "9K", "9,000 lines", "80% native" để tìm mọi chỗ lặp.
2. Sửa `tier3b-linux-vm.md`: platform table, apk, LOC, perf disclaimer, devices/limits table.
3. Đồng bộ `system-architecture.md` §Tier 3 và `project-roadmap.md` (x86 = Planned).
4. Verify link chéo `[system-architecture.md]`, `[cells/guests/...]` còn đúng.

## Todo
- [ ] Grep các chuỗi sai lệch
- [ ] Sửa platform/status table (x86 Planned)
- [ ] apt→apk (ngữ cảnh Alpine)
- [ ] LOC + perf disclaimer
- [ ] Đồng bộ system-architecture.md + roadmap
- [ ] Kiểm link chéo

## Success Criteria
- Không còn chuỗi khẳng định x86 đang chạy trong `docs/`.
- `haily-project-manager` xác nhận doc khớp implementation thực tế.
- Grep "Working (G2)" cho x86 = 0 kết quả.

## Risk Assessment
- Thấp. Rủi ro duy nhất: bỏ sót một chỗ lặp → mitigate bằng grep toàn `docs/` ở bước 1.

## Security Considerations
- Không. Doc-only.

## Next Steps
- Mở khóa nhận thức đúng cho P05 (guest glibc dùng apt là hợp lệ) và P08 (x86).
