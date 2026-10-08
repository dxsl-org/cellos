# Cellos

[![CI](https://github.com/dxsl-org/cellos/actions/workflows/ci.yml/badge.svg)](https://github.com/dxsl-org/cellos/actions/workflows/ci.yml)
[![Ko-fi](https://img.shields.io/badge/Ko--fi-Donate-%23FF5E5B?logo=ko-fi)](https://ko-fi.com/dxsl_org)
[🌐 English](./README.md)

**Cellos là hệ điều hành nghiên cứu Rust-native với một hướng đi đang hoạt động duy nhất: Cell-to-Cell Anywhere trên Intel x86-64.**

Mục tiêu là giao tiếp Cell-to-Cell có hợp đồng rõ ràng giữa thực thi cục bộ, LAN và relay có kiểm soát, trên một cấu hình phần cứng Intel cố định, không cần giao diện đồ họa (headless). Phần mềm tham gia qua hợp đồng và adapter cụ thể, không phải tự động phân tán ứng dụng bất kỳ. Các Cell native tin cậy dùng chung Single Address Space (SAS); cô lập bằng ngôn ngữ Rust không phải ranh giới phần cứng hay bảo mật tuyệt đối.

[Định hướng chỉ Intel (ADR-0022)](./docs/decisions/0022-intel-x86-64-c2c-only-direction.md) và [trọng tâm hiện tại](./docs/roadmap/current-focus.md) chi phối mọi công việc. Mỗi việc phải chỉ rõ đầu ra C2C trên Intel, phụ thuộc trực tiếp hoặc kiểm thử hồi quy cần thiết. Các chương trình độc lập về GUI, trình duyệt, AI, robot, OS đa dụng và nền tảng AMD/ARM/RISC-V mới đều tạm dừng; mã nguồn, bằng chứng hiện có và hồi quy đa kiến trúc cần thiết vẫn được giữ lại.

---

## ✨ Điểm khác biệt của Cellos (So với OS truyền thống)

Các cơ chế kiến trúc sau phục vụ hướng nghiên cứu này, với ranh giới tin cậy và mức bằng chứng riêng:

*   **Không gian bộ nhớ chung (Cellular SAS):** Thành phần tin cậy có thể chia sẻ bộ nhớ và chuyển quyền sở hữu buffer. Điều này không hứa hẹn zero-copy qua ranh giới tier hoặc giữa các máy.
*   **Cô lập bằng ngôn ngữ (LBI):** Kiểm tra Safe Rust giảm rủi ro an toàn bộ nhớ trong nền tảng tin cậy gồm compiler, kernel và mã unsafe đã duyệt. Nó không cô lập nhị phân không tin cậy bất kỳ khỏi SAS.
*   **Nghiên cứu Heap Snapshot:** Đã có phần định dạng snapshot, nhưng capture/restore vẫn tắt trong image phát hành, chờ quiescence và bằng chứng lưu/reset/khôi phục/tiếp tục trên board thật; chưa có bảo đảm khởi động tức thì.
*   **Mục tiêu ba tier:** Cell native, miền phân trang C/C++ và guest VM sẽ tham gia C2C qua adapter rõ ràng. Đây là hướng nghiên cứu và xác minh, không phải tuyên bố cả ba tier đã hoạt động end-to-end trên Intel.

---

## 🎯 Tầm nhìn & Định vị: Cellos là gì (và không là gì)?

Mục tiêu đang hoạt động duy nhất là **Cell-to-Cell Anywhere trên một cấu hình Intel x86-64 cố định, headless**, không phải chương trình OS desktop đa dụng, robot hay nhiều thị trường song song.

*   **Một mẫu phần cứng trước:** Xác minh một cấu hình chính xác trước khi thêm máy thứ hai cùng mẫu. Yêu cầu đích gồm VT-x/EPT, VT-d, COM1, HPET và hợp đồng firmware/thiết bị trong [HCL](./docs/hardware-compatibility-list.md).
*   **Cục bộ, LAN và relay có cổng riêng:** Giữ nguyên yêu cầu danh tính, phân quyền, protected authority và cổng production. Quyết định này không cho phép mua phần cứng, nới ABI/bảo mật, tự động thực thi từ xa hoặc kích hoạt production.
*   **Không phân tán ứng dụng bất kỳ một cách trong suốt:** Ứng dụng phải có bản port hoặc adapter xác định; chạy trong VM không tự biến ứng dụng thành Cell phân tán.

### Mô hình 3 Tier (Ranh giới thực thi)

Cả ba tier đều thuộc mục tiêu C2C, với ranh giới tin cậy khác nhau:
1.  **Tier 1 (Lõi & Native Cell):** Cell native tin cậy trong SAS chung, tuân thủ ký mã và ràng buộc tin cậy đã duyệt.
2.  **Tier 2 (Paged Domain Cell):** Miền phân trang phần cứng cho C/C++ và các workload cần cô lập. Bằng chứng admission x86 mới ở mức test-only; khoảng trống C++ shim chưa được khép lại.
3.  **Tier 3 (VM Guest):** Thành phần trong VM tham gia qua guest adapter rõ ràng. Intel VMX chưa hoàn tất; bằng chứng AMD SVM trên QEMU không xác minh Intel. [Hướng dẫn Guest](./docs/guides/tier3b-linux-vm.md) là tài liệu kỹ thuật; [quyết định trình duyệt trước đây](./docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md) không kích hoạt chương trình trình duyệt.

---

## 🚀 Trạng thái dự án: `v0.2.1-dev` (Mycelium)

Hướng đang hoạt động: **Cell-to-Cell Anywhere trên Intel x86-64**. Bảng dưới giữ lại bằng chứng kiến trúc hiện có, không phải danh sách chương trình phát triển song song.

| Nền tảng | Vai trò / bằng chứng | Giới hạn |
|--------|----------------------|----------|
| `x86_64-unknown-none` (Intel) | **Đích hoạt động duy nhất**; bằng chứng boot/CPL3 QEMU q35 | [Hướng dẫn q35](./boards/qemu/q35-x86_64/README.md); Intel VMX chưa hoàn tất; chưa có máy x86 vật lý đạt chuẩn HCL. |
| `riscv64gc-unknown-none-elf` | Lane tham chiếu/QEMU hiện có; dừng phát triển nền tảng mới | Giữ bằng chứng và hồi quy mã dùng chung cần thiết; không phải hướng chính. |
| `aarch64-unknown-none` | Bằng chứng boot và thiết bị cụ thể hiện có; dừng phát triển nền tảng mới | Có thể tham khảo protected authority cho phụ thuộc Intel, không mở chương trình ARM mới. |
| `riscv32imc-unknown-none-elf` | Bằng chứng boot Cellos-Nano trên QEMU hiện có; dừng phát triển nền tảng mới | Giữ triển khai và hồi quy cần thiết. |

QEMU chỉ cung cấp bằng chứng phần mềm/tích hợp, không chứng nhận phần cứng thật. Các ID `igb` được hỗ trợ (`8086:10c9` trong QEMU và i210 có flash `8086:1533`) không chứng minh một máy thật đã đạt chuẩn.

---

## Bắt đầu

**Đường x86 đang hoạt động:** Theo [hướng dẫn build và test QEMU q35 x86-64 trong repo](./boards/qemu/q35-x86_64/README.md). Lane này là bằng chứng phần mềm, không phải hoàn tất Intel VMX hay chứng nhận PC vật lý.

### Tham khảo quickstart RV64 cũ

Các lệnh dưới được giữ cho lane RV64 tham chiếu hiện có, không phải bản build mặc định của chương trình Intel. Cần **Rust nightly**, `qemu-system-riscv64`, và Python 3/PowerShell.

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
3. **Giữ ranh giới kiến trúc:** Dùng `VAddr`/`PAddr`, không hardcode kích thước pointer. Bảo toàn tính đúng đắn đa kiến trúc hiện có không kích hoạt chương trình phần cứng khác.
4. **Unsafe Management:** Cell Safe Rust dùng `#![forbid(unsafe_code)]`; ngoại lệ driver/FFI đã duyệt phải được nêu rõ. Thao tác unsafe trong kernel/HAL cần ghi chú bất biến `// SAFETY:`.
5. **Modern Module Style:** Dùng `foo.rs` và thư mục `foo/`. Cấm file `mod.rs`.
6. **Cellos Naming:** Trait và Type dùng tiền tố `Vi` (Virtual Interface, ví dụ: `ViDriver`). Tên file viết thường `snake_case`.
7. **Trait Objects:** Ở ranh giới hệ thống, dùng đa hình tĩnh qua `Arc<dyn ViDriver + Send + Sync>`.
8. **RAII - Clean Up Explicitly:** Các Cell tự chịu trách nhiệm dọn dẹp tài nguyên (Drop). Không có process-based cleanup do bản chất chia sẻ bộ nhớ SAS.

👉 Đọc chi tiết tại [CONTRIBUTING.md](./CONTRIBUTING.md) và [code-standards.md](./docs/code-standards.md).

---

## 📚 Kho tài liệu

Cellos có hệ thống đặc tả và kiến trúc minh bạch. Trước khi bắt tay vào một phân hệ mới, hãy tìm đọc tài liệu tương ứng:

*   **Hướng đang hoạt động:** [ADR-0022](./docs/decisions/0022-intel-x86-64-c2c-only-direction.md) | [current-focus.md](./docs/roadmap/current-focus.md)
*   **Bắt đầu:** [getting-started.md](./docs/getting-started.md) | [project-roadmap.md](./docs/project-roadmap.md)
*   **Kiến trúc:** [system-architecture.md](./docs/system-architecture.md) | [hardware-dev-guide.md](./docs/hardware-dev-guide.md)
*   **Đặc tả hệ thống (Specs):** Từ [00-context.md](./docs/specs/00-context.md) đến bộ nhớ ([02-memory.md](./docs/specs/02-memory.md)), ứng dụng tầng ([05-application.md](./docs/specs/05-application.md)), mạng, VFS... (Xem trong thư mục `docs/specs/`).

---

**Đóng góp ý tưởng hoặc mã nguồn?** Chạy `cargo clippy -- -D warnings` và `cargo test --all` trước khi tạo PR nhé!

Cellos gửi lời cảm ơn đến những ý tưởng tuyệt vời từ: *Theseus OS* (SAS & Live Evolution), *Asterinas* (FrameKernel Safety), *Tock* (Embedded traits), và *Redox OS* (Microkernel IPC).
