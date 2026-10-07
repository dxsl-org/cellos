# Cellos

[![CI](https://github.com/dxsl-org/cellos/actions/workflows/ci.yml/badge.svg)](https://github.com/dxsl-org/cellos/actions/workflows/ci.yml)
[![Ko-fi](https://img.shields.io/badge/Ko--fi-Donate-%23FF5E5B?logo=ko-fi)](https://ko-fi.com/dxsl_org)
[🌐 English](./README.md)

**Hệ điều hành Rust-native thế hệ mới, được thiết kế chuyên biệt cho hệ thống nhúng, RTOS, robot và các máy chủ/PC chuyên dụng.** 

Thay vì tổ chức phần mềm thành các process cồng kềnh truyền thống, Cellos kiến trúc hệ thống bằng các **Cell**. Các Cell chia sẻ chung một không gian bộ nhớ (Single Address Space) và được cô lập hoàn toàn bởi hệ thống kiểu (type system) mạnh mẽ của Rust, mang lại hiệu suất tối đa mà không đánh đổi sự an toàn.

---

## ✨ Điểm khác biệt của Cellos (So với OS truyền thống)

Thay vì đi theo lối mòn của hạt nhân nguyên khối (Monolithic) hay vi hạt nhân (Microkernel) truyền thống, Cellos mang lại:

*   **Không gian bộ nhớ chung (Cellular Single Address Space - SAS):** Loại bỏ độ trễ chuyển đổi ngữ cảnh bộ nhớ (MMU context switch) đắt đỏ đối với các thành phần tin cậy. Các Cell nội bộ giao tiếp với độ trễ gần bằng 0 (Zero-copy IPC) nhờ cơ chế chuyển giao quyền sở hữu (ownership transfer) trực tiếp của Rust.
*   **Cô lập bằng ngôn ngữ (Language-Based Isolation - LBI):** Sự an toàn không đến từ các ranh giới phần cứng cồng kềnh, mà đến từ hệ thống kiểu (type system) khắt khe của Rust (`#![forbid(unsafe_code)]`). Mọi vi phạm bộ nhớ bị phát hiện và chặn đứng ngay từ lúc biên dịch.
*   **Khôi phục trạng thái tức thì (Instant-On & Heap Snapshot):** Tích hợp sẵn khả năng snapshot và restore trạng thái bộ nhớ heap, giúp hệ thống khởi động với tốc độ chớp nhoáng và phục hồi nhanh chóng cho các thiết bị nhúng.
*   **Kiến trúc lai 3 Tầng (3-Tier Hybrid Architecture):** Vận hành linh hoạt dưới cùng một bộ vi lập lịch (micro-scheduler): Chạy mã Native tin cậy tuyệt đối (Tier 1), nhốt mã nguồn kém an toàn vào phân trang phần cứng MMU (Tier 2), hoặc chạy toàn bộ một hệ điều hành khách như Linux trong máy ảo phần cứng (Tier 3).

---

## 🎯 Tầm nhìn & Định vị: Cellos là gì (và không là gì)?

Cellos ra đời với một mục tiêu rõ ràng: **Hiệu năng và độ tin cậy cho phần cứng xác định.** Dự án không chạy đua để trở thành một hệ điều hành đa dụng.

*   ✅ **Sinh ra cho phần cứng chuyên biệt:** Cellos tỏa sáng trên hệ thống nhúng, robot, server chạy các dịch vụ cốt lõi, hoặc PC dạng kiosk/appliance làm một nhiệm vụ cụ thể.
*   ✅ **Tương lai của RTOS & Độ trễ thấp:** Tập trung vào kiểm soát tài nguyên chặt chẽ và thời gian thực, quản lý bởi vi hạt nhân (nano-kernel) cực nhẹ.
*   ❌ **Không phải bản thay thế Linux/Windows:** Chúng tôi không cố gắng tạo ra một desktop OS để chạy mọi phần mềm phổ thông hay hỗ trợ mọi loại chuột/bàn phím trên thị trường.
*   ❌ **Không ôm đồm di sản quá khứ:** Cellos từ chối việc phình to mã nguồn để tương thích ngược với hàng ngàn thiết bị cũ. Phần cứng được hỗ trợ là một bản hợp đồng chặt chẽ: từ board mạch, vi điều khiển đến firmware. (QEMU chạy được không có nghĩa là máy vật lý đã được chứng nhận — xem [Chính sách phần cứng](./docs/hardware-compatibility-list.md)).

### Mô hình 3 Tier (Ranh giới thực thi)
Rust-native là linh hồn của dự án, nhưng Cellos đủ thực tế để xử lý các nhu cầu phức tạp thông qua kiến trúc đa tầng:
1.  **Tier 1 (Lõi & Native Cell):** Tốc độ tối đa trong không gian bộ nhớ chung (SAS). Tuyệt đối tin cậy.
2.  **Tier 2 (Paged Domain Cell):** Chạy code Native trong miền phân trang bộ nhớ riêng (MMU) để cô lập các phần mềm cần ranh giới phần cứng khắt khe (C-FFI, code chưa được xác thực).
3.  **Tier 3 (VM Guest - Lối thoát hiểm):** Chạy toàn bộ Guest OS (như Linux) bên trong máy ảo. **Đây không phải là mục tiêu chính của Cellos**, mà chỉ là giải pháp đặc thù để chạy một trình duyệt web đầy đủ hoặc các ứng dụng cũ bắt buộc phải có `fork()`/JIT. Xem [Quyết định về trình duyệt](./docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md) và [Hướng dẫn Guest](./docs/guides/tier3b-linux-vm.md).

---

## 🚀 Trạng thái dự án: `v0.2.1-dev` (Mycelium)

Dự án đang trong giai đoạn phát triển tích cực: **G1 — Robot & Embedded** (Tập trung vào ARM64/RV64 SBC và RV32 MCU).
Giai đoạn **G2 — Server & Specialized PC** sẽ mở rộng cho hệ thống đa nhân và thiết bị x86_64 mạnh mẽ.

| Nền tảng | Trạng thái | Ghi chú |
|--------|--------|-------|
| `riscv64gc-unknown-none-elf` | ✅ **Primary** | Hỗ trợ boot hoàn chỉnh và đầy đủ các dịch vụ nền tảng. |
| `aarch64-unknown-none` | ✅ Boot | Đã vào đến scheduler; chuẩn bị bring-up toàn diện cho G1. |
| `x86_64-unknown-none` | ✅ Boot | Hoàn thành cổng chuyển CPL3 trên QEMU q35. (Xem [Tài liệu q35](./boards/qemu/q35-x86_64/README.md)). Chưa có máy tính vật lý x86 nào chính thức được xác nhận. |
| `riscv32imc-unknown-none-elf`| ✅ Boot | Cellos-Nano · Đã xác minh boot S-mode trên QEMU. |

*Lưu ý:* Việc chạy thành công trên máy ảo QEMU là minh chứng kiến trúc, không phải là cam kết hoạt động 100% trên một board vật lý chưa qua tinh chỉnh.

---

## ⚡ Bắt đầu trong 5 phút

Để build Cellos, bạn cần: **Rust nightly**, `qemu-system-riscv64`, và Python 3/PowerShell.

```powershell
# 1. Clone mã nguồn
git clone https://github.com/dxsl-org/cellos.git
cd cellos

# 2. Build kernel (Rust xử lý cờ PIC tự động, đừng cấu hình PIC toàn cầu)
cargo build --release

# 3. Tạo disk image FAT32 và khởi chạy trên QEMU
./gen_disk.ps1
./run.ps1        # Dùng Ctrl+A X để thoát QEMU
```
*Giao diện dòng lệnh của Cellos sẽ xuất hiện. Thử gõ `ls /bin`, `date`, `cat /proc/version`!*

---

## 🧩 Cấu trúc mã nguồn & Kiến trúc

```text
Cellos/
├── kernel/             Nano-kernel: Lập lịch, bộ nhớ, VFS, nạp Cell, IPC
├── hal/                Abstraction layer & kiến trúc phần cứng (RISC-V, ARM, x86)
├── boards/             Định danh board mạch, hợp đồng firmware và thiết lập phần cứng
├── libs/               Thư viện dùng chung: ABI, ostd, ViUI, HTTP
├── cells/              Phần mềm phân tán: apps, demos, drivers, tools, services
├── tests/integration/  Kiểm thử tích hợp (Host-driven & QEMU)
└── docs/               Đặc tả thiết kế (Design specs) & hướng dẫn phát triển
```

Trước khi đóng góp hoặc phát triển, vui lòng xem bản thiết kế hệ thống tại [system-architecture.md](./docs/system-architecture.md) và mô hình bảo mật tại [security-model.md](./docs/security-model.md).

---

## ⚖️ 8 Đạo luật Lập trình của Cellos

Để duy trì tầm nhìn khắc nghiệt về an toàn bộ nhớ và thiết kế modular, toàn bộ codebase phải tuân thủ nghiêm ngặt:

1. **Interface is Sacred:** Đổi `libs/api/` cần sự đồng thuận cao (2x review).
2. **Owned Buffers for Async:** Luôn dùng `Box<[u8]>` thay vì mượn `&mut [u8]` cho dữ liệu qua ranh giới IPC/async.
3. **Multi-Architecture:** Dùng kiểu dữ liệu `VAddr`/`PAddr`, không hardcode kích thước pointer.
4. **Unsafe Management:** Các Cell bị cấm tuyệt đối `unsafe` (`#![forbid(unsafe_code)]`). Kernel nếu dùng phải có ghi chú `// SAFETY:`.
5. **Modern Module Style:** Dùng `foo.rs` và thư mục `foo/`. Cấm file `mod.rs`.
6. **Cellos Naming:** Trait và Type dùng tiền tố `Vi` (Virtual Interface, ví dụ: `ViDriver`). Tên file viết thường `snake_case`.
7. **Trait Objects:** Ở ranh giới hệ thống, dùng đa hình tĩnh qua `Arc<dyn ViDriver + Send + Sync>`.
8. **RAII - Clean Up Explicitly:** Các Cell tự chịu trách nhiệm dọn dẹp tài nguyên (Drop). Không có process-based cleanup do bản chất chia sẻ bộ nhớ SAS.

👉 Đọc chi tiết tại [CONTRIBUTING.md](./CONTRIBUTING.md) và [code-standards.md](./docs/code-standards.md).

---

## 📚 Kho tài liệu

Cellos có hệ thống đặc tả và kiến trúc minh bạch. Trước khi bắt tay vào một phân hệ mới, hãy tìm đọc tài liệu tương ứng:

*   **Bắt đầu:** [getting-started.md](./docs/getting-started.md) | [project-roadmap.md](./docs/project-roadmap.md)
*   **Kiến trúc:** [system-architecture.md](./docs/system-architecture.md) | [hardware-dev-guide.md](./docs/hardware-dev-guide.md)
*   **Đặc tả hệ thống (Specs):** Từ [00-context.md](./docs/specs/00-context.md) đến bộ nhớ ([02-memory.md](./docs/specs/02-memory.md)), ứng dụng tầng ([05-application.md](./docs/specs/05-application.md)), mạng, VFS... (Xem trong thư mục `docs/specs/`).

---

**Đóng góp ý tưởng hoặc mã nguồn?** Chạy `cargo clippy -- -D warnings` và `cargo test --all` trước khi tạo PR nhé!

Cellos gửi lời cảm ơn đến những ý tưởng tuyệt vời từ: *Theseus OS* (SAS & Live Evolution), *Asterinas* (FrameKernel Safety), *Tock* (Embedded traits), và *Redox OS* (Microkernel IPC).
