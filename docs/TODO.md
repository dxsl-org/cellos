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

Còn treo từ lần điều tra đó: dòng fault in ESR_EL2 dưới tên `scause`
(`kernel/src/task.rs:293,315`, giá trị đi qua `vi_terminate_on_fault` từ
`hal/arch/arm/src/aarch64/trap.rs`) — tên RISC-V trên ARM64, đã làm lệch hướng chẩn đoán một
lần rồi, nên đổi tên theo kiến trúc.

Gate graduation "nginx chạy thật trong Linux VM" — chưa verify.
AI inference server demo (HTTP → NPU cell → response, P99 bound) = G2 Level A, chính là bước cần board RK3588 — đây là mắt xích nối G2 sang G3.
Desktop đầy đủ (compositor + mouse, windowed) — 📋; VFS scale (ext4/large disk) — 📋; 
App Platform Layers §J: L1 SDK ✅, còn L0 docs / L2 middleware / L3 tooling / L4 observability — 📋.
Cell-to-Cell Anywhere phần G2 (P04-P08: HyParView, hole-punch, K2/K3 DICE)