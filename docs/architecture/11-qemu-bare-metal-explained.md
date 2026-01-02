# QEMU và Bare Metal RISC-V - Giải Thích Chi Tiết

## 🖥️ QEMU là gì?

**QEMU** (Quick Emulator) là một **máy ảo mã nguồn mở** có thể:
- **Giả lập (Emulate)** phần cứng của nhiều kiến trúc CPU khác nhau
- **Ảo hóa (Virtualize)** hệ điều hành

### Sự Khác Biệt: Emulation vs Virtualization

| Emulation | Virtualization |
|-----------|----------------|
| Giả lập **toàn bộ** phần cứng | Chạy trực tiếp trên CPU thật |
| Chậm hơn (dịch mã máy) | Nhanh gần như native |
| Có thể chạy ARM trên x86 | Chỉ chạy cùng kiến trúc |
| **QEMU sử dụng mode này** | VirtualBox, VMware |

### Ví Dụ Thực Tế

```
┌─────────────────────────────────────┐
│   Máy Tính Của Bạn (Windows/x86)   │
│                                     │
│  ┌───────────────────────────────┐ │
│  │         QEMU Process          │ │
│  │                               │ │
│  │  ┌─────────────────────────┐ │ │
│  │  │  Máy Ảo RISC-V          │ │ │
│  │  │  - CPU: rv64gc          │ │ │
│  │  │  - RAM: 128MB           │ │ │
│  │  │  - UART: NS16550A       │ │ │
│  │  │  - Timer: CLINT         │ │ │
│  │  │                         │ │ │
│  │  │  [ViOS Kernel chạy đây]│ │ │
│  │  └─────────────────────────┘ │ │
│  └───────────────────────────────┘ │
└─────────────────────────────────────┘
```

---

## 🔩 Bare Metal là gì?

**Bare Metal** = "Kim loại trần" = **Chạy trực tiếp trên phần cứng**, không có OS nào khác ở dưới.

### So Sánh: Application vs OS

#### Application Thông Thường
```
┌─────────────────┐
│  Your App.exe   │  ← Chương trình của bạn
├─────────────────┤
│  Windows/Linux  │  ← Hệ điều hành
├─────────────────┤
│  Hardware (CPU) │  ← Phần cứng
└─────────────────┘
```

#### Bare Metal OS (ViOS)
```
┌─────────────────┐
│  ViOS Kernel    │  ← Kernel của bạn chạy TRỰC TIẾP
├─────────────────┤
│  Hardware (CPU) │  ← Không có OS nào ở giữa!
└─────────────────┘
```

### Điều Này Có Nghĩa Là Gì?

1. **Bạn phải tự làm MỌI THỨ**:
   - Khởi tạo bộ nhớ (stack, heap)
   - Quản lý ngắt (interrupts)
   - Giao tiếp với phần cứng (UART, timer)
   - Không có `printf()`, `malloc()`, `sleep()` sẵn!

2. **Không có "Safety Net"**:
   - Lỗi → Máy treo (kernel panic)
   - Không có debugger
   - Không có error messages (trừ khi bạn tự viết)

---

## 🏗️ RISC-V là gì?

**RISC-V** (đọc là "risk-five") là một **kiến trúc CPU mã nguồn mở**.

### Tại Sao Chọn RISC-V?

| Đặc Điểm | Giải Thích |
|----------|------------|
| **Mã Nguồn Mở** | Không cần trả phí license (khác ARM, x86) |
| **Đơn Giản** | Chỉ ~50 lệnh cơ bản (x86 có hàng ngàn!) |
| **Mở Rộng Được** | Có thể thêm lệnh tùy chỉnh |
| **Phổ Biến** | Dùng trong IoT, AI chips, robotics |

### RISC-V vs x86 vs ARM

```
Complexity:
x86:    ████████████████████ (Phức tạp nhất)
ARM:    ████████████         (Trung bình)
RISC-V: ██████               (Đơn giản nhất)

License Cost:
x86:    $$$$ (Intel/AMD độc quyền)
ARM:    $$$  (Phải trả phí)
RISC-V: FREE (Mã nguồn mở)
```

---

## 🎯 Tại Sao ViOS Dùng QEMU + RISC-V?

### 1. Phát Triển Dễ Dàng
```
Không cần phần cứng thật → Dùng QEMU
Không cần mua board RISC-V (vài triệu đồng)
Có thể test ngay trên laptop
```

### 2. Debug Thuận Tiện
```
QEMU có thể:
- Dừng máy ảo bất cứ lúc nào
- Xem toàn bộ bộ nhớ
- Ghi log mọi lệnh CPU
```

### 3. Chuẩn Bị Cho Phần Cứng Thật
```
Code chạy trên QEMU → Chạy trên board thật
Chỉ cần thay đổi địa chỉ UART, Timer
Logic kernel giữ nguyên 100%
```

---

## 🔧 Cách QEMU Giả Lập RISC-V

### QEMU "virt" Machine

