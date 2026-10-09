# Nghiên cứu: Đánh giá Sử dụng Trình biên dịch Zig (`zig cc`/`zig c++`) để Biên dịch Chéo và Nhúng C/C++ vào Cellos (Rust)

**Ngày thực hiện:** 2026-10-09  
**Phương pháp:** hl-research (`eval`), đối chiếu mã nguồn thực tế tại kho Cellos, tài liệu chính thức Zig/Rust/Cargo, và chạy thực nghiệm biên dịch trực tiếp (empirical compiler probes).  
**Môi trường thực nghiệm:** WSL2 Linux x86_64, Windows Zig 0.13.0 (Clang 18.1.6 bên trong), kiểm thử trên 3 kiến trúc đích: `riscv64-freestanding-none`, `aarch64-freestanding-none`, `x86_64-freestanding-none`.

---

## 1. Kết luận tóm tắt (Verdict)

> **CÓ THỂ SỬ DỤNG, NHƯNG CHỈ NÊN DÙNG LÀM "OBJECT COMPILER" CÓ CHỌN LỌC (SCOPED C/C++ BACKEND), KHÔNG THỂ THAY THẾ DROP-IN CHO TOÀN BỘ TOOLCHAIN HOẶC RUNTIME.**

1. **Điểm mạnh xác thực:**
   - `zig cc` và `zig c++` đóng gói sẵn Clang + lld trong 1 file nhị phân duy nhất, hỗ trợ biên dịch chéo ra ELF64 cho cả 3 kiến trúc của Cellos (RV64, AArch64, x86_64) mà không cần cài đặt 3 bộ cross-gcc khác nhau (`gcc-riscv64-unknown-elf`, `gcc-aarch64-linux-gnu`, `g++`).
   - Kết quả thực nghiệm: File C++ `engine.cpp` của `cpp-smoke` biên dịch thành công ra ELF64 với đầy đủ cờ ABI chuẩn (`lp64d`, `double-float ABI`, `RVC`), và file C `lapi.c` của Lua biên dịch sạch với header shim hiện có của Cellos.
2. **Những rào cản và cạm bẫy kỹ thuật (Load-bearing Gotchas):**
   - **Zig KHÔNG cung cấp libc hay libc++ cho target freestanding:** Zig chỉ ship libc (glibc, musl) cho các hệ điều hành hosted (Linux, BSD, Windows). Trên `*-freestanding-none`, `#include <stdlib.h>` hay `#include <vector>` đều lập tức báo lỗi `file not found`. Trách nhiệm cung cấp C/C++ runtime vẫn thuộc 100% về Cellos (`libs/api/src/services/posix/{alloc.rs, cxxabi.rs, sysio.rs}` hoặc `mlibc`).
   - **Xung đột cờ dòng lệnh (CLI Flag Incompatibility):** Driver `zig cc` từ chối cờ chuẩn GCC `-march=rv64gc` (báo `error: unknown CPU: 'rv64gc'`). Zig yêu cầu cú pháp `-mcpu=generic_rv64+m+a+f+d+c`. Rất nhiều `build.rs` trong Cellos hiện tại đang hardcode `-march=rv64gc`.
   - **Không thay thế được `libclang-dev` cho bindgen:** Crate `littlefs2-sys` dùng `bindgen` để sinh binding cấu trúc LFS; `bindgen` đòi hỏi thư viện động `libclang.so` (`libclang-dev`), file nhị phân độc lập `zig.exe`/`zig` không thay thế được.
   - **An toàn bộ nhớ (Safety boundary):** Việc dùng Zig biên dịch C/C++ **không** làm mã nguồn đó tự động an toàn hơn trong Rust. Mã C/C++ nhúng vào Rust Cell vẫn là foreign unsafe code; trong Tier 1 SAS nếu có lỗi con trỏ vẫn có thể làm hỏng bộ nhớ chung. Muốn cô lập an toàn phần cứng, Cellos vẫn phải đưa sang Tier 2 Paged Domain (CR3).

---

## 2. Bảng so sánh ma trận (Comparison Matrix)

