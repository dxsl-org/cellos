# TODO

Lane hypervisor bất định — `QEMU Hypervisor Machinery Smoke (TCG)` fail 3/6 lần trên
cùng một cây source (PR #9 pass, push main sau #9 fail, PR #10 fail rồi rerun pass,
PR #12 pass, push main sau #12 fail). Chữ ký khi fail:
`[ERROR] [fault] Cell 2 terminated: scause=0x82000006` + `[panic-in-cell 9] panicked at
hal/arch/arm/src/aarch64/trap.rs:134`. Giải mã: EC=0x20 (instruction abort từ EL thấp
hơn), ISS=0x6 (translation fault mức 2) — nhưng trên một **Cell**, không phải guest, nên
cổng kiểm panic bắt được. Cell 2 trong thứ tự boot đó là cell VFS. Không do thay đổi VFS:
main hiện cùng chữ ký khi chưa có thay đổi nào. Nghĩa là badge CI của main đỏ mà không
phản ánh sức khoẻ code.
Quan sát được (đã xong): `scripts/qemu-hypervisor-smoke.sh` giờ in `qemu-hv.log` (mặc định
200 dòng cuối, đổi bằng `LOG_TAIL`) trên **mọi** nhánh fail, không chỉ các dòng fault đã grep;
và cả hai job hypervisor trong CI upload `qemu-hv.log` + `qemu-hv.raw.log` làm artifact mỗi
lần chạy (`if: always()`, giữ 14 ngày) — nên có thể so log một lần fail với một lần pass.

Bước tiếp theo: chạy lane vài lần cho ra ít nhất một artifact fail và một artifact pass, rồi
diff xem Cell 2 abort sau bước nào. Hai chỗ đáng nghi khi đọc code: (1) dòng fault in ESR_EL2
dưới tên `scause` (`kernel/src/task.rs:296`, giá trị đi qua `vi_terminate_on_fault` từ
`hal/arch/arm/src/aarch64/trap.rs:126`) — tên gọi RISC-V trên ARM64, dễ đọc sai khi điều tra;
(2) panic ở `trap.rs:134` là nhánh `_` (EC không phải 0x15/0x20/0x24) của cell 9, một sự kiện
KHÁC với abort của Cell 2 — thông điệp panic có in `ec=`, nay log mới sẽ cho thấy giá trị đó.

Gate graduation "nginx chạy thật trong Linux VM" — chưa verify.
AI inference server demo (HTTP → NPU cell → response, P99 bound) = G2 Level A, chính là bước cần board RK3588 — đây là mắt xích nối G2 sang G3.
Desktop đầy đủ (compositor + mouse, windowed) — 📋; VFS scale (ext4/large disk) — 📋; 
App Platform Layers §J: L1 SDK ✅, còn L0 docs / L2 middleware / L3 tooling / L4 observability — 📋.
Cell-to-Cell Anywhere phần G2 (P04-P08: HyParView, hole-punch, K2/K3 DICE)