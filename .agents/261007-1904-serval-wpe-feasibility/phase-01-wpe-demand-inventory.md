---
phase: 1
title: "Đo WPE trên Linux + kiểm D1–D3"
status: pending
priority: P1
effort: "4–6 ngày (mỗi lần build WPE mất vài giờ)"
dependencies: []
tier: medium
---

# Phase 01: Đo WPE trên Linux + kiểm D1–D3

> **Required — deviation-log:** ghi mọi Decision/Deviation/Surprise vào § Deviation Log ngay khi xảy ra.

## Overview
Build WPE WebKit 2.54.1 với cấu hình gần nhất với những gì Tier 2 có thể cung cấp, chạy trên Linux, rồi kết luận ba điều kiện dừng sớm:
- **D1:** render headless chỉ bằng CPU, không cần GPU/EGL.
- **D2:** build tĩnh, không cần `dlopen`.
- **D3:** với JIT tắt, trang thật vẫn dùng được.

Đồng thời đo nhu cầu tài nguyên. Phase này không chạm repo Cellos ngoài thư mục evidence. Đây là bước rẻ nhất để biết có phải dừng sớm hay không.

## Requirements
- Functional: tạo bảng số đo tái lập được cho một corpus trang cố định:
  - kích thước từng thư viện và binary;
  - peak RSS mỗi process;
  - VA đã reserve;
  - số thread và số process;
  - tập syscall và tần suất;
  - signal được dùng;
  - mẫu mmap/mprotect (có W→X không);
  - socket/fd/shm;
  - symbol import từ libc/libstdc++/glib.
- Functional: kết luận **đạt/trượt** cho D1, D2, D3, mỗi kết luận kèm lệnh tái lập và output.
- Non-functional: ghi lại mọi version, hash tarball, cờ CMake và lệnh chạy. Không tải dữ liệu mạng lúc đo; corpus phải chạy offline.

## Architecture
- Nguồn: `wpewebkit-2.54.1.tar.xz` (verify sha256 từ trang release), Skia đi kèm trong source tree.
- Môi trường build: container Debian trixie, hoặc WebKit Container SDK chạy trên Docker có sẵn của host. Host có 27 core, 31 GiB RAM, còn trống 767 GiB. Host đang thiếu `gperf`, `ruby`, `strace` nên phải cài trong container.
- Hai cấu hình đo:
  - **C0 baseline**: mặc định của WPE + `ENABLE_WPE_PLATFORM_HEADLESS`. Dùng làm điểm tham chiếu.
  - **C1 Tier-2-shaped**: JIT tắt (LLInt hoặc CLoop), `USE_SYSTEM_MALLOC=ON` (bỏ bmalloc/libpas, tắt Gigacage), tắt video/audio/WebRTC/WebGL/WebXR/gamepad/spellcheck/introspection/sandbox bubblewrap, chỉ giữ platform headless.
  - Tên option chính xác lấy từ `Source/cmake/OptionsWPE.cmake` và `WebKitFeatures.cmake` của tarball, không đoán.
- Arch: x86_64 native trên host để lặp nhanh; host cũng là đại diện tốt cho i7-8565U. Thêm một build **aarch64 cross** cho C1 (đại diện RPi5) để đo `.text`/`.rodata`/`.data`. Nếu cross build kẹt quá 1 ngày, ghi deviation và ước lượng bằng hệ số kích thước đo trên JSC đơn lẻ.
- **Cấu hình C2 (chỉ dùng cho D2)**: C1 + build tĩnh (`BUILD_SHARED_LIBS=OFF` hoặc option tương đương của WebKit), GIO module TLS của glib-networking build tĩnh. Nếu WebKit/glib-networking không hỗ trợ build tĩnh, ghi chính xác chỗ bắt buộc phải có `dlopen`.
- **Thước đo D3**: chạy Speedometer 3 và JetStream 2 (bản offline cố định, cùng revision) trên host với C0 (JIT bật) và C1 (JIT tắt, LLInt; thêm CLoop nếu C1 dùng CLoop). Đo thêm thời gian tải-đến-tương-tác của trang 3 và 6 trong corpus. Quy đổi theo tỷ lệ hiệu năng đơn luồng giữa host và i7-8565U (ghi nguồn số liệu tham chiếu), và ghi là ước lượng.
- **Thước đo D1b (hiệu năng khi chỉ dùng CPU)**: đo **trên chính i7-8565U** chạy Linux (live USB), cùng một bản WPE C0, một lần GPU bật (Mesa iris) và một lần ép chạy hoàn toàn bằng CPU. Đo ở 1920×1080:
  - MotionMark 1.3 (đồ họa: canvas, CSS transform, SVG);
  - FPS khi cuộn trang 5 và 6 (đếm frame trình bày, không tính frame bị bỏ);
  - FPS một animation CSS `transform`/`opacity` và một `filter: blur`;
  - Speedometer 3 (JS/DOM, đối chứng: GPU gần như không ảnh hưởng);
  - CPU% trung bình trong 30 s cuộn.

  Nếu có RPi5 thì lặp lại trên RPi5 (Mesa v3d). Máy chưa sẵn sàng thì ghi deviation, chạy trên host, và không quy đổi sang máy mục tiêu.
