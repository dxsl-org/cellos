---
phase: 3
title: "Spike A: sysroot C++ hosted + JSC shell trong Tier 2"
status: pending
priority: P1
effort: "3–6 tuần (độ bất định cao)"
dependencies: [2]
tier: thinking
---

# Phase 03: Spike A — sysroot C++ hosted + JSC shell trong Tier 2

> **Required — deviation-log:** ghi mọi Decision/Deviation/Surprise vào § Deviation Log ngay khi xảy ra.

## Overview
Chỉ chạy khi G1 = go. Mục tiêu là chứng minh lát cắt thật nhỏ nhất của WebKit, gồm WTF + JavaScriptCore (shell `jsc`, JIT tắt), build được bằng sysroot C++ hosted và chạy đúng trong một domain Tier 2 trên **AArch64 QEMU `virt` `-cpu cortex-a76`** (cùng họ CPU với RPi5). Spike này kiểm tra cùng lúc toolchain, libc, bộ nhớ, TLS, thread, FP/SIMD và GC. Bỏ qua đồ họa và mạng. Tier 2 trên AArch64 hiện chỉ admit trong `test-hooks`; spike chạy trong image đó.

## Requirements
- Functional:
  - `jsc` chạy một bộ script cố định trong cell Tier 2, cho output **byte-identical** với `jsc` cùng cấu hình trên host Linux.
  - Bộ script gồm: một tập test262 chọn lọc (≥200 test, danh sách cố định), richards, deltablue và một script cấp phát lớn để ép GC chạy nhiều chu kỳ.
- Non-functional:
  - JIT tắt, W^X giữ nguyên.
  - Không async signal.
  - Mọi delta ở kernel/ABI phải nằm sau feature `serval-spike` và chỉ có trong image `test-hooks`. Không đổi hành vi mặc định của image production.
  - Đo kích thước ELF, peak heap, thời gian chạy và so với QuickJS hiện có.

## Architecture
- Chiến lược theo G1:
  - **S1**: mlibc (đã có sysdeps AArch64, mở rộng phần thiếu) + LLVM libc++/libc++abi/libunwind build bằng clang `--target=aarch64-unknown-none-elf` (hard-float, NEON), với sysroot riêng.
  - **S2**: musl static `aarch64-linux` + lớp dịch syscall tối thiểu trong kernel (chỉ đủ cho `jsc`).

  Phase này viết theo S1. Nếu G1 chọn S2, thay bước 2–3 và ghi deviation.
- Cấu hình JSC: `ENABLE_JIT=OFF`; chọn LLInt hoặc `ENABLE_C_LOOP` theo kết quả Phase 01; `USE_SYSTEM_MALLOC=ON`; tắt Gigacage; `--useConcurrentGC=false --numberOfGCMarkers=1` (hoặc tên option tương đương); chỉ dùng một mutator thread. Như vậy không cần suspend thread (giả định, phải xác minh).
- Delta kernel/ABI dự kiến, mỗi mục cần approval Law 1 riêng:
  1. **Profile ngân sách lớn**, chỉ cho một cell được đặt tên: slot VA và quota heap lớn hơn (ví dụ VA 512 MiB, quota 256 MiB). Chạm `kernel/src/loader/va_alloc.rs` và `kernel/src/memory/cell_quota.rs`.
  2. **Nạp ELF lớn**: không đọc nguyên file vào kernel heap 4 MiB (`kernel/src/fs.rs:20-50`). Spike có thể dùng một phân vùng riêng hoặc nạp theo từng segment.
  3. **TLS tĩnh**: loader truyền vị trí PT_TLS (auxv/phdr) cho libc. libc tự cấp block TLS cho mỗi thread rồi gọi `SetTlsBase` (đã có). Mục tiêu là không thêm syscall mới.
  4. **Anonymous mapping**: spike giả lập `mmap(MAP_PRIVATE|MAP_ANONYMOUS)` trong libc bằng aligned allocation trên heap; `munmap` trả bộ nhớ về heap; không có guard page; `mprotect(PROT_EXEC)` thất bại. Nếu JSC buộc phải có guard page hoặc reserve/decommit thật, ghi đó là gap K và dừng ở mức đo đạc.
  5. **FP/SIMD**: nếu D5 kết luận kernel chưa lưu thanh ghi Q0–Q31/FPCR/FPSR khi context switch task, thêm phần save/restore này (eager trước; lazy để sau). Cell Rust hiện build `aarch64-unknown-none-softfloat` và QuickJS dùng `-mgeneral-regs-only`. Phần JSC hard-float chỉ được giao tiếp với host Rust qua hàm có tham số nguyên/con trỏ để không trộn ABI float.
- Vị trí mã: `cells/tests/jsc-spike/` (theo precedent `cells/tests/cpp-smoke/`). Script dựng sysroot và toolchain CMake C++ đặt ở `scripts/serval-spike/`. Không đưa vào image mặc định.

## Assumptions
- **Claim:** với system malloc, việc cấp phát MarkedBlock/structure heap của JSC hoạt động được bằng `mmap` giả lập trên heap.
  **Confidence:** low. **How to verify:** đọc `OSAllocatorPOSIX.cpp`, `StructureAlignedMemoryAllocator.cpp` và `MarkedBlock.cpp` của 2.54.1; chạy `jsc` trên host với `mmap` bị shim (LD_PRELOAD) theo cùng ngữ nghĩa.
