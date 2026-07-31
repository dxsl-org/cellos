# TODO

Lane hypervisor bất định — **đã xong** (PR #16). Nguyên nhân: `vt_irq_el2_lower`
(`hal/arch/arm/src/aarch64/el2.rs`) dùng chung thân với `vt_irq_el2_cur` và không đọc
`TPIDR_EL2`, nên tick timer rơi đúng lúc vCPU đang chạy bị xử lý ngay tại chỗ —
`vi_timer_tick` → `yield_cpu` → context switch, đưa một Cell lên CPU ở EL0 trong khi
`HCR_EL2.VM` còn bật (Stage-2 sống với VTTBR_EL2 của guest) và bank sysreg EL1 vẫn giữ
`TTBR0_EL1`/`TCR_EL1`/`SCTLR_EL1`/`VBAR_EL1` của guest. Bank host chỉ được restore ở
`run_vcpu_impl` bước 4, đường đó không bao giờ tới, nên mọi lần fetch lệnh của Cell abort với
EC 0x20 / ISS 0x6. `HCR_EL2.IMO` route IRQ lên EL2 và IRQ đã route lên EL2 thì `PSTATE.I` của
EL thấp hơn không mask được, nên guest chạy với DAIF mask hết vẫn không ngăn được.

Hai kết luận trong bản ghi cũ ở đây đều **sai**, giữ lại để không ai lặp lại: chữ ký
`[fault] Cell 2 terminated: scause=0x82000006` không phải lỗi VFS (Cell 2 chỉ tình cờ là cell
bị schedule kế tiếp), và badge đỏ của main **có** phản ánh sức khoẻ code — nó chỉ ra một lỗi
kernel thật. Tính bất định là do cần tick rơi đúng cửa sổ guest chạy *và* scheduler chọn đúng
một Cell.

Sửa: vector IRQ lower-EL giờ check `TPIDR_EL2` như vector sync, thoát guest qua `vt_vcpu_trap`
rồi báo `ViVmExit::Preempted`. Lane xanh 4/4 lần sau khi sửa, không lần nào còn `[fault] Cell`
hay `panic-in-cell`. Fault phụ EC 0x22 (`elr=0x4153C0E9`, PC lệch 4 byte ở EL2) cũng biến mất
cùng lúc, đúng như dự đoán rằng nó là hệ quả của cùng cửa sổ state nửa vời — không phải bug
riêng. Chi tiết: `.agents/reports/debug-260729-1401-el2-irq-guest-preemption.md`.

Việc phái sinh từ lần điều tra đó cũng đã xong: dòng fault từng in `ESR_EL2` dưới tên RISC-V
`scause` — chính chỗ làm bản ghi cũ ở trên giải mã sai — nay dùng tên theo vai trò
(`cause`/`pc`/`addr`) và có bảng đối chiếu từng kiến trúc trong rustdoc của
`terminate_current_cell_on_fault`. Sửa kèm: `hal/arch/riscv/src/rv32/trap.rs` khai báo
`vi_terminate_on_fault` thiếu một tham số so với định nghĩa, nên in ra rác làm địa chỉ fault.

**Chưa làm — lỗ hổng cấu trúc, không phải lỗ hổng CI.** Ban đầu tôi quy lỗi arity trên cho
việc CI không build rv32. Sai: đã thử khai báo thiếu tham số trong `rv64/trap.rs` (target CI
build mọi lần push) và `cargo check -p vicell-kernel` vẫn xanh. rustc **không** đối chiếu khai
báo `extern "Rust"` với định nghĩa `#[no_mangle]` ở crate khác — `clashing_extern_declarations`
chỉ so các khai báo trong cùng crate, còn linker chỉ khớp tên symbol chứ không khớp signature.
Nên **không lane CI nào bắt được lỗi này**, và HAL hiện có 23 khai báo tay như vậy (14 symbol:
`vi_terminate_on_fault`, `vi_timer_tick`, `vi_trap_handler`, `ViCell_syscall_dispatch`, …), mỗi
cái là một chỗ signature có thể lệch âm thầm. Cách sửa bền là để signature tồn tại đúng một
nơi compiler kiểm được (crate trait dùng chung / macro sinh cả khai báo lẫn định nghĩa), thay
vì mỗi kiến trúc tự khai báo lại.

Ghi kèm để ai định thêm lane rv32 biết trước: kernel **không** build được cho
`riscv32imac-unknown-none-elf` — hai lỗi `E0308` có sẵn (`task/syscall.rs:3540`,
`task.rs:483`, đều là `u32` vs `usize` do trap frame rv32 dùng `u32`). `hal-riscv` thì compile
sạch.

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

**A2 — `OutOfMemory` cần error riêng, và cần log chỗ cấp phát thất bại.**
`ViError::OutOfMemory` bị map thành `SyscallError::Unknown` (`kernel/src/task/syscall.rs`) và
không log gì. Thất bại duy nhất mà một thí nghiệm dung lượng cần thấy lại là thất bại ABI
không diễn đạt được; ngay hai dòng trên, `NotFound → FileNotFound` map đúng.

**A3 — cần syscall MemInfo; `memory_footprint` bench hiện không đo gì.**
`cells/tests/bench/src/scenarios/memory_footprint.rs` trả về một hằng số compile-time kèm
`// TODO: replace with MemInfo syscall`, mà suite báo **PASS**. Ngoài `total_frames` không có
kế toán frame trống nào, nên A1 phải suy ra dung lượng bằng cách spawn tới khi fail thay vì
đọc một con số.

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
