# Tính Đa Kiến Trúc (Multi-Architecture) trong Hệ Điều Hành

## 1. Code RISC-V có chạy được trên PC (x86) hay ARM không?

Câu trả lời ngắn gọn là: **KHÔNG**.

### Tại sao?
Mỗi loại chip (CPU) sử dụng một "ngôn ngữ" khác nhau, gọi là **ISA** (Instruction Set Architecture - Kiến trúc tập lệnh).

| Chip | Kiến trúc | Ngôn ngữ máy (Ví dụ lệnh Cộng) |
|------|-----------|-------------------------------|
| **Intel/AMD** | x86_64 | `add rax, rbx` (0x48 0x01 0xD8) |
| **Apple M1/M2, Robot** | ARM64 | `add x0, x0, x1` (0x8B010000) |
| **ViOS hiện tại** | RISC-V | `add t0, t0, t1` (0x006282B3) |

**Nếu bạn đưa một file kernel đã build cho RISC-V vào máy x86, CPU x86 sẽ thấy toàn "tiếng lạ" và báo lỗi "Illegal Instruction" ngay lập tức.**

---

## 2. Vậy làm sao để OS chạy được trên nhiều thiết bị khác nhau?

Bí quyết nằm ở thiết kế **Hệ điều hành đa tầng**.

### Mô hình "Lõi chung - Vỏ riêng"

Để OS có thể chạy trên cả PC, Điện thoại, và Máy chơi game, lập trình viên chia code thành 2 phần:

#### A. Phần Độc lập Kiến trúc (Architecture Independent - ~90%)
Đây là code Rust nguyên bản, không động chạm gì đến hợp ngữ (assembly).
- **Bộ lập lịch (Scheduler)**: Thuật toán chọn process nào chạy tiếp.
- **Hệ thống file (VFS)**: Cách tổ chức folder, file.
- **Mạng (Network Stack)**: Cách xử lý gói tin TCP/IP.
- **Logic Ứng dụng**: Shell, Calculator...

#### B. Tầng Trừu tượng Phần cứng (HAL/Arch Layer - ~10%)
Đây là code riêng biệt cho từng loại chip.
- **Bootloader**: Cách đánh thức CPU lúc vừa bật nguồn.
- **Quản lý bộ nhớ (MMU)**: Cách chip đó phân chia RAM.
- **Ngắt (Interrupts)**: Cách chip đó nhận tín hiệu từ phần cứng.

---

## 3. Cấu trúc của ViOS để hỗ trợ nhiều Chip

Trong tương lai, thư mục `hal/` của chúng ta sẽ trông như thế này:

```
hal/
├── hal-core/        # Chứa "Giao diện chung" (Traits)
├── hal-riscv/       # Code riêng cho chip RISC-V (Hiện tại)
├── hal-x86_64/      # Code riêng cho chip Intel/AMD (Tương lai)
└── hal-arm64/       # Code riêng cho chip ARM/Apple (Tương lai)
```

### Ví dụ về Trait `SerialPort`
Trong `hal-core`, chúng ta định nghĩa:
```rust
pub trait SerialPort {
    fn send(&mut self, data: u8);
}
```

- **RISC-V** sẽ triển khai viết vào địa chỉ `0x10000000`.
- **x86_64** sẽ triển khai dùng lệnh `out` vào port `0x3F8`.

**Kernel chỉ gọi `serial.send()`, nó không cần biết nó đang chạy trên chip nào!**

---

## 4. Quá trình "Porting" (Cổng dịch)

Khi bạn muốn đưa ViOS lên máy chơi game (VD: PlayStation dùng chip x86 custom):

1. **Giữ nguyên** thư mục `kernel/`, `libs/`, `apps/`.
2. **Tạo mới** thư mục `hal/hal-playstation`.
3. **Viết lại** file `boot.s` và các hàm quản lý ngắt cho chip đó.
4. **Compile lại** (Recompile) toàn bộ dự án với Target mới.

---

## 5. Tại sao Linux/Windows làm được?

- **Linux**: Có hàng ngàn folder trong thư mục `arch/` (arch/x86, arch/arm, arch/mips...). Khi bạn cài Linux, nó sẽ tự chọn đúng folder cho máy bạn.
- **Windows**: Ngày xưa chỉ có x86, nhưng giờ đã có **Windows on ARM** (chạy trên Surface Pro 9, MacBook qua ảo hóa). Microsoft đã phải "port" tầng HAL của Windows sang ARM.

---

## 6. Tổng kết

- **Code máy (Binary)**: Không chạy chung được.
- **Code nguồn (Source Code)**: Chạy chung được nếu thiết kế tốt (**HAL**).
- **ViOS**: Đang được thiết kế rất tốt vì chúng ta dùng **Traits** trong `hal-core`. Việc mang ViOS sang chip khác chỉ là vấn đề viết thêm module HAL mới, không cần sửa Logic Kernel.

**Đó là lý do tại sao chúng ta tập trung vào "Traits" và "Interfaces" ngay từ đầu!** 🚀
