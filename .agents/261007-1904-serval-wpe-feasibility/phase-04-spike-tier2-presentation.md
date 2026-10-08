---
phase: 4
title: "Spike B: hiển thị pixel từ Tier 2"
status: pending
priority: P1
effort: "1–2 tuần"
dependencies: [2]
tier: thinking
---

# Phase 04: Spike B — hiển thị pixel từ domain Tier 2

> **Required — deviation-log:** ghi mọi Decision/Deviation/Surprise vào § Deviation Log ngay khi xảy ra.

## Overview
Chỉ chạy khi G1 = go. Hiện cell Tier 2 không đưa được frame lên màn hình: `ViSurface` cần `GrantShare` từ app sang compositor SAS (`libs/ostd/src/display/surface.rs:52-83`), mà kernel từ chối grant giữa SAS và domain theo cả hai chiều (`kernel/src/task/grant_gate_selftest.rs:255-269`). Ocel PDF đã vướng đúng lỗi này (`docs/guides/ocel-viewer.md:269-271`). Serval là trình duyệt raster CPU nên cần một đường frame + input cho Tier 2 có đo hiệu năng. Phase này chạy độc lập với Phase 03.

## Requirements
- Functional:
  - Một cell test Tier 2 (UNTRUSTED, `CapSet::EMPTY`) tạo surface, vẽ, gửi damage, và nhận sự kiện phím/chuột khi được focus.
  - Compositor hiển thị đúng pixel.
  - Đóng/crash cell thì surface và grant được thu hồi.
- Non-functional:
  - Không cho domain ghi vào bộ nhớ SAS. Compositor không được ghi vào bộ nhớ domain ngoài vùng đã grant.
  - Đo frame/s cho cả damage toàn khung 1280×720 BGRA (≈3.5 MiB) và damage nhỏ, trên AArch64 QEMU (`-cpu cortex-a76`).

## Architecture
Ba phương án. Chọn một bằng approval Law 1 trước khi sửa:
- **P-a: domain → compositor, chỉ đọc.** Nới grant gate cho đúng một trường hợp: chủ private-root share **read-only** cho receiver là compositor đã đăng ký service. Compositor map vùng đó vào SAS ở chế độ chỉ đọc. Đây là delta nhỏ nhất và gỡ được cả Ocel PDF. Rủi ro: dữ liệu của domain hiện ra trong không gian SAS (chỉ đọc). Domain có thể đổi pixel trong lúc compositor đang đọc (tearing), nhưng không làm hỏng compositor nếu compositor không tin vào nội dung pixel.
- **P-b: compositor → domain.** Compositor cấp buffer, share ghi cho domain. Hướng này để domain ghi vào trang của SAS. Bị loại trừ khi có lý do mạnh.
- **P-c: copy qua broker.** Domain gửi pixel bằng grant domain–domain 64 KiB đến một broker, broker copy sang surface SAS. Không cần nới gate, nhưng mỗi khung đầy cần khoảng 57 lần chuyển grant. Chỉ chọn nếu số đo cho thấy đủ nhanh.

Khuyến nghị **P-a** vì có giá trị chung cho Ocel PDF. Quyết định cuối thuộc approval.

Input: compositor hiện route sự kiện theo owner của surface (`cells/services/compositor/src/input_handler.rs:126-140`). Cần xác minh việc gửi sự kiện tới owner là domain có đi qua copied IPC hay không.

## Assumptions
- **Claim:** compositor không cần ghi vào buffer của app, chỉ đọc để blend.
  **Confidence:** high (`cells/services/compositor/src/render.rs:8-62`). **How to verify:** đọc lại `render.rs` và đường damage.
- **Claim:** RV64 có lifecycle grant domain đầy đủ; AArch64 chỉ có trong test-hooks; x86_64 đóng (`kernel/src/task/syscall.rs:233-281`).
  **Confidence:** high. Spike cam kết AArch64 (image test-hooks, arch mục tiêu) và RV64 (để kiểm lại Ocel PDF, vốn chỉ build cho RV64). x86_64 ghi vào gap matrix.
- **Claim:** việc gửi sự kiện input tới owner là domain hoạt động qua IPC typed hiện có.
  **Confidence:** medium. **How to verify:** test ở bước 5.

## Related Files
- Create: `cells/tests/tier2-surface/` (cell test Tier 2 vẽ pattern có thể kiểm tra)
- Create: `tests/integration/tests/tier2-surface.rs`
- Modify (sau approval): `kernel/src/task/syscall.rs` (grant gate), `kernel/src/task/grant_gate_selftest.rs` (ca mới: được phép/bị từ chối), `libs/ostd/src/display/surface.rs` nếu cần cờ read-only, compositor nếu cách map thay đổi
- Modify: `docs/guides/ocel-viewer.md` (gỡ ghi chú lỗi PDF nếu P-a gỡ được)

## Implementation Steps
1. Đọc lại grant gate, compositor attach và đường input. Soạn packet approval cho P-a (hoặc P-c), gồm exact delta, ma trận allow/deny, và hành vi khi revoke/crash. **Checkpoint:** chờ approval.
2. Triển khai delta. Thêm ca self-test:
   - domain → compositor read-only: cho phép;
   - domain → compositor read-write: từ chối;
   - domain → cell SAS không phải compositor: từ chối;
   - SAS → domain: vẫn từ chối.
3. Cell `tier2-surface` vẽ pattern có checksum theo từng frame (ô màu + số frame), damage toàn khung và damage từng ô, rồi log `[tier2-surface] frame N`.
4. Lane integration:
   - log admit Tier 2;
   - chụp frame bằng screendump của QEMU và kiểm tra pixel ở 4 vùng;
   - gửi phím/chuột qua monitor QEMU và kiểm tra cell log sự kiện;
   - kill cell, chứng minh surface biến mất và grant được thu hồi.
5. Đo frame/s cho damage toàn khung và damage nhỏ trên AArch64 QEMU, ghi rõ đây chỉ là bằng chứng QEMU.
6. Chạy lại Ocel PDF trên image Tier 2 RV64 (MuPDF chỉ build cho RV64). Nếu PDF đã hiển thị được, cập nhật guide và CHANGELOG. Nếu chưa, ghi lý do.
7. Hồi quy: `ocel-browser`, `desktop-shell`, `window-policy`, `tier2-fault-isolation`.
8. Ghi `evidence/spike-b-presentation.md`.

## Success Criteria
- [ ] Self-test grant gate pass đủ 4 ca allow/deny.
- [ ] Lane `tier2-surface` pass: pixel đúng, input tới đúng cell, kill thì thu hồi được.
- [ ] Có số frame/s cho 1280×720 trên AArch64 QEMU; lane `tier2-surface` pass trên cả AArch64 và RV64.
- [ ] Các lane hồi quy hiển thị/window pass.

## Security Considerations
Chỉ nới đúng một cặp (domain owner → compositor đã đăng ký service, read-only). Compositor phải coi pixel là dữ liệu không tin cậy: không parse, chỉ blend trong giới hạn kích thước của surface. Revoke phải unmap phía compositor trước khi trả trang lại cho domain.

## Risk Notes
- Nếu P-a bị từ chối vì lý do LBI, P-c có thể quá chậm cho trình duyệt. Khi đó G2 phải ghi đây là blocker.
- Rollback: delta grant gate nhỏ, có self-test, revert được theo commit. Cell và lane test độc lập với phần còn lại.

## Deviation Log
