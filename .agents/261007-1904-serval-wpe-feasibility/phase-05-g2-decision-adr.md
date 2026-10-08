---
phase: 5
title: "Gate G2 và ADR Serval"
status: pending
priority: P1
effort: "2–3 ngày"
dependencies: [3, 4]
tier: thinking
---

# Phase 05: Gate G2 và ADR Serval

> **Required — deviation-log:** ghi mọi Decision/Deviation/Surprise vào § Deviation Log ngay khi xảy ra.

## Overview
Tổng hợp evidence của Phase 01–04 (hoặc chỉ 01–02 nếu G1 = no-go) thành một quyết định kiến trúc chính thức: Serval chạy ở tier nào, theo chiến lược substrate nào, và còn những gate nào trước khi port engine. Kết quả là ADR-0022 cùng các cập nhật tham chiếu đi kèm.

## Requirements
- Functional:
  - ADR-0022 “Serval — tier và engine của trình duyệt đầy đủ” ở trạng thái **Accepted** hoặc **Rejected**.
  - ADR phải nêu rõ quan hệ với ADR-0017 (§2.2–§4, §5c) và ADR-0018 (JIT, mmap, signal, dynamic linking): thay thế, bổ sung, hay giữ nguyên.
- Non-functional:
  - Mỗi lập luận trỏ về file evidence.
  - Không tuyên bố tương thích web hay hiệu năng vượt quá những gì spike đã đo.

## Architecture
Nội dung ADR-0022:
1. Bối cảnh: yêu cầu Serval, mâu thuẫn với ADR-0017/0018, và định vị trong README (Tier 3 cho trình duyệt).
2. Số đo: footprint WPE (Phase 01), gap matrix + chi phí S1/S2/S3 (Phase 02), kết quả spike A/B (Phase 03/04).
3. Quyết định: một trong ba hướng:
   - Tier 2 S1;
   - Tier 2 S2;
   - giữ Tier 3 (S3), với Serval là frontend native.
4. Hệ quả: danh sách gap K/A phải đóng trước khi port; bo mục tiêu tối thiểu (theo K1); những gì bị loại (JIT, media, WebGL); và nghĩa vụ license.
5. Gate tiếp theo: nếu go, tạo một kế hoạch port riêng (`$hc-plan`). Kế hoạch đó bắt đầu từ substrate, chưa đụng tới UI.

Nếu ADR được accept theo hướng Tier 2, cập nhật các tài liệu đang mô tả trình duyệt đầy đủ chỉ nằm ở Tier 3. Trước khi sửa, grep để có danh sách chính xác:
- `docs/decisions/0017-...` (ghi chú superseded-by);
- `README.md`, `README_VN.md` (mục Tier 3);
- `docs/app-development-guide.md` (bảng tier).

## Assumptions
- **Claim:** số ADR tiếp theo là 0022 (hiện có đến `0021-actor-supervisor-library-in-userspace.md`).
  **Confidence:** high. **How to verify:** `ls docs/decisions` trước khi tạo file.

## Related Files
- Create: `docs/decisions/0022-serval-browser-tier-and-engine.md`
- Modify (chỉ khi ADR thay đổi hướng): `docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md`, `README.md`, `README_VN.md`, `docs/app-development-guide.md`
- Modify: `plan.md` (trạng thái cuối)
- Modify: `CHANGELOG.md` chỉ khi có delta code từ Phase 03/04 được giữ lại

## Implementation Steps
1. Gom bảng kết quả: K1–K5 (G1), spike A, spike B và các gap còn mở.
2. Viết bản nháp ADR-0022 theo cấu trúc trên, kèm link tới evidence.
3. **Checkpoint G2:** trình bày ADR nháp cho người dùng và chờ người dùng chọn Accepted/Rejected cùng hướng đi.
4. Hoàn tất ADR và cập nhật các tài liệu tham chiếu nếu hướng đi thay đổi. Kiểm tra mọi link nội bộ còn tồn tại.
5. Delta spike: nếu G2 = no-go thì quyết định với người dùng việc giữ delta presentation (có giá trị cho Ocel PDF) hay revert. Delta `serval-spike` của Phase 03 bị revert nếu không có kế hoạch port.
6. Nếu go: in lệnh `$hc-plan` cho kế hoạch port Serval, với đầu vào là ADR-0022 và gap matrix.

## Success Criteria
- [ ] ADR-0022 tồn tại ở trạng thái Accepted hoặc Rejected, có link tới evidence.
- [ ] ADR-0017 và README/app guide nhất quán với ADR-0022.
- [ ] Số phận của mọi delta spike (giữ/revert) đã được quyết định và thực hiện.

## Security Considerations
ADR phải ghi rõ bề mặt tấn công mới nếu chọn Tier 2: grant gate được nới, profile ngân sách lớn, lớp tương thích syscall (S2), và engine web untrusted chạy trong domain.

## Risk Notes
- Có thể ADR được accept theo hướng ngược với định vị trong README. Khi đó phải sửa README/README_VN ngay trong cùng thay đổi, không để tài liệu mâu thuẫn nhau.
- Rollback: ADR là tài liệu. Muốn đổi hướng sau này thì viết ADR mới để supersede.

## Deviation Log