- Driver đo: một chương trình C nhỏ dùng WPEPlatform headless + WebKit API. Nó tải từng URL `file://`, đợi `load-changed`/`FINISHED`, chụp snapshot (API snapshot có từ 2.52), chờ 5 s idle rồi thoát. Không dùng MiniBrowser vì nó kéo thêm UI.
- Corpus gồm 6 trang, đặt trong `evidence/corpus/`:
  1. HTML tĩnh;
  2. CSS flex/grid + webfont;
  3. trang JS-heavy (bản offline cố định của TodoMVC hoặc tương đương);
  4. trang 2D canvas;
  5. một bài viết Wikipedia đã lưu offline;
  6. một ứng dụng SPA lớn đã lưu offline.

  Ghi license/nguồn của từng trang.

## Assumptions
- **Claim:** WPE 2.54 headless render được bằng Skia CPU mà không cần GPU/EGL/DMA-BUF.
  **Confidence:** medium — tài liệu Graphics nói API mới hiện chỉ hỗ trợ DMA-BUF.
  **How to verify (D1):** chạy C1 trong container không có `/dev/dri`, không có Mesa/libEGL trong image (`ldd` không được kéo libEGL/libgbm). Nếu build hoặc chạy bắt buộc cần EGL/GBM: ghi lỗi chính xác, thử lại với Mesa softpipe (không LLVM) và đo chi phí. Kết luận D1 = “trượt, lối thoát = port Mesa softpipe” kèm kích thước và RSS của Mesa.
- **Claim:** WebKit và glib-networking hỗ trợ build tĩnh, và GIO TLS backend có thể đăng ký mà không cần `dlopen`.
  **Confidence:** low–medium. **How to verify (D2):** đọc `meson_options.txt` của glib-networking và `OptionsWPE.cmake`, build C2, rồi `strace -e trace=openat` để chứng minh không có `.so` nào được mở lúc chạy.
- **Claim:** có thể tắt JIT bằng option CMake mà vẫn chạy được (LLInt hoặc CLoop).
  **Confidence:** high. **How to verify:** đọc `OptionsWPE.cmake`, chạy `jsc` của C1 với `--useJIT=false` để đối chiếu.
- **Claim:** với system malloc, JSC không reserve vùng VA khổng lồ (Gigacage tắt). Structure heap vẫn có thể reserve một vùng lớn.
  **Confidence:** medium. **How to verify:** cộng dồn `/proc/<pid>/maps` theo loại vùng ngay sau khi khởi động và sau khi load xong corpus.
- **Claim:** IPC của WebKit trên Unix dùng socketpair + `SCM_RIGHTS` để chuyển handle shared memory.
  **Confidence:** high. **How to verify:** `strace -f -e trace=sendmsg,recvmsg,memfd_create` khi tải trang 1.

## Related Files
- Create: `.agents/261007-1904-serval-wpe-feasibility/evidence/serval-demand-inventory.md`
- Create: `.agents/261007-1904-serval-wpe-feasibility/evidence/corpus/` (6 trang + `SOURCES.md`)
- Create: `.agents/261007-1904-serval-wpe-feasibility/evidence/harness/` (driver C, Dockerfile, script đo)
- Không sửa file nào trong repo Cellos.

## Implementation Steps
1. Viết Dockerfile (Debian trixie, clang, cmake, ninja, gperf, ruby, perl, python3, strace, ltrace, các dev package của glib/libsoup3/ICU/harfbuzz/freetype/fontconfig/libxml2/sqlite/libjpeg/png/webp). Ghi digest của image.
2. Tải tarball, verify sha256, đọc `OptionsWPE.cmake` để lấy đúng tên option cho C0 và C1. Lưu lệnh `cmake` đầy đủ vào evidence.
3. Build C0 và C1 (x86_64). Ghi thời gian build, dung lượng build tree và kích thước sau khi strip của `libWPEWebKit-2.0.so`, `libJavaScriptCore`, WebProcess, NetworkProcess và từng thư viện phụ thuộc (`ldd` closure).
4. Viết driver headless và chạy corpus. Mỗi trang chạy 3 lần, ghi trung vị:
   - peak RSS (`/proc/<pid>/status` VmHWM) cho UI/Web/Network process;
   - VA reserved (tổng các vùng trong `maps`, tách theo loại);
   - số thread tối đa mỗi process (`/proc/<pid>/task`);
   - thời gian từ lúc bắt đầu tải đến lúc có snapshot.
