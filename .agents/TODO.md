# TODO


## Canonical Capability Routing

Product stages are release overlays, not a global work queue. Execute a lane
only at its documented evidence ceiling; QEMU/host evidence does not become
physical, secure-root, cloud, or production evidence.

| Lane | Execution class | Next action / reopening event |
|---|---|---|
| RPi3 HDMI software and exact-device boundary | `scope-gated` | Completed/regression-only on RPi3-B; reopen only for a regression or separately governed production qualification |
| Tier 3 hostile QEMU evidence | `scope-gated` | Add VMM/VirtIO transport for bounds, descriptors, and backend errors, plus independent preemption and supervisor-restart outcomes |
| ARM64 persistent storage | `scope-gated` | Add supported Phase 06 scenarios; the fixed 8 MiB `build/tier3-arm64-persistent.img` policy is approved |
| Desktop/ViUI/SDK | `scope-gated` | F1 signing governance is restored; resume the owning QEMU input/scanout slice |
| Local Cell-to-Cell | `scope-gated` | Implement the approved ephemeral K1 injection for the RV64 `app-bench` oracle, then record IPC, queue/cache, and saturation baselines |
| Kernel security/PAL | `governance-gated` | Obtain named approvals and implementation checkpoints |
| Authenticated evidence | `scope-gated` | Run and offline-verify the immutable `.github/workflows/ci.yml` bundle; only approved software/QEMU classes are eligible |
| x86 VirtIO parity | `scope-gated` | Add supported Phase 06 hostile scenarios, then pin one transport after the shared persistence backend |
| G3 accelerator | `external-gated` | Obtain RK3588, accepted RKNN package/license, then X390 evidence |
| Protected relay / production root | `external-gated` | Obtain named hardware/cloud assets; ADR-0006 requires a superseding GO ADR |
1. [in-progress] RPi3:
    - SD Storage và HDMI trên RPi3-B [completed]; I2C/SPI [in-progress]
    - Phase 05: Gỡ nghẽn USB Policy v3 & Level IRQ 9
    - cần sensor như SHT3x hoặc MPU6050
2. [in-progress] RISC-V/x86 Board: Bringup thực tế trên VF2, Pioneer, MiniPC
3. [blocked] SDK relay client mutual TLS: phụ thuộc production KMS/Silo identity lifecycle.
 
4. [blocked] App Tiers completion: cần phần cứng (RPi4b + secure controller riêng hoặc secure boot + remote CAS service)
    - Tier 1 baseline
    - Tier 1 rust std: PAL-019, PAL-031
    - Tier 3

5. [in-progress] Chuẩn hóa manifest và tooling phía người phát triển. Về lâu dài cần tách rõ:
    - execution_tier: Tier 1/2/3.
    - runtime_profile: Rust, FFI/POSIX, Lua, Linux guest.
    - protection_class: trường tương thích hiện dùng cho PKU/floor.
    - capabilities: quyền thực tế.
    - admission evidence: chữ ký, provenance, owner authorization.

Manifest v2 và tooling tương thích đã [done]. Việc đổi field vật lý [blocked], chờ Manifest v3 và phê duyệt ABI riêng.

6. [done] Xây acceptance matrix chung. Mỗi tổ hợp có trạng thái PASS, BLOCKED, PLANNED:
    - Tier × runtime profile.
    - Kiến trúc CPU.
    - QEMU/KVM/phần cứng thật.
    - Signed/unsigned/admission mode.
    - IPC/grant/MMIO/DMA.
    - SDK module.
    - Build, boot, restart và security-negative tests.

7. [in-progress] Cổng hoàn tất cuối cùng, App tiers chỉ nên được coi là hoàn thiện khi:
    - Không còn dùng Tier 1b, Tier 3b, SDK L1/L2 ngoài compatibility/historical text.
    - Manifest terminology không còn đụng với application tier.
    - Tier 1 có baseline và production admission rõ ràng.
    - Tier 3 có ít nhất một lane hardware-qualified.
    - Tier 2 chỉ được công bố khi private-domain containment đã có negative evidence.
    - SDK có module/profile matrix và examples khớp code.

8. [blocked] AI inference server demo (HTTP → NPU cell → response, P99 bound) = G2 Level A, chính là bước cần board RK3588 — đây là mắt xích nối G2 sang G3.

9. [in-progress] Desktop compositor & ViUI:
    - [done] Scope bounded đã phê duyệt: exact clipped damage, một `ManagedSurfaceApp`
      xử lý configure/minimize/restore/close, và `viui-demo` Counter chạy như managed surface.
    - [done] Focused tests, compositor regressions, RISC-V build và scope guard `libs/api`.
    - [blocked] QEMU runtime evidence: `run.ps1` invoked the repository-owned
      generator, which refused F1 signing until the Hypha unsafe prohibition
      and reviewed BCM unsafe allowlist entry are restored.

10. [in-progress] Test board thật: RISC-V và mini pc x86 (Dell)

11. [blocked] Phần cứng (StarFive VisionFive 2 v1.3B, STM32H573I-DK Discovery Kit của STMicroelectronics, Infineon OPTIGA™ TPM 2.0 SLB9672 kit) và AWS DEV account/region để unlock KMS Silo

### App Layers
1. **Tier 1** - Trusted Native SAS Cell
**Profile:**
    - Rust no-std: hiện đã hoạt động, dùng core + alloc + ostd.
    - Rust std: mục tiêu G4 tương lai, vẫn là Tier 1; dùng pure-Rust PAL
    - FFI: C/C++ freestanding, Zig native, POSIX shim, mlibc, Rust có FFI, Lua VM viết bằng C, Vendor SDK như RKNN/Hailo/codec libraries.
    - Lua
