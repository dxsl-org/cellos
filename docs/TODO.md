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
Bước đầu tiên đáng làm: `scripts/qemu-hypervisor-smoke.sh` ở nhánh lỗi này chỉ in các dòng
fault đã grep, không bao giờ in `qemu-hv.log` — nên nhìn output CI không biết được chuyện gì
xảy ra trước khi Cell 2 abort. Sửa chỗ đó trước khi điều tra.

Gate graduation "nginx chạy thật trong Linux VM" — chưa verify.
AI inference server demo (HTTP → NPU cell → response, P99 bound) = G2 Level A, chính là bước cần board RK3588 — đây là mắt xích nối G2 sang G3.
Desktop đầy đủ (compositor + mouse, windowed) — 📋; VFS scale (ext4/large disk) — 📋; 
App Platform Layers §J: L1 SDK ✅, còn L0 docs / L2 middleware / L3 tooling / L4 observability — 📋.
Cell-to-Cell Anywhere phần G2 (P04-P08: HyParView, hole-punch, K2/K3 DICE)