| Tiêu chí | Toolchain hiện tại (GCC + Clang phân tán) | Thay bằng Zig toolchain (`zig cc` / `zig c++`) | Ghi chú & Đánh giá |
|---|---|---|---|
| **Cài đặt & Thiết lập máy dev** | Phải cài riêng: `gcc-riscv64-unknown-elf`, `gcc-aarch64-linux-gnu`, `clang`, `libclang-dev`. | Chỉ cần 1 binary `zig` cho C/C++ (nhưng vẫn cần `libclang-dev` cho bindgen). | **Zig thắng về độ tiện lợi đóng gói toolchain.** |
| **Hỗ trợ Target Freestanding (RV64, AArch64, x86_64)** | Đầy đủ, đã được kiểm chứng trên toàn bộ CI và QEMU. | Đầy đủ; thực nghiệm xác nhận ELF64 sinh ra đúng flags `0x5, RVC, double-float ABI` (`lp64d`). | **Ngang nhau về chất lượng mã ELF sinh ra.** |
| **Khả năng tương thích cờ build (`-march=rv64gc`)** | Tương thích tuyệt đối với cờ GCC/Clang hiện có. | **Không tương thích trực tiếp:** báo lỗi `unknown CPU: 'rv64gc'`. Phải đổi sang `-mcpu=generic_rv64+m+a+f+d+c`. | **Cần wrapper script để dịch cờ.** |
| **C Standard Library (libc)** | Phụ thuộc vào header trần hoặc xpack newlib / picolibc / mlibc. | Tương tự: Zig **không có libc** cho target `*-freestanding-none`. | **Cả hai đều cần POSIX shim / mlibc của Cellos.** |
| **C++ Standard Library (libc++ / libstdc++)** | Không có (bị chặn chủ động bởi `docs/guides/tier1b-c-zig.md`). | Không có (`fatal error: 'vector' file not found`). | **Cả hai đều chỉ chạy được subset `cpp-freestanding`.** |
| **Tích hợp với Cargo / `cc` crate** | Mặc định: `cc` crate tự nhận diện GCC/Clang qua `CC_<target>` trong `.cargo/config.toml`. | Cần wrapper script vì `cc` crate (1.4.0) thực thi trực tiếp binary, không hỗ trợ truyền thêm cờ `-target` nếu không bọc. | **Hiện tại GCC/Clang mượt mà hơn với Cargo.** |
| **Tương thích Windows / WSL2** | Phụ thuộc vào môi trường chạy. | Nếu dùng Zig Windows (`/mnt/c/zig/zig.exe`) từ WSL sẽ bị lỗi `BadPathName` với đường dẫn tuyệt đối Linux. Cần Zig Linux native. | **Lưu ý môi trường WSL.** |

---

## 3. Bằng chứng thực nghiệm (Empirical Probes Evidence)

Các lệnh thực nghiệm được chạy trực tiếp tại `/home/dmin/cellos/target/zig-probe`:

### Probe 1: Khả năng sinh mã Freestanding ELF64
```bash
# RV64 lp64d
zig cc -c test_c.c -o test_c_rv64.o -target riscv64-freestanding-none -mabi=lp64d -mcpu=generic_rv64+m+a+f+d+c -fPIC -ffreestanding
# Kết quả readelf:
# Class: ELF64, Machine: RISC-V, Flags: 0x5, RVC, double-float ABI (khớp 100% với rust-lld)

# AArch64
zig cc -c test_c.c -o test_c_aarch64.o -target aarch64-freestanding-none -fPIC -ffreestanding
# Kết quả readelf: Class: ELF64, Machine: AArch64

# x86_64
zig cc -c test_c.c -o test_c_x86.o -target x86_64-freestanding-none -fPIC -ffreestanding -mno-red-zone
# Kết quả readelf: Class: ELF64, Machine: Advanced Micro Devices X86-64
```

### Probe 2: Biên dịch C++ Freestanding thực tế (`cells/tests/cpp-smoke/cpp/engine.cpp`)
```bash
zig c++ -c cells/tests/cpp-smoke/cpp/engine.cpp -o engine_rv64.o \
  -target riscv64-freestanding-none -mabi=lp64d -mcpu=generic_rv64+m+a+f+d+c \
  -fPIC -ffreestanding -fno-exceptions -fno-rtti -fno-threadsafe-statics -fno-use-cxa-atexit \
  -Icells/tests/cpp-smoke/cpp
```
- **Kết quả:** Biên dịch thành công 100%.
- **Biểu tượng chưa phân giải (`nm -u engine_rv64.o`):**
  - `_ZdaPv`, `_ZdlPv`, `_Znam`, `_Znwm` (operator new/delete)
  - `__cxa_pure_virtual`, `atexit`
  - `open`, `close`, `read`
  - Không có bất kỳ biểu tượng nào của `libstdc++` hay unwinding (`__cxa_throw`, `_Unwind_*`). Tất cả đều khớp đúng với các hàm ABI do `libs/api/src/services/posix/{alloc.rs, cxxabi.rs, sysio.rs}` cung cấp.

### Probe 3: Header chuẩn C/C++ trên target Freestanding
- Thử nghiệm `#include <vector>`: Lỗi `fatal error: 'vector' file not found`.
- Thử nghiệm `#include <stdlib.h>`: Lỗi `fatal error: 'stdlib.h' file not found`.
- Thử nghiệm builtin headers (`<stdint.h>`, `<stddef.h>`, `<stdarg.h>`, `<limits.h>`): Thành công.
- Thử nghiệm mã Lua thật (`cells/runtimes/lua/src/c/src/lapi.c`) kèm `third_party/freestanding-include`: Thành công.

### Probe 4: Xung đột cờ `-march=rv64gc`
```bash
zig cc -c test_c.c -target riscv64-freestanding-none -march=rv64gc
# Kết quả: error: unknown CPU: 'rv64gc'
```

---

## 4. Phân tích tác động theo từng tầng kiến trúc của Cellos

