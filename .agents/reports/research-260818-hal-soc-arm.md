## Immediate Slice Should Stay on RISC-V Profiles
**Verdict:** `hal/soc` nên mở bằng một lát cắt RISC-V nhỏ, không phải BCM27xx+MMC, vì RV64 đã có board descriptor + QEMU boot gate còn RPi3 vẫn là lane hardware-risk cao.
- `boards/qemu/virt-riscv64/board.rs` đã là nguồn sự thật cho compatibles, fallback RAM, UART/PLIC/CLINT/RTC và driver list; `kernel/src/board.rs` chỉ còn validate rồi cấp descriptor cho RV64 boot/platform.
- `kernel/src/platform.rs` phía RV64 đã đọc descriptor cho QEMU mặc định, nên phần còn rò chủ yếu là override Pioneer và VF2 fallback, không phải generic driver behavior.
- Research trước đó đã chốt thứ tự: `boards/` first, `hal/soc` second, feature-collapse last; và cảnh báo RPi3 làm trước sẽ kéo linker, HAL feature leak, pinmux, SDHCI quirk cùng lúc.
**Source:** boards/qemu/virt-riscv64/board.rs:6-115; kernel/src/board.rs:1-26; kernel/src/platform.rs:41-57,80-111; .agents/reports/research-260817-board-soc-driver-split.md:29-49

## BCM27xx+MMC Is Not a Small SoC Slice
**Verdict:** Ý tưởng “BCM27xx+MMC trước” thực chất là lát cắt đa-trục, không phải một extraction hẹp.
- `platform.rs` hardcode toàn bộ RPi3 defaults trong generic kernel path.
- `mmc.rs` chọn `SDHCI_BASE` và pinmux bằng `#[cfg(feature = "board-*")]`, nghĩa là policy board đang đi thẳng vào driver entrypoint.
- `sdhci.rs` nhúng access width 32-bit only, transfer-mode shadow, và write spacing dưới `board-rpi3`, nên generic SDHCI driver hiện chưa tách được controller quirk khỏi board feature.
- `hal/core` còn propagate `board-rpi3` xuống `hal-arm`; `hal/arch/arm/src/aarch64.rs` còn switch IRQ controller và UART path theo đúng board feature đó.
**Source:** kernel/src/platform.rs:113-131; kernel/src/task/drivers/mmc.rs:16-33,108-141; kernel/src/task/drivers/mmc/sdhci.rs:23-156; hal/core/Cargo.toml:25-41; hal/arch/arm/src/aarch64.rs:9-14,42-45,94-102; .agents/reports/research-260817-cellos-soc-board-layering.md:89-99

## RPi3 Still Couples SoC Extraction to Hardware-Only Regression Risk
**Verdict:** BCM27xx+MMC không có gate an toàn tương xứng với độ chạm; regressions sẽ xuất hiện ở boot/runtime thật trước khi CI nhìn thấy.
- `boot.rs` vẫn giữ fallback map riêng cho RPi3 trong generic kernel code; đổi SoC/MMC mà không xử lý boot contract song song sẽ để lại split-brain ownership.
- Research hiện hành ghi rõ RPi3 còn phụ thuộc linker riêng, pinmux riêng, SDHCI quirks riêng, và phần boot-success thật có thể hỏng dù compile xanh.
- Validation lane được khuyến nghị là QEMU RV64 + QEMU AArch64 boot; real-board chỉ nên là compile/image smoke trước. Với RPi3-specific MMC quirks, compile-only không đủ.
**Source:** kernel/src/boot.rs:347-370; .agents/reports/research-260817-board-soc-driver-split.md:33-56

## The Smaller RISC-V Profile Slice Has Clean Ownership
**Verdict:** Lát cắt kế tiếp nên gom nốt RISC-V “profile facts” vào SoC/profile helpers: Pioneer console restriction, VF2 fallback RAM, và các DT-compatible fallback rules.
- Pioneer override hiện chỉ sửa `uart_base`, `uart_irq`, `rtc_base`, `virtio_mmio` sau DT parse; đây là profile data, không phải driver algorithm.
- VF2 fallback map trong `boot.rs` là exact-board RAM contract còn sót lại ở generic kernel; tách nó không đụng SDHCI/UART/GIC behavior.
- QEMU RV64 descriptor path đã chứng minh mô hình “data first”; nối thêm SG2042/JH7110 profile helpers vào RV64 lane sẽ giữ blast radius ở boot/platform data thay vì controller code.
**Source:** kernel/src/platform.rs:81-97; kernel/src/boot.rs:291-318; boards/qemu/virt-riscv64/board.rs:18-115; .agents/reports/research-260817-cellos-soc-board-layering.md:118-149

## Recommended Immediate Scope
**Verdict:** Immediate = “RISC-V profile slice”; Deferred = “BCM27xx+MMC quirk extraction”.
- **#1 Immediate:** tạo `hal/soc/<riscv-profile>` hoặc helper tương đương cho SG2042/JH7110/QEMU-RV64 profile facts; move only fallback/platform data and DT-compatible glue.
- **#2 After that:** thêm board package/descriptors cho RPi3 trước, rồi mới tách BCM2837 facts ra `hal/soc/bcm27xx`.
- **#3 Only then:** đổi `sdhci.rs` sang typed quirk object selected by compatible, không selected by `board-rpi3`.
- **Do not do now:** chạm `mmc.rs` + `sdhci.rs` + `hal/core` + `hal/arch/arm` trong cùng pass khi chưa có AArch64 board descriptor và boot gate cho exact lane.
**Source:** .agents/reports/research-260817-board-soc-driver-split.md:36-49; .agents/reports/research-260817-cellos-soc-board-layering.md:118-135,199-209

## Ranked Comparison
**Verdict:** Xếp hạng cho pass ngay sau `c0096ade`/`9427482f`: RISC-V profile slice thắng rõ; BCM27xx+MMC phải defer.
- **1. Smaller RISC-V profile slice:** Architectural fit = high; adoption risk = low; test leverage = high; rollback cost = low.
- **2. BCM27xx+MMC slice:** Architectural fit = medium; adoption risk = high; test leverage = weak until exact AArch64 lane is added; rollback cost = medium-high because it spans generic driver + HAL facade + board boot facts.
- **Reject for now:** “do the full BCM27xx+MMC extraction immediately” because it violates the repo’s own staged migration rule and spends regression budget where evidence is weakest.
**Source:** kernel/src/platform.rs:80-131; kernel/src/boot.rs:291-370; kernel/src/task/drivers/mmc.rs:16-141; kernel/src/task/drivers/mmc/sdhci.rs:23-156; hal/core/Cargo.toml:40-41; hal/arch/arm/src/aarch64.rs:94-102

## Preconditions Before Any ARM SoC Pass
**Verdict:** ARM SoC work should stay deferred until three gates exist.
- RPi3 board descriptor or equivalent board-owned boot/pinmux contract, so `hal/soc` does not absorb board wiring.
- Exact AArch64 boot verification for the touched lane, not just compile checks.
- A narrow plan deciding whether BCM IRQ/timer/UART extraction from `hal/arch/arm` is in-scope together with MMC, because splitting only MMC leaves the main ARM board leak intact.
**Source:** .agents/reports/research-260817-cellos-soc-board-layering.md:120-135,152-154; hal/arch/arm/src/aarch64.rs:94-105; kernel/src/task/drivers/mmc.rs:117-141