QEMU cung cấp một máy ảo tên là **"virt"** với:

```
Memory Map (Bản đồ bộ nhớ):
0x0000_0000 - 0x0100_0000: Debug/Test devices
0x0200_0000 - 0x0200_FFFF: CLINT (Timer)
0x0C00_0000 - 0x1000_0000: PLIC (Interrupt Controller)
0x1000_0000 - 0x1000_0100: UART (Serial Port)
0x8000_0000 - 0x8800_0000: RAM (128MB)
```

### Khi Bạn Chạy QEMU

```powershell
qemu-system-riscv64 -machine virt -kernel kernel
```

QEMU sẽ:
1. **Tạo CPU ảo RISC-V** (64-bit)
2. **Cấp phát 128MB RAM** tại địa chỉ `0x8000_0000`
3. **Giả lập UART** tại `0x1000_0000`
4. **Load kernel** vào RAM
5. **Nhảy đến địa chỉ `0x8000_0000`** (entry point)

---

## 📝 Ví Dụ: Boot Sequence của ViOS

### Bước 1: QEMU Khởi Động
```
QEMU: "Tôi là một máy RISC-V!"
QEMU: "Tôi có 128MB RAM tại 0x80000000"
QEMU: "Tôi load file 'kernel' vào RAM..."
QEMU: "Nhảy đến 0x80000000 (địa chỉ _start)..."
```

### Bước 2: Assembly Boot Code (`hal-riscv/boot.s`)
```asm
_start:
    # Tắt ngắt
    csrw sie, zero
    
    # Thiết lập stack pointer
    la sp, __stack_end
    
    # Xóa .bss section (biến global)
    la t0, __bss_start
    la t1, __bss_end
1:  sd zero, 0(t0)
    addi t0, t0, 8
    bltu t0, t1, 1b
    
    # Nhảy đến Rust!
    call kmain
```

### Bước 3: Rust Entry Point (`kernel/main.rs`)
```rust
#[no_mangle]
pub extern "C" fn kmain() -> ! {
    // Khởi tạo logger (UART)
    init_logger();
    
    // Khởi tạo heap allocator
    memory::init();
    
    // Khởi tạo scheduler
    process::init();
    
    log::info!("ViOS Ready!");
    
    // Idle loop
    loop {
        unsafe { asm!("wfi"); } // Wait For Interrupt
    }
}
```

### Bước 4: UART Output
```
ViOS ghi vào địa chỉ 0x10000000 (UART)
     ↓
QEMU nhận byte từ địa chỉ đó
     ↓
QEMU in ra terminal của bạn
     ↓
Bạn thấy: "[INFO] ViOS Ready!"
```

---

## 🆚 So Sánh: Simulation vs Bare Metal

### Simulation Mode (Hiện Tại)
```rust
// Chạy trên Windows/Linux
cargo run -p kernel

// Kernel chạy như một process bình thường
// Có thể dùng println!(), std::thread, etc.
```

**Ưu điểm:**
- ✅ Debug dễ (breakpoints, print statements)
- ✅ Không cần QEMU
- ✅ Test logic nhanh

**Nhược điểm:**
- ❌ Không test được phần cứng thật
- ❌ Không test được boot sequence
- ❌ Không giống môi trường thật

### Bare Metal Mode (QEMU)
```rust
// Chạy trên QEMU RISC-V
cargo build --no-default-features -p kernel
qemu-system-riscv64 -kernel kernel

// Kernel chạy TRỰC TIẾP trên CPU ảo
// Không có OS, không có std library
```

**Ưu điểm:**
- ✅ Giống phần cứng thật 99%
- ✅ Test được boot, interrupts, hardware
- ✅ Chuẩn bị cho deployment thật

**Nhược điểm:**
- ❌ Debug khó hơn
- ❌ Cần cài QEMU
- ❌ Build chậm hơn

---

## 🎓 Tóm Tắt

### QEMU
- Máy ảo giả lập phần cứng
- Cho phép chạy code RISC-V trên máy x86
- Giống phần cứng thật 99%

### Bare Metal
- Code chạy TRỰC TIẾP trên CPU
- Không có OS nào ở dưới
- Bạn phải tự làm mọi thứ

### RISC-V
- Kiến trúc CPU mã nguồn mở
- Đơn giản, dễ học
- Phổ biến trong IoT/Robotics

### ViOS + QEMU + RISC-V
```
ViOS (OS của bạn)
  ↓
Chạy trên QEMU (Máy ảo)
  ↓
Giả lập RISC-V (Kiến trúc CPU)
  ↓
Trên Windows/Linux (Máy thật)
```

---

## 🚀 Bước Tiếp Theo

Khi code chạy tốt trên QEMU, bạn có thể:
1. Mua board RISC-V thật (VD: SiFive HiFive, Kendryte K210)
2. Flash kernel lên board
3. Chạy ViOS trên phần cứng thật!

**Và đó là lúc bạn có một OS thật sự!** 🎉