5. `strace -f -c` và `strace -f -e trace=%signal,%memory,%ipc,%net` cho trang 1, 3 và 6. Xuất các bảng:
   - syscall → số lần gọi;
   - signal được cài/gửi;
   - mmap theo kích thước và cờ;
   - mprotect có PROT_EXEC hay không;
   - sendmsg có attachment hay không.
6. `nm -D --undefined-only` trên mọi `.so` của C1 để lập bảng symbol import từ libc, libstdc++, libgcc, libm, pthread và glib/gio. Đếm số symbol duy nhất theo nhóm (thread, time, file, socket, locale, dlopen, signal, memory).
7. Cross build aarch64 cho C1 (hoặc chỉ JSC + WTF nếu build toàn bộ kẹt). Ghi `size -A` của `.text`/`.rodata`/`.data`.
8. **D1:** chạy corpus với C1 trong container không GPU/EGL như mô tả ở Assumptions. Ghi đạt/trượt.
9. **D2:** build C2, chạy trang 1 và 5 với `strace -e trace=openat`. Ghi mọi `.so`/module được nạp. Ghi đạt/trượt.
10. **D3:** chạy Speedometer 3 và JetStream 2 với C0 và C1 (3 lần mỗi bản, lấy trung vị), cộng với thời gian tải trang 3 và 6. Bảng hóa tỷ lệ JIT-tắt/JIT-bật. Ngưỡng đạt đề xuất (người dùng chốt trước khi chấm): trang 3 và 6 tương tác được trong ≤3 s trên host quy đổi sang i7-8565U, và điểm Speedometer khi tắt JIT ≥25% điểm khi bật JIT. Ghi đạt/trượt.
10b. **D1b:** chạy bộ đo D1b trên i7-8565U (và RPi5 nếu có). Lập bảng GPU so với CPU cho từng thước đo. Ngưỡng đạt đề xuất (người dùng chốt trước khi đo): ở chế độ CPU, cuộn trang 5–6 đạt ≥30 fps và animation `transform` đạt ≥30 fps ở 1080p. Ghi đạt/trượt và các nhóm nội dung bị chậm nhiều nhất.
11. Viết `serval-demand-inventory.md`: kết luận D1–D3 nằm ở đầu file, sau đó là bảng số đo, cấu hình, lệnh tái lập, các giả định đã xác nhận/bác bỏ, và khác biệt giữa C0, C1, C2.

## Success Criteria
- [ ] C0 và C1 build thành công, có hash tarball, digest image và lệnh `cmake` trong evidence.
- [ ] Đủ 6 trang × 3 lần chạy, mỗi lần có snapshot PNG (nhìn ra được nội dung, không trắng/đen toàn bộ).
- [ ] Có bảng RSS/VA/thread/process cho từng process, và tổng của cả C0 lẫn C1.
- [ ] Có danh sách syscall, signal và nhóm symbol import của C1.
- [ ] Có kích thước AArch64 (`.text`+`.rodata`+`.data`) của C1, hoặc của JSC kèm hệ số ước lượng.
- [ ] D1, D2, D3 mỗi điều kiện có kết luận đạt/trượt, kèm output tái lập. Điều kiện nào trượt thì nêu lối thoát và chi phí của lối thoát.
- [ ] D1b có bảng GPU so với CPU đo trên i7-8565U (hoặc ghi rõ lý do chỉ đo được trên host).

## Security Considerations
Corpus phải offline. Không chạy trang lạ có kết nối mạng trong container có quyền host. Container không mount `$HOME` và không cần quyền privileged (strace dùng `--cap-add=SYS_PTRACE`).

## Risk Notes
- D1 có thể trượt: tài liệu Graphics nói API mới hiện chỉ dùng buffer DMA-BUF. Khi đó lối thoát là Mesa softpipe trong Tier 2. Phải định lượng chi phí này, không né.
- Ngưỡng D3 là đề xuất. Người dùng chốt trước khi đo, không chỉnh sau khi đã thấy kết quả.
- Số đo trên x86_64 Linux là cận dưới cho logic, không phản ánh chi phí Cellos IPC.
- Rollback: phase này chỉ tạo file evidence; xóa thư mục là hoàn tác hoàn toàn.

## Deviation Log
