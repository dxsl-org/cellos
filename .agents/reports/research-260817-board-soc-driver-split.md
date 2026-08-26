## Scope Guard
**Verdict:** Pass này chỉ chốt chiến lược và lát cắt di trú an toàn; không nên “triển khai toàn bộ” một lần.
- Handoff kiến trúc yêu cầu đọc theo thứ tự và tiếp tục từ lane DTB/board facts trước khi mở rộng thay đổi cấu trúc lớn.
- Quy tắc codebase chặn thay ABI `libs/api`/`libs/types`; refactor board/SoC phải tránh lan sang interface ổn định.
**Source:** .agents/reports/HANDOFF-260731.md:3-5,160-167; docs/code-standards.md:12-20

## Boards Belong At Root, Not Under HAL
**Verdict:** `boards/` phải là lớp tích hợp sản phẩm ở root; nhét nó vào `hal/` sẽ trộn board policy với cơ chế phần cứng.
- `docs/system-architecture.md` tách Cells / Kernel / HAL / Hardware; board packaging không nằm trong contract của HAL.
- Workspace hiện đã tách `hal/*` cho cơ chế phần cứng và `cells/drivers/*` cho Driver Cells; thêm `boards/*` ở root khớp pattern hiện có hơn là nhét vào `hal/`.
- `kernel/Cargo.toml` đang biểu diễn board bằng feature cục bộ; đó là chỗ nên được thay bằng board descriptor, không phải mở rộng trách nhiệm HAL.
**Source:** docs/system-architecture.md:29-40; Cargo.toml:33-70; kernel/Cargo.toml:83-100

## Driver Duplication Is Not The Real Problem
**Verdict:** Shared driver không cần dời sang `boards/`; thứ đang phình là board policy rò vào boot/platform/HAL feature flags.
- `cells/drivers/*` đã là Driver Cells dùng chung, không phải bản sao theo board.
- UART kernel driver đọc base động từ `platform::with`, cho thấy cơ chế driver đã có thể dùng descriptor thay vì fork theo board.
- Rò cấu hình hiện nằm ở `kernel/src/platform.rs`, `kernel/src/boot.rs`, `kernel/build.rs`, `kernel/src/task/drivers/mmc.rs`, và `hal/core` feature propagation.
**Source:** Cargo.toml:53-70; kernel/src/task/drivers/uart.rs:139-154; kernel/src/platform.rs:115-281; kernel/src/boot.rs:240-516; kernel/build.rs:10-25; kernel/src/task/drivers/mmc.rs:16-33,112-141; hal/core/Cargo.toml:33-41

## Current Leak Map
**Verdict:** Board knowledge đang bị hardcode ở 4 lớp: build/link, boot fallback, platform discovery, và one-off peripheral quirks.
- `board-rpi3` đổi linker script ở build time.
- `boot.rs` giữ fallback memory map riêng cho QEMU RV64, VF2, RPi3, AArch64 virt.
- `platform.rs` hardcode QEMU virt defaults, Pioneer overrides, RPi3 defaults, AArch64 virt defaults trong cùng file.
- `mmc.rs` chọn SDHCI base/pinmux bằng `#[cfg(feature = ...)]`; `hal/core` còn propagate `board-rpi3` xuống `hal-arm`.
**Source:** kernel/build.rs:10-25; kernel/src/boot.rs:240-516; kernel/src/platform.rs:14-209; kernel/src/task/drivers/mmc.rs:3-33,117-141; hal/core/Cargo.toml:40-41; hal/arch/arm/Cargo.toml:25-27

## Minimal Slice To Establish The Architecture
**Verdict:** Lát cắt đầu tiên nên chỉ đưa vào `boards/` metadata + chuyển QEMU RV64 sang descriptor-consumer; chưa tạo `hal/soc` đầy đủ và chưa di chuyển shared driver.
- QEMU RV64 là lane CI boot gate thật, có DTB parser và fallback map rõ ràng; đổi lane này trước cho feedback nhanh nhất.
- VF2 và Pioneer hiện chủ yếu khác QEMU RV64 ở fallback DRAM/console contract, nên descriptor schema có thể cover chúng sau mà chưa cần driver refactor.
- RPi3 kéo theo linker script riêng, HAL feature leak, pinmux và SDHCI quirks; nếu làm trước sẽ biến migration thành nhiều biến cùng lúc.
**Source:** .github/workflows/ci.yml:249-273; kernel/src/boot.rs:240-307; docs/vf2-bringup.md:59-64,147-149; docs/pioneer-bringup.md:34-50,79-83,163-170