**Lưu ý:**
    - C/FFI không được Rust LBI bảo vệ. Vì nó vẫn chạy trong SAS nên code này phải được tin cậy. PKU/MTE nếu có chỉ là defense-in-depth, không biến nó thành sandbox portable trên mọi kiến trúc.
    - mlibc chưa hoàn tất: Checkout hiện không có third_party/mlibc/build*/libc.a. Thiếu malloc, printf, free, clock_gettime.

2. **Tier 2** - Native Domain Cell
    - unsigned/unverify/untrusted tier 1
    - arbitrary native ELF

3. **Tier 3** - Virtual Machine - VM
    - Gate “nginx chạy thật trong Linux VM” chưa được xác minh
    - Storage hiện là RAM disk nhỏ, volatile: VFS scale (ext4/large disk)
    - Intel VMX chưa có VMCS/world-switch hoàn chỉnh.
    - RunVcpu cần enforcement budget đáng tin cậy trước khi gọi workload bên thứ ba là production-safe.
    - Boot-to-shell ARM64 nghiêm ngặt vẫn cần KVM/real hardware; QEMU TCG chỉ là machinery evidence.
    - x86 host shell pre-GUI: PASS trên QEMU-TCG (`HV_SMOKE_MODE=host-shell`, follow-up của `9d8e5eab`); lỗi `ReadDir` EOF làm kẹt probe `/bin/*` đã được sửa.
    - x86 Linux guest strict boot: PASS trên QEMU-TCG 10.2.0 ở 1 GiB và 2 GiB (`Linux 6.12.81` → `/bin/sh` → `~ #`); QEMU-TCG 8.2.2 vẫn BLOCKED bởi `CELLOS-HV-X86-TCG-001`.
    - Dùng ARM64 làm đường ngắn nhất để đóng gate: Alpine → nginx → HTTP request/response có log.
    - x86 hiện boot bằng initramfs nhưng personality chưa nối VirtIO MMIO/block/network dùng chung; các module đó vẫn chỉ bật cho AArch64
    - persistent disk, Ubuntu/glibc và các lane AMD/Intel hardware

4. Cellos **Native SDK**:
    - Tier 1 và Tier 2 nên dùng cùng API nguồn càng nhiều càng tốt. Khác biệt nằm ở target/deployment profile:
    - Tier 1 cho phép SAS zero-copy grants.
    - Tier 2 dùng domain-safe IPC và explicit mapped grants.
    - SDK có thể từ chối API không hợp lệ theo target profile tại compile time.
    - family chia theo module/layer và target profile:
```
Cellos Native SDK
├── Native SDK Core
│   ├── ABI, manifest, lifecycle
│   ├── capabilities
│   ├── IPC, Grant
│   └── low-level surface/display client
│
├── Cellos Middleware
│   ├── VFS, network, service discovery
│   ├── AppContext
│   └── UI / ViUI
│       ├── Signal
│       ├── widgets
│       ├── layout
│       ├── navigation
│       └── rendering facade
│
├── Developer Tooling
│   ├── build/package/signing
│   ├── templates
│   ├── manifest validation
│   └── .vi compiler/code generation
│
└── Operations / Observability
    ├── logging
    ├── metrics and frame timing
    ├── tracing
    ├── health/watchdog
    └── crash and UI diagnostics
```

| SDK module | rust-no-std | rust-std | ffi-posix | Lua |
|---|---:|---:|---:|---:|
| Core ABI/IPC | Có | Dự kiến | Qua C ABI | Qua binding |
| VFS/network | Có | Dự kiến | POSIX mapping | Binding hạn chế |
| ViUI | Có | Dự kiến dùng lại | Không ưu tiên | Có thể binding |
| `.vi` tooling | Có | Có thể dùng lại | Không trực tiếp | Không trực tiếp |
| Observability | Có | Dự kiến | Qua ABI | Qua runtime |

 
### Cách đặt tên
    - **Tier**: cấp thực thi/cô lập ứng dụng — khác nhau về trust boundary, page table, IPC và chi phí.
    - **Profile**: ngôn ngữ hoặc runtime trong một tier — Rust no_std, Rust std, C/POSIX, Lua…
    - **Layer**: lớp cấu trúc phần mềm — SDK Core, Service Clients, Middleware, Tooling; hoặc Hardware Isolation Layer A/B/C.
    - **Stage G1–G5**: giai đoạn sản phẩm/roadmap, hoàn toàn độc lập với app tier.


# BLOCKERS
1. Port Drivers - phase 06:
    - cần board RK3588 (SoC ARM của Rockchip: 4× Cortex-A76 + 4× Cortex-A55, GPU Mali-G610 và NPU 3 lõi khoảng 6 TOPS. Board Radxa ROCK 5B 8 GB hoặc 16 GB) để boot và giữ lại UART log 
    - chốt phiên bản RKNN SDK/runtime, giấy phép và quyền phân phối firmware/binary.
    - Chưa chạy inference thật: load model → input → run → output → cleanup và các đường lỗi.
    - Chưa chứng minh buffer/DMA/cache lifecycle, đặc biệt quyền sở hữu IOMMU/SMMU của NPU.
    - Chưa có P50/P95/P99, memory-pressure, restart/fault-injection và kiểm tra stale DMA.
    - Chưa có phần cứng X390 (SiFive Intelligence X390 Gen 2 là RISC-V processor IP có vector engine RVV 1.0 512-bit, có thể ghép accelerator qua SSCI/VCIX) để làm implementation thứ hai, bảo đảm ABI chung không bị đóng khung theo RKNN.