### 4.1. Tầng Cargo & Build Scripts (`build.rs`)
- Hiện tại nhiều crate (`cells/demos/doom`, `cells/demos/tetris-c`, `cells/services/ocel-pdf`, `cells/runtimes/lua`) có logic cứng trong `build.rs`:
  - `build.flag("-march=rv64gc")`
  - `build.compiler("riscv-none-elf-gcc")` khi không có `CC_<target>`
  - `cpp-smoke/build.rs` kiểm tra cứng lệnh `have("riscv64-unknown-elf-g++")` và `have("clang++")`, không tôn trọng biến `CXX_<target>`.
- Nếu chuyển sang Zig, các `build.rs` này sẽ bị gãy nếu không có một lớp wrapper giả lập interface của gcc/clang.

### 4.2. Tầng Runtime & Liên kết (Linker & CRT)
- Quy trình liên kết Cellos hiện tại:
  1. C/C++ compiler sinh file `.o` (hoặc `.a`).
  2. Cargo gọi `rustc` và `rust-lld`.
  3. `libs/cell-build` phát sinh linker script `cell.ld` (`-pie`, ET_DYN).
  4. `libs/ostd/src/startup.rs` cung cấp `_start`, khởi tạo `__init_array` (static constructors cho C++).
- **Điểm mấu chốt:** Zig không nên tham gia vào khâu liên kết cuối cùng (`final link`). Khâu này bắt buộc phải do `rust-lld` và `cell-build` đảm nhận để giữ đúng layout bộ nhớ và header ELF Cellos. Zig chỉ nên đóng vai trò là trình biên dịch mã nguồn C/C++ thành object files.

### 4.3. Tầng An toàn bộ nhớ & Phân cấp ứng dụng (Trust & Application Tiers)
- Việc dùng `zig cc` hoàn toàn không thay đổi bản chất an toàn bộ nhớ:
  - **Tier 1 (Rust SAS):** Nếu nhúng code C/C++ vào cell Rust, code C/C++ vẫn chạy unconstrained trong không gian địa chỉ chung. Lỗi tràn bộ nhớ trong C/C++ vẫn có thể phá vỡ tính toàn vẹn của Cellos.
  - **Tier 2 (Paged Domain):** Các cell C/C++ độc lập (như `cpp-smoke`) được kernel phân tách bằng CR3 (trang bảng riêng biệt), giao tiếp qua IPC. Đây mới là nơi cách ly an toàn thực sự, độc lập với việc dùng compiler nào.

---

## 5. Lộ trình khuyến nghị (Ranked Recommendations)

### Khuyến nghị 1: Không thay thế ồ ạt toàn bộ toolchain ngay lập tức (Do Not Big-Bang Migrate)
Việc thay thế toàn bộ `.cargo/config.toml` và các file `build.rs` sang Zig sẽ làm gãy các dependency nhạy cảm (như `littlefs2-sys` + `bindgen`, `ocel-pdf` với MuPDF GNU C11 extensions, Lua với header xpack).

### Khuyến nghị 2: Triển khai theo mô hình "Opt-in Toolchain Wrapper" (Khả thi & An toàn nhất)
Tương tự như script `tools/cellos-cc` đã tồn tại cho CMake, Cellos có thể xây dựng một wrapper script `tools/cellos-zig-cc` và `tools/cellos-zig-cxx`:
1. Nhận các tham số tiêu chuẩn từ `cc::Build`.
2. Tự động chuyển đổi `-march=rv64gc` thành `-mcpu=generic_rv64+m+a+f+d+c` và `-target riscv64-freestanding-none`.
3. Bổ sung `-Ithird_party/freestanding-include` mặc định cho các target không có libc.
4. Cho phép cấu hình qua biến môi trường (ví dụ `CELLOS_USE_ZIG=1`).

### Khuyến nghị 3: Thí điểm trên các Cell C/C++ độc lập trước
- **Thí điểm cấp độ 1:** Các cell kiểm thử nhỏ: `cells/tests/c-pthread`, `cells/demos/tetris-c`.
- **Thí điểm cấp độ 2:** `cells/tests/cpp-smoke` (cần sửa `build.rs` để ưu tiên `CXX_<target>` thay vì hardcode tên compiler).
- **Giữ nguyên:** `littlefs2-sys`, `mlibc`, và các cell sản xuất cốt lõi cho đến khi pipeline CI của wrapper Zig được chứng minh ổn định 100%.

---

## 6. Rủi ro còn tồn đọng (Remaining Risks & Open Questions)

1. **Sự phụ thuộc vào phiên bản Zig:** Zig trước 1.0 (hiện là 0.13 - 0.14) vẫn có sự thay đổi về CLI và target triple giữa các minor release.
2. **Hỗ trợ Thread-Local Storage (TLS):** C/C++ `__thread` / `thread_local` chưa được hỗ trợ trong Cellos do chưa có TLS segment allocator cho userspace. Dùng Zig cũng không giải quyết được vấn đề này cho đến khi hạ tầng runtime TLS được hoàn thiện.
3. **Môi trường WSL2 vs Host Windows:** Các lập trình viên dùng Windows + WSL2 cần cài đặt bản `zig` dành cho Linux (`x86_64-linux`) bên trong WSL thay vì gọi sang `/mnt/c/zig/zig.exe` để tránh lỗi đường dẫn Unix.