- **Claim:** single mutator + GC không concurrent không cần suspend thread.
  **Confidence:** low–medium. **How to verify:** đã xác minh ở Phase 02. Nếu sai, thêm một delta K: syscall suspend/resume + đọc register của thread cùng domain, cần approval riêng.
- **Claim:** mlibc mở rộng đủ cho libc++ (locale tối thiểu “C”, `pthread`, `clock_gettime`).
  **Confidence:** medium. **How to verify:** build libc++ và chạy test smoke trước khi đụng đến JSC.

## Related Files
- Create: `cells/tests/jsc-spike/` (cell host Rust + `build.rs` link archive JSC/WTF)
- Create: `scripts/serval-spike/build-sysroot.sh`, `scripts/serval-spike/cellos-aarch64-cxx.cmake`
- Create: `tests/integration/tests/jsc-spike.rs` (lane QEMU so output với golden file của host)
- Modify (chỉ sau approval, sau `serval-spike`): `kernel/src/loader/va_alloc.rs`, `kernel/src/memory/cell_quota.rs`, `kernel/src/fs.rs` hoặc loader segment, đường auxv/phdr trong loader, context switch AArch64 trong `hal/arch/arm/src/aarch64/` (FP/SIMD), launch profile cho cell spike
- Modify: sysdeps mlibc (theo `docs/mlibc-build.md`)

## Implementation Steps
1. Soạn packet approval Law 1: liệt kê exact delta 1–5, feature gate, cách tắt, và test chứng minh image mặc định không đổi. **Checkpoint:** chờ approval.
2. Dựng sysroot: mlibc AArch64 → libunwind → libc++abi → libc++ (exceptions + RTTI bật, threads qua pthread của mlibc). Smoke C++ trong Tier 2: exceptions, RTTI, `thread_local`, `std::thread` + `std::mutex` + `std::condition_variable`, `std::chrono`, `std::string`/`std::vector`/`std::unordered_map`, cùng một test hai thread làm phép tính `double`/NEON xen kẽ (bắt lỗi FP bị hỏng khi context switch). Smoke này phải pass trước khi build JSC.
3. Viết toolchain CMake C++ (C và CXX, `CMAKE_SYSTEM_NAME Generic`, link tĩnh bằng clang + linker script của cell). Mục tiêu: build `libJavaScriptCore.a` + `libWTF.a` từ tarball 2.54.1 với cấu hình JSC-only. Ghi mọi patch upstream vào `cells/tests/jsc-spike/PATCHES.md`, một dòng cho mỗi patch, kèm lý do.
4. Cell host: entry Rust gọi `jsc` main với argv cố định, đọc script từ VIFS1/VFS, in kết quả ra serial.
5. Lane integration: boot image `test-hooks` + `serval-spike`, chạy toàn bộ bộ script, so khớp byte với golden output sinh từ host. Kiểm tra log `[domain] admitted cell 'jsc-spike' to Tier 2`.
6. Đo: kích thước ELF (`size -A`), peak heap (kernel quota counter), số thread, thời gian mỗi benchmark trên QEMU, và bảng so sánh với QuickJS (`cells/services/ocel-quickjs`).
7. Kiểm tra hồi quy: build và chạy lane mặc định (`ocel-browser`, `ocel-quickjs`, `tier2-fault-isolation`, `launch-profile`) trên image **không có** `serval-spike` để chứng minh không thay đổi.
8. Ghi `evidence/spike-a-jsc.md`: kết quả, số đo, patch list, các gap K đã gặp và gap nào còn mở.

## Success Criteria
- [ ] Smoke C++ hosted pass trong Tier 2 (exceptions, RTTI, `thread_local`, `std::thread`).
- [ ] `jsc` chạy trong cell được admit Tier 2. Output khớp golden với ≥200 test262 + richards + deltablue + GC stress.
- [ ] Có số đo kích thước/heap/thời gian, so sánh với QuickJS.
- [ ] Lane mặc định pass trên image không có `serval-spike`.
- [ ] `PATCHES.md` liệt kê mọi thay đổi upstream.

## Security Considerations
Cell spike là UNTRUSTED Tier 2, `CapSet::EMPTY`, không có network/block_io/spawn. Profile ngân sách lớn chỉ áp cho đúng tên cell đó trong image `test-hooks`. Không mở đường cho cell khác xin ngân sách tùy ý.

## Risk Notes
- Rủi ro lớn nhất: dựng sysroot C++ hosted có thể tự nó đã tốn hàng tuần. Nếu sau 2 tuần smoke C++ ở bước 2 vẫn chưa pass, dừng lại và báo cáo, không ép tiếp.
- CLoop chậm hơn LLInt nhiều. Kết quả hiệu năng chỉ là cận dưới.
- Rollback: tắt feature `serval-spike` là trở lại hành vi cũ. Delta kernel nằm sau `cfg` nên có thể revert theo commit. Patch upstream nằm trong tree spike, không lan ra ngoài.

## Deviation Log
