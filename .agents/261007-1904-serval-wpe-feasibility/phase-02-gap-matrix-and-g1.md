---
phase: 2
title: "Kiểm D4–D5, gap matrix, cost model, gate G1"
status: pending
priority: P1
effort: "2–3 ngày"
dependencies: [1]
tier: thinking
---

# Phase 02: Kiểm D4–D5, gap matrix, cost model, gate G1

> **Required — deviation-log:** ghi mọi Decision/Deviation/Surprise vào § Deviation Log ngay khi xảy ra.

## Overview
Phase này làm hai việc:
- Kết luận hai điều kiện dừng sớm còn lại bằng cách đọc mã:
  - **D4:** GC của JSC chạy được mà không cần async signal;
  - **D5:** kernel lưu/khôi phục FP/SIMD của task user.
- Đối chiếu từng nhu cầu đo được ở Phase 01 với năng lực Tier 2 trong [scout-report.md](scout-report.md), ước lượng chi phí, rồi để người dùng quyết định G1.

Phase này chỉ tạo tài liệu, không sửa code.

## Requirements
- Functional:
  - Ma trận nhu cầu → năng lực Cellos (có / một phần / thiếu), dẫn chứng `file:line`.
  - Mỗi gap được phân loại: **U** (userland: libc, sysroot, port lib), **K** (kernel/ABI, cần Law 1), **A** (mâu thuẫn ADR, cần ADR mới).
  - Mỗi gap có ước lượng effort, owner đề xuất, và có chặn spike hay không.
- Non-functional: không làm tròn để đẹp số liệu. Ghi rõ khoảng bất định. Phải so sánh với phương án Tier 3.

## Architecture

### Các nhóm gap bắt buộc phải có trong ma trận (từ scout; Phase 01 bổ sung số đo)
| Nhóm | Hiện trạng | Loại dự kiến |
|---|---|---|
| VA/heap mỗi domain (32 MiB slot, quota 16 MiB) | thiếu | K |
| Anonymous mmap/munmap, guard page, decommit | thiếu | K + A (ADR-0018) |
| PT_TLS, `__thread`/`thread_local` | thiếu | K + U |
| Sysroot C++20 hosted (libc++/libc++abi/libunwind, exceptions, RTTI, locale) | thiếu | U |
| libc đủ rộng (pthread đầy đủ, clock/nanosleep, poll, dirent, locale, time zone) | một phần | U (+K nếu cần poll) |
| Cơ chế dừng thread cho GC (thay SIGUSR1) | thiếu | K + sửa WTF |
| Đa process WebKit: spawn cell con, shm giữa các domain, truyền handle | một phần (RV64; AArch64 test-hooks; x86_64 đóng) | K + A |
| Tier 2 production trên arch mục tiêu: admission AArch64/x86_64 hiện chỉ test-hooks (`kernel/src/loader/domain_admission.rs:149-170`) | thiếu | K |
| x86_64: POSIX shim, C++ ABI, lifecycle grant domain (`libs/api/src/services/posix.rs:11-17`; `kernel/src/task/syscall.rs:233-281`) | thiếu | K + U |
| Lưu FP/SIMD của task user (D5) | chưa thấy (grep) | K |
| Nạp ELF lớn (VIFS1 đọc nguyên file vào heap 4 MiB; P6 32 MiB) | thiếu | K + build |
| Hiển thị Tier 2 → compositor | thiếu (mixed grant bị chặn) | K |
| Font/text: FreeType, HarfBuzz, fontconfig hoặc thay thế, ICU data | thiếu | U |
| Mạng: libsoup trên net-service IPC, TLS, HTTP/2 | một phần | U (+K nếu cần poll/readiness) |
| Đồ họa: Skia CPU, có/không cần EGL (theo Phase 01) | thiếu | U hoặc U+K |
| Input: IME, clipboard, touch | thiếu | K (service) |

### Quyết định chiến lược substrate (chọn tại G1)
- **S1 — Native Cellos libc**: sysdeps của mlibc (hoặc libc mới) trên syscall Cellos + LLVM libc++. Hợp triết lý Cellos, nhưng phải tự viết lại hàng trăm hàm POSIX và hành vi biên.
- **S2 — Linux-ABI personality cho Tier 2**: binary static musl `aarch64-linux`/`x86_64-linux`, kernel dịch syscall Linux bên trong domain (kiểu linuxulator/gVisor-lite). Port WPE gần như nguyên bản, nhưng chính là đưa một lớp tương thích Linux vào kernel. Hướng này mâu thuẫn mạnh với ADR-0018 và với định vị “không tương thích ngược” trong README.
- **S3 — Không port Tier 2**: WPE trong Tier 3 guest, Serval là frontend native nhận frame. Đây là mốc so sánh chi phí bắt buộc, không phải phương án ngầm định.

Ma trận phải có ước lượng effort cho cả S1, S2 và S3, để G1 so sánh thật.

