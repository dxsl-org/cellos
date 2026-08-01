# Cellos Architecture: Graphics & Input
**Version**: 0.4 (Zero-Cost Compositing, Low-Latency Input & SAS Security Boundary)
**Status**: Definitive
**Amended 2026-08-01**: D11 replaces the false cross-cell page-fault/`Poisoned` claim with the implemented capability, LBI, and MMU boundaries.

---

## 1. Triết lý Đồ họa: Shared Framebuffer
Trong Cellos SAS, chúng ta loại bỏ hoàn toàn việc copy buffer giữa Client và Server (như X11/Wayland).

### Quy trình hiển thị Zero-Copy
1. **Compositor Cell**: Nắm giữ con trỏ đến **Physical Framebuffer** do phần cứng cung cấp.
2. **App Cells**: Vẽ vào các vùng nhớ riêng gọi là **Surface**.
3. **Compositing**: 
    * Thay vì copy toàn bộ, Compositor chỉ thực hiện `memcpy` các vùng dữ liệu bị thay đổi (Damaged regions).
    * **Game/Full-screen Mode**: Compositor chuyển nhượng trực tiếp quyền sở hữu vùng nhớ Framebuffer cho App Cell thông qua Capability. Đây là mức hiệu năng **True Zero-Copy**.

## 2. Hệ thống Input: Focus-Gated Dispatcher
Độ trễ input phải được đo bằng mục tiêu kiểm chứng được; không có đường `direct call` giữa các Cell.

* **Input Driver (Tier 1)**: Nhận ngắt (IRQ), giải mã thành `InputEvent` (Enum).
* **Dispatcher**: 
    * Nắm giữ danh sách các `Window` của các Cell.
    * Xác định Cell đang được focus.
    * Chuyển sự kiện qua cơ chế của [Spec 17 §6](17-ipc-wire-contract.md#6-blocking-discipline--the-input-queue): kernel-mediated try-send vào mailbox bounded của target focus, với drop/backpressure theo queue bounds và không gọi trực tiếp vào callback giữa các Cell.



## 3. Chế độ vận hành (Profiles)
Cellos cho phép cấu hình linh hoạt tùy theo mục đích sử dụng:

| Mode | Target | Đồ họa |
| :--- | :--- | :--- |
| **Mode 1: CLI** | Server / Robot Nano | Không GUI. Chỉ dùng Serial/VGA Driver cho Shell. |
| **Mode 2: Kiosk** | Industrial Panel / ATM | Full-screen cho một App duy nhất. Tối ưu Direct Scanout. |
| **Mode 3: Desktop** | Workstation | Hỗ trợ nhiều cửa sổ, Taskbar, Start Menu thông qua ViUI (xem §4; Slint đã bị loại — §4 note 2026-06-07). |

## 4. UI Toolkit: ViUI (Custom, Cellos-native)

> **Quyết định 2026-06-07**: Slint bị loại do GPL-3 viral / $1+/device commercial license không phù hợp cho một OS platform. iced bị loại do `iced_runtime` cần std. egui bị loại do tessellation pipeline không phù hợp với software renderer. ViUI được xây dựng từ đầu. Xem chi tiết: [specs/14-viui.md](14-viui.md).

ViUI là UI toolkit `no_std`-native của Cellos với:
* **Dual authoring layer**: Rust `ViNode` API + declarative `.vi` DSL (`vi_design!` inline hoặc `viui-build` lúc build), cùng tạo một Reactive Signal Tree.
* **Direct pixel rendering**: widget → app-owned pixel surface → DamageNotify. Comparative performance requires a checked-in benchmark artifact; no egui/iced compatibility or speed multiplier is promised.
* **Event-driven**: 0 CPU khi idle (retained mode + DamageNotify, không phải game loop).
* **Text**: Bitmap 8×8 cho CLI mode + `GlyphAtlas` + fontdue cho scalable Unicode text.
* **MIT license**: không viral, không per-device fee — safe cho toàn bộ Cellos ecosystem.

Mode 3 (Desktop) dùng ViUI thay vì Slint.

## 5. Bảo mật đồ họa trong SAS
Tier 1 dùng một page table chung, nên bảo vệ surface có hai lớp khác nhau:

* **API capability + sender identity**: Compositor chỉ cho creator của surface thực hiện
  attach, damage, move, raise, detach và destroy. `GrantShare`/`GrantSlice` kiểm tra owner
  và grantee trước khi trả con trỏ qua syscall.
* **LBI + signed-cell trust boundary**: buffer surface dùng Grant identity-mapped
  `USER+RW`. `GrantPerm::ReadOnly` là contract phần mềm cho Compositor, không phải PTE
  read-only riêng theo Cell. Vì vậy một Cell có khả năng tạo con trỏ tùy ý có thể truy cập
  data page của Cell khác mà không gây page fault; untrusted native code phải chạy ở Tier 2
  khi per-domain page tables hoàn tất, hoặc Tier 3 VM.

Page fault chỉ xảy ra khi quyền PTE thật sự từ chối truy cập: ghi vào `.text`/`.rodata`
đã được W^X hạ quyền, guard page, hoặc địa chỉ unmapped. Kernel terminate task vi phạm,
thu hồi resource và chuyển nó qua zombie/reap; runtime hiện không đặt trạng thái
`CellState::Poisoned`.

Xem [Spec 19](19-hardware-isolation-layers.md) cho ranh giới Layer A hiện tại và Layer B
per-domain page tables.