## Recommended Order
**Verdict:** Xếp hạng triển khai: `boards/` descriptor root first, `hal/soc` second, Cargo feature collapse last.
- **#1 Root `boards/` descriptors:** tạo schema chứa identity, compatibles, boot contract, fallback memory map, pinmux/PHY wiring, driver enable list; chỉ kernel `boot/platform` đọc schema.
- **#2 `hal/soc/` extraction:** kéo phần SoC glue thật sự ra khỏi `platform.rs`/`mmc.rs` sau khi board descriptor đã tồn tại; ưu tiên BCM2837, JH7110, SG2042.
- **#3 Feature/build cleanup:** thay `board-*` linker/build toggles và HAL propagation bằng build target chọn board descriptor; khi đó mới gỡ `hal/core` feature `board-rpi3`.
- **Avoid:** đặt `boards/` trong `hal/` hoặc sao chép UART/SDHCI/GIC/PLIC/PCIe thành driver riêng theo board.
**Source:** kernel/Cargo.toml:83-102; kernel/src/platform.rs:119-209; kernel/src/task/drivers/mmc.rs:16-33,112-141; hal/core/Cargo.toml:33-41

## Validation Lane
**Verdict:** Gate tối thiểu sau mỗi bước phải là compile + boot thật trên QEMU RV64 và AArch64; board thật giữ compile/image generation smoke trước.
- RV64 boot gate chính thức: `bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`.
- AArch64 boot gate chính thức: build cells, tạo `kernel_fs.img`, rồi `BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh`.
- CI hiện không có boot lane cho VF2/Pioneer; với hai board này chỉ nên thêm compile/image smoke trước (`scripts/vf2-build.ps1`, `scripts/pioneer-build.ps1`) rồi mới tính hardware CI.
**Source:** .github/workflows/ci.yml:169-183,249-385; scripts/qemu-boot-test.sh:13-79; scripts/qemu-aarch64-test.sh:7-73; scripts/vf2-build.ps1:19-30; scripts/pioneer-build.ps1:18-29

## Rollback Risk
**Verdict:** Rủi ro lớn nhất không phải driver mà là boot regressions do lệch linker/fallback map/embedded image assembly.
- RPi3 phụ thuộc linker script riêng `kernel/linker-rpi3.ld`; đổi cách chọn board mà quên lane này sẽ brick boot sớm.
- AArch64 virt có fallback RAM sizing runtime + `qemu-virt-1g`; sai descriptor có thể phá hypervisor lane dù compile vẫn xanh.
- `gen_disk`/embedded image assembly là phần build thành công nhưng boot hỏng; QEMU boot scripts phải giữ là gate bắt buộc.
**Source:** kernel/build.rs:10-25; kernel/src/boot.rs:361-470; .github/workflows/ci.yml:342-377,456-476; scripts/qemu-boot-test.sh:8-12,50-79

## Trade-off Matrix
**Verdict:** Chọn phương án A.
- **A. root `boards/` + kernel consumers first**: fit 5/5, churn 3/5, rollback 4/5, CI leverage 5/5.
- **B. `hal/boards` under HAL**: fit 2/5, churn 3/5, rollback 3/5, CI leverage 3/5.
- **C. keep feature-based status quo and patch per board**: fit 1/5, churn 5/5 short-term, rollback 5/5, CI leverage 1/5.
**Source:** docs/system-architecture.md:29-40; kernel/Cargo.toml:83-102; hal/core/Cargo.toml:33-41

## Limitations
**Verdict:** Đánh giá này chưa cover mọi file runtime/hardware lane và chưa chứng minh schema cuối cùng cho x86_64/ACPI.
- Chưa đọc hết toàn bộ `main.rs`/paging/hypervisor paths; recommendation chỉ nhắm slice board/SoC boot path đầu tiên.
- CI hiện xác thực mạnh cho QEMU RV64/AArch64, yếu cho VF2/Pioneer; phần “build lại bất kỳ lúc nào” trên board thật vẫn cần thêm lane sau migration đầu.
**Source:** kernel/src/main.rs:105-112,541-591; .github/workflows/ci.yml:249-478