### Gate G1 (người dùng chốt ngưỡng trước khi chấm)
- **D1–D3** (từ Phase 01) và **D4–D5** (từ phase này): điều kiện nào trượt mà không có lối thoát chấp nhận được → **dừng sớm**. Lối thoát cần ADR (bật JIT, `dlopen`, async signal) chỉ được trình bày, không mặc định chọn.
- **K1 footprint**: peak RSS tổng của C1 trên corpus, cộng ngân sách cho Cellos và một guest Tier 3 (≥256 MiB theo ADR-0017), phải vừa RAM của RPi5 và laptop i7-8565U. Dung lượng RAM thực tế lấy từ người dùng. Đây là chỉ tiêu cần đo, không phải lý do dừng.
- **K2 kích thước**: image AArch64 của C1 có con số ước lượng, kèm kế hoạch nạp và đóng gói (P6 hiện chỉ 32 MiB). Không có con số thì không go.
- **K3 chi phí**: tổng person-month của S1 hoặc S2 (substrate + port engine + mở Tier 2 production trên arch mục tiêu) đặt cạnh S3. Go chỉ khi người dùng chấp nhận mức chênh lệch.

## Assumptions
- **Claim:** WTF có backend suspend thread không dùng signal, có thể tham chiếu để viết bản cho Cellos (Darwin dùng `thread_suspend`, Windows dùng `SuspendThread`).
  **Confidence:** medium. **How to verify:** đọc `Source/WTF/wtf/Threading*.cpp` và `ThreadingPOSIX.cpp` trong tarball.
- **Claim:** với một mutator thread và GC không concurrent/không parallel, JSC không cần suspend thread khác.
  **Confidence:** low–medium. **How to verify:** đọc `MachineStackMarker.cpp` (`MachineThreads::gatherConservativeRoots`) và các option `useConcurrentGC`, `numberOfGCMarkers`.
- **Claim (D5):** context switch của task trên AArch64/x86_64/RV64 không lưu thanh ghi FP/SIMD. Grep `stp q`/`fxsave`/`xsave`/`fsd` chỉ thấy ở đường vCPU (`hal/arch/arm/src/aarch64/vcpu.rs:566-751`). Cell x86 build `x86_64-unknown-none` (soft-float).
  **Confidence:** medium. **How to verify:** đọc đường trap/context switch của từng arch trong `hal/arch/*` và `kernel/src/task/`; viết một kịch bản suy luận gồm hai thread dùng FP xen kẽ (chỉ đọc mã, chưa chạy). Kết luận D5 = đạt / gap K (thêm lazy hoặc eager FP save) / phá bất biến.

## Related Files
- Create: `.agents/261007-1904-serval-wpe-feasibility/evidence/serval-gap-matrix.md`
- Create: `.agents/261007-1904-serval-wpe-feasibility/evidence/serval-license-inventory.md` (license từng dependency; nghĩa vụ relink của LGPL khi link tĩnh)
- Modify: `plan.md` (cập nhật trạng thái và kết quả G1)

## Implementation Steps
1. Lấy từng dòng số đo của Phase 01 và gán vào ma trận với dẫn chứng Cellos. Dòng nào không có số đo thì ghi “chưa đo”, không suy diễn.
2. **D4:** đọc `ThreadingPOSIX.cpp`, `MachineStackMarker.cpp`, và các option GC của JSC 2.54.1. Kết luận một trong ba: (a) single mutator thì không cần suspend; (b) cần một syscall suspend/resume + đọc register đồng bộ trong cùng domain (gap K, không phá ADR-0018); (c) bắt buộc phải có async signal (phá ADR-0018 → điều kiện dừng). Đồng thời đọc StructureID heap, `OSAllocator`, `PageReservation` để biết gap K nào né được bằng cấu hình.
3. **D5:** đọc context switch/trap của AArch64, x86_64, RV64 theo hướng dẫn ở Assumptions. Ghi kết luận và ước lượng chi phí thêm FP/SIMD save (eager hoặc lazy).
4. Ước lượng effort cho S1, S2, S3 theo từng nhóm gap. Ghi khoảng (tối thiểu–tối đa) và căn cứ, ví dụ so với port QuickJS/MuPDF đã làm.
5. Lập inventory license.
6. Trình bày G1 cho người dùng: bảng D1–D5 (đạt/trượt + lối thoát), K1–K3, phương án đề xuất, rủi ro. **Checkpoint:** chờ người dùng chọn go (S1 hoặc S2) hoặc dừng. Không tự chọn thay.
7. Ghi kết quả G1 vào `plan.md`. Dừng thì nhảy thẳng Phase 05 để ghi ADR từ chối.

## Success Criteria
- [ ] Mọi nhóm gap trong bảng trên có dòng trong ma trận, có loại U/K/A và effort.
- [ ] Mỗi dòng K hoặc A nêu chính xác file/ABI/ADR bị chạm.
- [ ] Có ước lượng S1, S2, S3 cạnh nhau.
- [ ] D4 và D5 có kết luận đạt / gap K / dừng, kèm dẫn chứng mã.
- [ ] Người dùng đã quyết định G1 và quyết định được ghi lại.

## Security Considerations
Ghi rõ hệ quả bảo mật của từng chiến lược. S2 mở bề mặt tấn công lớn trong kernel (hàng trăm syscall Linux). Shared memory nhiều bên và spawn cell con từ một domain untrusted phải giữ nguyên quy tắc launch edge đã review và không cấp quyền thiết bị/DMA.

## Risk Notes
- Ước lượng effort cho WebKit thường bị thấp hơn thực tế. Áp dụng hệ số bất định và nói rõ đã áp dụng.
- Rollback: chỉ là tài liệu. Kết luận G1 có thể xem lại khi có số đo mới.

## Deviation Log
