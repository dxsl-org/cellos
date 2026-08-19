
# BUG
1. `qemu_exit::AArch64Semihosting`


# TODO

0. Khi smoke RPi3 kết thúc, duyệt triển khai: $hc-cook /home/dmin/cellos/.agents/260819-1416-port-common-drivers-g1-g2-g3/plan.md

1. Hoàn tất G1: làm một vertical slice GPIO/I²C sensor → Cell → output trên RPi3.

3. **Chưa làm — lỗ hổng cấu trúc, không phải lỗ hổng CI.** Ban đầu tôi quy lỗi arity trên cho
việc CI không build rv32. Sai: đã thử khai báo thiếu tham số trong `rv64/trap.rs` (target CI
build mọi lần push) và `cargo check -p cellos-kernel` vẫn xanh. rustc **không** đối chiếu khai
báo `extern "Rust"` với định nghĩa `#[no_mangle]` ở crate khác — `clashing_extern_declarations`
chỉ so các khai báo trong cùng crate, còn linker chỉ khớp tên symbol chứ không khớp signature.
Nên **không lane CI nào bắt được lỗi này**, và HAL hiện có 23 khai báo tay như vậy (14 symbol:
`vi_terminate_on_fault`, `vi_timer_tick`, `vi_trap_handler`, `ViCell_syscall_dispatch`, …), mỗi
cái là một chỗ signature có thể lệch âm thầm. Cách sửa bền là để signature tồn tại đúng một
nơi compiler kiểm được (crate trait dùng chung / macro sinh cả khai báo lẫn định nghĩa), thay
vì mỗi kiến trúc tự khai báo lại.

4. Ghi kèm để ai định thêm lane rv32 biết trước: kernel **không** build được cho
`riscv32imac-unknown-none-elf` — hai lỗi `E0308` có sẵn (`task/syscall.rs:3540`,
`task.rs:483`, đều là `u32` vs `usize` do trap frame rv32 dùng `u32`). `hal-riscv` thì compile sạch.

6. AI inference server demo (HTTP → NPU cell → response, P99 bound) = G2 Level A, chính là bước cần board RK3588 — đây là mắt xích nối G2 sang G3.
7. Desktop đầy đủ (compositor + mouse, windowed) — 📋; VFS scale (ext4/large disk) — 📋; 
App Platform Layers §J: L1 SDK ✅, còn L0 docs / L2 middleware / L3 tooling / L4 observability — 📋.

### Cell-to-Cell Anywhere
1. Chốt/commit slice HAL đang dở; worktree hiện có nhiều thay đổi kernel/HAL, không nên trộn VM vào cùng trạng thái.
   **xử lý nợ kỹ thuật extern "Rust" viết tay có nguy cơ lệch chữ ký.**
2. Đồng bộ tài liệu với code thực tế và tạo smoke x86 tái lập được.
3. Mở nhánh riêng, ví dụ feat/g2-tier3b-vm-closure.
4. Dùng ARM64 làm đường ngắn nhất để đóng gate: Alpine → nginx → HTTP request/response có log.
   **Gate graduation "nginx chạy thật trong Linux VM" — chưa verify.**
5. Sau đó nối VirtIO MMIO/block/net cho x86.
6. Tiếp theo mới làm persistent disk, Ubuntu/glibc và các lane AMD/Intel hardware.
Kết luận ngắn: GO cho Tier 3b G2 ngay bây giờ, nhưng dưới dạng hoàn thiện VMM hiện có; NO-GO cho việc nhảy thẳng sang G5 snapshot/CoW.