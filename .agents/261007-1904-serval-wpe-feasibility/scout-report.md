# Scout report — Serval (WPE WebKit trên Tier 2)

Thu thập 2026-10-07 bằng 3 scout chỉ đọc (PosixRuntime, BuildToolchain, PlatformServices) và nguồn upstream. Mọi dòng có `file:line` là sự thật đã đọc; `[INFERENCE]` là suy luận chưa đo.

## 1. Upstream WPE (tháng 10/2026)
- Bản ổn định hiện tại: **2.54.1**; 2.54.0 ra 2026-09-16 (https://wpewebkit.org/release/wpewebkit-2.54.0.html).
- 2.54: WPEPlatform là API nhúng ổn định, bật mặc định, có sẵn backend Wayland, DRM/KMS và **headless**; không còn cần libwpe/WPEBackend-fdo/Cog (https://wpewebkit.org/about/get-wpe.html).
- 2.54: compositor của Web Process chuyển từ TextureMapper sang **Skia**, bỏ hẳn Cairo. Từ 2.46 Skia là backend 2D, có raster CPU.
- Kiến trúc WebKit2 luôn đa tiến trình: UIProcess, WebProcess, NetworkProcess (+GPUProcess tùy chọn). Đường frame của API mới hiện **chỉ hỗ trợ buffer DMA-BUF** (https://docs.webkit.org/Ports/WebKitGTK%20and%20WPE%20WebKit/Graphics.html). Đường non-accelerated SHM có trong WebKitGTK; trên WPE headless/CPU thì chưa kiểm chứng `[INFERENCE]`.
- JSC trên Linux dùng **SIGUSR1** để suspend/resume thread khi GC (https://bugs.webkit.org/show_bug.cgi?id=220641).
- Yêu cầu toolchain: GCC/Clang mới và libstdc++ của Debian/Ubuntu LTS còn được hỗ trợ (https://docs.webkit.org/Ports/WebKitGTK%20and%20WPE%20WebKit/GCCRequirement.html).

## 2. Runtime Tier 2 hiện có
| Năng lực | Trạng thái | Bằng chứng |
|---|---|---|
| Admission domain riêng | RV64 mở; AArch64/x86_64 chỉ có ở image `test-hooks` | `kernel/src/loader/domain_admission.rs:149-170,185-209` |
| Thread cùng address space | Có, tối đa 32 thread/cell | `kernel/src/task/scheduler.rs:782-905` |
| pthread C | Tập con: create/join/mutex/condvar, tối đa 16 thread; không có TLS key/rwlock/detach | `libs/port-platform/include/cellos_pthread.h:2-17,40-58` |
| `__thread`/`thread_local` | Chưa có (loader chưa xử lý PT_TLS); chỉ có syscall `SetTlsBase` | `libs/api/src/abi/syscall.rs:160-191` |
| Futex | Có, key theo domain | `kernel/src/task/futex.rs:1-12` |
| Signals | Không có; `_kill` trả -1; ADR-0018 từ chối async signal | `libs/api/src/services/posix/sysio.rs:685-692`; `docs/decisions/0018-cell-native-portability-and-runtime-profiles.md:31-34,154-173` |
| mmap/munmap/mprotect | Không có; `mmap` trả NULL | `libs/api/src/services/posix/sysio.rs:177-196` |
| Heap / VA mỗi cell | Quota 16 MiB; slot VA 32 MiB | `kernel/src/memory/cell_quota.rs:26-31`; `kernel/src/loader/va_alloc.rs:1-8,37-57` |
| W^X | Bắt buộc; loader từ chối W+X, user không thể đổi quyền trang | `kernel/src/loader/wx.rs:70-119,131-197` |
| Dynamic loading | Không có; chỉ static PIE + RELATIVE reloc | `kernel/src/loader/reloc.rs:1-34`; ADR-0018:27-34 |
| Spawn cell con | Cần SpawnCap + launch edge đã review; không có fork/exec | `kernel/src/task/cap.rs:207-227`; `kernel/src/loader/launch_profile/mod.rs:1-5` |
| Shared memory giữa hai domain | Một grantee; RV64; AArch64 chỉ test-hooks; x86_64 đóng | `kernel/src/task/syscall.rs:233-281,2967-3034` |
| IPC | Message typed 4 KiB; grant VFS 64 KiB; không truyền fd | `libs/api/src/services/ipc.rs:1-32` |
| Socket | TCP client AF_INET qua net service; `send` ≤495 B/lần; không poll/epoll/eventfd | `libs/api/src/services/posix/net.rs:100-347` |
| Nạp ELF từ VIFS1 | Đọc nguyên file vào kernel heap 4 MiB | `kernel/src/fs.rs:20-50`; `kernel/src/main.rs:632-655` |

## 3. Toolchain
- C: QuickJS (RV64/AArch64) và MuPDF (chỉ RV64) build bằng `build.rs` + `cc`, freestanding, link tĩnh vào cell Rust (`cells/services/ocel-quickjs/build.rs:56-142`; `cells/services/ocel-pdf/build.rs:5-62`).
- C++: chỉ có `cpp-freestanding`: `-fno-exceptions -fno-rtti -fno-threadsafe-statics`, không có header chuẩn (`<cstdint>` cũng lỗi), không STL, không x86_64 (`cells/tests/cpp-smoke/build.rs:20-85`; `docs/guides/tier1b-c-zig.md:264-290`).
- CMake/Meson: chỉ C, chỉ compile; `tools/cellos-cc` từ chối link (`tools/cellos-cc:1-27`; `cmake/cellos-riscv64.cmake:1-12`).
- mlibc: build riêng, chỉ RV64/AArch64, arena bump 4 MiB, `AnonFree` no-op, SHA provenance `TBD` (`docs/mlibc-build.md:108-113,190-194`).
- Đóng gói: phân vùng cell-store P6 tổng **32 MiB** trên RV64 và AArch64 (`scripts/gen-disk-ci.sh:397-425,536-552`; `scripts/format-disk-arm.sh:80-111`).

## 4. Dịch vụ nền
- Hiển thị: chỉ có ViSurface (grant CPU BGRA) → compositor SAS → VirtIO-GPU 2D (`libs/ostd/src/display/surface.rs:52-123`; `cells/drivers/virtio-gpu/src/display.rs:126-168`). Domain Tier 2 **không share grant được sang compositor SAS** (`kernel/src/task/grant_gate_selftest.rs:255-269`); Ocel PDF đã gặp đúng lỗi này (`docs/guides/ocel-viewer.md:269-271`).
- GPU: không có EGL/GLES/Vulkan/virgl; `libs/viui` GLES2 mới là khung, các lệnh vẽ đều no-op (`libs/viui/src/gles2_canvas.rs:2-10`).
- Input: phím, chuột, cuộn; không có touch/IME/clipboard (`libs/api/src/services/input.rs:63-154`).
- Font: Ocel dùng font bitmap 8×8; không có FreeType/HarfBuzz/fontconfig/ICU dùng chung.
- Mạng: socket IPC typed, DNS và TLS chạy trong net service; TLS từng lỗi handshake với server thật (`cells/apps/hypha/llm-gateway/src/main.rs:40-45`).
- Âm thanh: ABI có `AudioPlay` nhưng kernel trả lỗi vì chưa có Sound Cell (`kernel/src/task/syscall.rs:6739-6751`).

## 5. Ràng buộc kiến trúc
- ADR-0017 §2.2–§4: trình duyệt đầy đủ chạy trong **Tier 3 VM**; §5c tách “trình duyệt native đầy đủ” thành app tương lai, chưa chốt engine/tier, và từ chối thêm JSC chỉ vì tương thích (`docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md:126-177,301-344`).
- ADR-0018 xếp JIT, dynamic linking, `mmap(MAP_SHARED)` và async signal vào lãnh thổ Tier 3 (`docs/decisions/0018-...:58-82,154-173`).
- Bo mạch: RPi3 có 948 MiB; VisionFive 2 không có driver display/GPU/mạng; RK3588 và x86 chưa có số liệu qualify (`docs/roadmap/hardware-tracks.md:26-89`; `boards/starfive/visionfive-2/board.rs:6-57`).

## 6. Kết luận scout
Muốn chạy WPE trên Tier 2 phải xây gần như một personality POSIX/Linux bên trong Tier 2. Các phần thiếu gồm: VA/heap lớn, anonymous mapping, PT_TLS, sysroot C++ hosted, cơ chế dừng thread cho GC, đa cell có shared memory/handle, đường hiển thị Tier 2 và stack font/text. Chưa có hạng mục nào trong số này, và một số hạng mục mâu thuẫn trực tiếp với ADR-0017/0018. Vì vậy cần **đo trước rồi mới port**.
