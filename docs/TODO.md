# TODO

1. Hoàn tất G1: làm một vertical slice GPIO/I²C sensor → Cell → output trên RPi3.
2. Hoặc xử lý nợ kỹ thuật extern "Rust" viết tay có nguy cơ lệch chữ ký.
3. Tránh code phình theo số board, Cellos nên hướng đến cấu trúc:
```
hal/arch/aarch64
hal/arch/riscv64

hal/soc/bcm27xx
hal/soc/jh7110
hal/soc/rk3588

boards/rpi3
boards/visionfive2
boards/rock5
```
Mỗi board chỉ nên chứa:
- Board identity và compatible strings.
- Boot/firmware contract.
- Pinmux và PHY wiring.
- DTB/fallback memory map.
- Danh sách driver SoC cần bật.

Driver UART, SDHCI, DesignWare I²C/SPI, GIC/PLIC và PCIe không được sao chép thành phiên bản riêng cho từng board.

4. **Chưa làm — lỗ hổng cấu trúc, không phải lỗ hổng CI.** Ban đầu tôi quy lỗi arity trên cho
việc CI không build rv32. Sai: đã thử khai báo thiếu tham số trong `rv64/trap.rs` (target CI
build mọi lần push) và `cargo check -p cellos-kernel` vẫn xanh. rustc **không** đối chiếu khai
báo `extern "Rust"` với định nghĩa `#[no_mangle]` ở crate khác — `clashing_extern_declarations`
chỉ so các khai báo trong cùng crate, còn linker chỉ khớp tên symbol chứ không khớp signature.
Nên **không lane CI nào bắt được lỗi này**, và HAL hiện có 23 khai báo tay như vậy (14 symbol:
`vi_terminate_on_fault`, `vi_timer_tick`, `vi_trap_handler`, `ViCell_syscall_dispatch`, …), mỗi
cái là một chỗ signature có thể lệch âm thầm. Cách sửa bền là để signature tồn tại đúng một
nơi compiler kiểm được (crate trait dùng chung / macro sinh cả khai báo lẫn định nghĩa), thay
vì mỗi kiến trúc tự khai báo lại.

Ghi kèm để ai định thêm lane rv32 biết trước: kernel **không** build được cho
`riscv32imac-unknown-none-elf` — hai lỗi `E0308` có sẵn (`task/syscall.rs:3540`,
`task.rs:483`, đều là `u32` vs `usize` do trap frame rv32 dùng `u32`). `hal-riscv` thì compile sạch.

Gate graduation "nginx chạy thật trong Linux VM" — chưa verify.
AI inference server demo (HTTP → NPU cell → response, P99 bound) = G2 Level A, chính là bước cần board RK3588 — đây là mắt xích nối G2 sang G3.
Desktop đầy đủ (compositor + mouse, windowed) — 📋; VFS scale (ext4/large disk) — 📋; 
App Platform Layers §J: L1 SDK ✅, còn L0 docs / L2 middleware / L3 tooling / L4 observability — 📋.
Cell-to-Cell Anywhere phần G2 (P04-P08: HyParView, hole-punch, K2/K3 DICE)
---

## Từ đợt audit mô tả kiến trúc (2026-07-31)

Bốn việc dưới đây phát sinh từ đợt đối chiếu spec với code; mỗi việc kèm bằng chứng vì lý do
tại sao nó không hiển nhiên. Docket đầy đủ (D1–D25, 8 mục đã chốt) nằm ở
`.agents/reports/decision-docket-260730.md` — **gitignored**, nên phần cần sống lâu ghi ở đây.

**A1 — RISC-V đọc memory node của DTB thay vì `FALLBACK_MEMORY_MAP`.** `kernel/src/boot.rs`
khai cứng vùng usable = `0x0BE0_0000` = **190 MiB** cho "QEMU virt (256 MB)", và không có
đường nào đọc DTB. Cấp cho guest 2 GiB thì kernel vẫn chỉ thấy 190 MiB. Đo được: spawn cell
đậu (parked) tới khi bị từ chối dừng ở **n = 9**, dù `MAX_CELLS` đã nâng 512 và còn 512 VA
slot — trần thật là RAM nhìn thấy được, không phải hằng số nào ta hay bàn. Đây không chỉ chặn
profile per-request server: **mọi deployment đang âm thầm bỏ RAM trên 190 MiB.** Rẻ nhất, đòn
bẩy lớn nhất.

**A2 — DONE 2026-08-01: cell-spawn OOM có mã riêng và log chẩn đoán.** Bốn syscall spawn
cell trả additive `-2` cho OOM, ostd giải mã thành `SyscallError::OutOfMemory`; lỗi generic vẫn
là `-1`, opcode cũ không đổi. Runtime probe xác nhận log nguồn cấp phát + caller/path, không
panic và shell tiếp tục hoạt động.

**A3 — DONE 2026-08-01: MemInfo và benchmark dùng số thật.** `MemInfo=243`, allowlist bit 56,
trả `ViMemInfoV1` 32 byte theo opt-in vì đây là telemetry xuyên cell. Frame allocator kế toán
chính xác theo bitmap transition. Benchmark đo **135.782.400 byte (129,49 MiB)**
allocator-committed, nên mục tiêu `<10 MiB` hiện **FAIL thật** thay vì PASS giả.

**Follow-up dung lượng:** giảm 129,49 MiB xuống dưới 10 MiB là việc tối ưu riêng; không đổi
định nghĩa metric hoặc threshold để làm gate xanh. `capacity-probe` có tính phá huỷ chỉ được
include/sign khi build test-mode với `CELLOS_INCLUDE_CAPACITY_PROBE=1`; image mặc định loại nó.

**D5 — QUEUED: profile per-request server.** Giữ mục tiêu qualification 1000 Cell cô lập đồng
thời, nhưng không coi đó là capacity hiện tại. Đo N=64/128/256/512 trước; sau Midori mới xét
shared `.text`/`.rodata` bất biến, stack demand-page, quota riêng theo profile và bảng động.
Large-app với mặc định 64 Cell không đổi.

**A4 — chạy lại cổng runtime mà phase 09 và 11 để ngỏ.** Cả hai đóng với lý do "runtime
UNVERIFIED — máy không có QEMU/cross toolchain". Tiền đề đó **sai**: QEMU cả ba arch và
`riscv64-unknown-elf-*` đều có. Hai vấn đề thật đều nhỏ — `build.rs` khai cứng tên
`riscv-none-elf-*` khi biến `CC_<target>` chưa set, và `gen_disk.ps1` soạn
`CFLAGS_riscv64gc_unknown_none_elf` nhưng không truyền được sang cargo (littlefs thiếu
`string.h`). Phase 10 đã verify theo đường này ngày 2026-07-31: `wx-text-write` 2/2 PASS, suite
`boot` 54/54 PASS. Cách làm ghi ở `.agents/reports/qemu-build-unblock-260731.md`.

**Lưu ý cho integration test trên Linux**: cần `--target x86_64-unknown-linux-gnu`. Bản
`.cargo/config.toml` trong repo mặc định target Windows, nên `cargo test` trần sẽ fail vì không
tìm thấy `core` cho `x86_64-pc-windows-msvc` trước khi kịp khởi động QEMU.

## BUG
1. `qemu_exit::AArch64Semihosting`
