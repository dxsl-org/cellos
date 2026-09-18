# TODO
- CI gate restoration [completed 2026-09-15]: toàn bộ pipeline CI 23/23 jobs đã XANH
  hoàn toàn trên hosted runner (GitHub Actions Run `34964009461` tại commit `2bb82e50a`).
  Toàn bộ 7 job đỏ ban đầu đã được giải quyết:
  1. `Lint (fmt + clippy)`, `Clippy (x86_64)`, `Clippy (aarch64)`: sạch nLOC metrics, formatting và clippy (phase-01 & fix).
  2. `CellosFS /srv Integration Test`: sửa assertion cũ (phase-02) và sửa dứt điểm kernel fault qua trap-root discipline (phase-04).
  3. `C2C Broker Oracle`: ký dev cells trong `run-c2c-broker-oracle-qemu.sh` để driver được nhận vào Tier 1 SAS thay vì Tier 2 Paged Domain thiếu MMIO (phase-05).
  4. `Network Data-Path Integration (riscv64)`: bổ sung hỗ trợ tham số CLI `<port> [path]` cho `service-httpd` (phase-05), toàn bộ 54/54 test pass.
  5. `QEMU Hypervisor Boot-to-Shell (x86_64)`: khắc phục OOM crash loop của hypervisor cell với 10 MiB custom heap, thêm `pci=off`, thêm cơ chế thoát sớm khi thấy prompt (phase-05).
  Kế hoạch và bằng chứng lưu tại `.agents/260914-ci-gate-restoration/`.
- Phase 07 authenticated software evidence [completed/regression-only] tại trần
  `host`: GitHub-hosted run `33251921677:1` đã được verify, consume qua durable
  operator-owned replay state và replay chính xác bị từ chối. Mở lại chỉ khi
  pipeline/schema/workflow identity regression hoặc có higher-evidence scope
  được phê duyệt; không nâng bất kỳ claim nào lên physical/production.
- Phase 10 x86 Tier 3 VirtIO software đã [completed] tại trần QEMU: hai boot
  block/network và 27 hostile scenarios, gồm malformed transport/queue,
  pause-less vCPU preemption, VFS/Net supervisor restart và backend recovery.
- Phase 06 còn [blocked] riêng trên ARM64 vì synchronous TCG fault xảy ra trước
  guest probe; không hạ gate hoặc suy diễn PASS từ kết quả x86.
- Qualification AMD/Intel thật vẫn là hardware gate độc lập.
- App-tier acceptance ledger [completed 2026-09-15]: schema v5 bind `source` evidence vào revision
  đã lưu trữ theo digest (contract + mirror cho mọi file nguồn khác), thay vì file sống có thể
  amendment; migration v4→v5 re-base `source_binding` sang contract đã amendment tại `81dbb81c`
  (chỉ prose/witness C2-MID, matrix digest không đổi) và archive cả hai revision. Blocker
  `B-AARCH64-SEMHOSTING` được resolve lại bằng lần chạy QEMU semihosting thật (RC 0, vfs-test
  96 PASS / 0 FAIL), TTL nâng từ 2 ngày lên 30 ngày (hết hạn 2026-10-15) vì TTL 2 ngày làm gate đỏ
  theo cấu trúc. Phase 04 ghi `IMPLEMENTED` với artifact prequalification đã pin; `c9` vẫn
  NOT_COMPLETE và cả ba blocker vẫn BLOCKED. Lưu ý: chuỗi append-only chỉ hồi phục được sau một
  push trung gian đỏ (baseline `994c0b01` invalid tại tip của chính nó) — xem
  `.agents/logs/260915-app-tier-ledger-schema-v5.md`.
- Ngoài lane này còn các việc lớn như x86 Platform Cell discovery [completed], kernel-owned x86 paging, per-vector IDT stubs [completed] và production
  trust keys.
- Phase 04 two-node/direct-LAN vẫn được ghi blocked riêng theo authority mTLS; không chặn single-guest gate.
- Tier 1 Rust `std` PAL [completed 2026-09-18]: hoàn tất lõi PAL thuần Rust và target specs
  `*-unknown-cellos` cho cả 3 kiến trúc (riscv64, aarch64, x86_64). Đã hiện thực bộ cấp phát
  heap tự do có ghép khối liền kề (4 MiB boundary-tag coalescing allocator), phục hồi tham số dòng
  lệnh qua `ViSyscall::StateRestore` (`ARGV_STASH_KEY`), đơn điệu `Instant::now()`, điều phối
  `yield_now()`, stdout log và các API không cấp quyền fail-closed. Đã kiểm chứng stress test
  1.000 chu kỳ alloc/dealloc không rò rỉ RAM, parity benchmark p99 regression <= 5%, và boot
  end-to-end PASS trên QEMU (`scripts/run-std-smoke-qemu.sh`).
- Tier 2 fail-closed security & hardware containment [completed 2026-09-18]: xóa bỏ hoàn toàn
  cơ chế fallback ngầm sang SAS trong `kernel/src/task/launch.rs`; mã unsigned hoặc FFI khi tạo
  domain thất bại hoặc thiếu metadata/không hỗ trợ kiến trúc bắt buộc fail-closed (`OutOfMemory`,
  `InvalidInput`, `NotSupported`), bảo đảm bất biến Spec 22 không bao giờ để mã unverified chạy
  trần trong SAS. Khắc phục lazy re-probe cho `EarlyLoader` trong `kernel/src/loader/early.rs` khi
  thiết bị khối sẵn sàng sau boot. Đã kiểm chứng 100% PASS integration test `tier2_fault_isolation.rs`
  trên QEMU (bắt bẫy page fault NULL-write từ `/bin/tier2-exploit`, kết liễu an toàn, shell tiếp tục sống sót).

- Tier 3 hypervisor signing pipeline & vCPU preemption quantum yield [completed 2026-09-18]:
  nâng cấp `scripts/lib-sign-cells.sh` tự động chọn đúng cross-objcopy theo kiến trúc ELF
  (AArch64, RISC-V, x86_64), tích hợp F1/F5 signing vào `make-hypervisor-fs.sh` và `make-hypervisor-fs-x86.sh`
  giải quyết triệt để lỗi từ chối cell trên AArch64; bổ sung `ostd::task::yield_now()` tường minh
  khi `ViVmExit::Preempted` trong `run_loop.rs` và `run_loop_x86.rs` ngăn chặn vCPU độc quyền thời gian CPU host;
  kiểm chứng 100% PASS QEMU AArch64 machinery smoke test (`scripts/qemu-hypervisor-smoke.sh`).
1. Chọn floor backend + production hardware
2. Security review thiết kế floor/A-B protocol
3. [done 2026-09-16] Fix loader signature boundary (CELLOS-LOADER-SIG-001): xác thực toàn bộ metadata container ELF, .rela.dyn và section headers trước khi cấp phát/relocation; kiểm thử âm bản trên host (test-cell-signing.sh) và boot QEMU (elf_tests)
4. [done 2026-09-16] Implement floor persistence/recovery: định dạng nhị phân A/B slot record (`VI_OWNER_SLOT_V1`), parser fail-closed kiểm tra chữ ký trước khi parse, kiểm thử âm bản tamper và tích hợp với `AdmissionDecision::decide()`
5. [done 2026-09-16] Provision publisher/owner anchors: bổ sung anchor thứ 3 `OWNER_SIGNER_PUBKEY` độc lập với fleet policy và publisher key, hàm `verify_owner_signature` và self-test khởi động
6. [done 2026-09-16] Wire mọi task-creation path: tính SHA-256 toàn bộ ELF một lần tại đầu spawn_gated, kết nối kiểm tra evaluate_owner_admission trước khi cấp phát/tạo task và tái sử dụng digest cho measurement
7. Thiết lập production-admission authenticated runner; software-only authenticated evidence runner đã hoàn tất riêng ở Phase 07
8. Chạy hostile + physical power-loss matrix
9. Retain immutable evidence bundle
10. [done 2026-09-16] Hai umbrella approvals: Security Owner và Independent Reviewer phê duyệt Spec 18c (Publisher Provenance Envelope Contract) và Umbrella Phase 03 baseline design
11. [done 2026-09-16] Sáu PAL approvals trên cùng manifest digest (`99cf7d24cd14c3b862959d17b499053735bbefded850202fa72b9eb8509129b3`) đã được phê duyệt và ghi nhận
12. [done 2026-09-16] Release/ledger: ghi nhận chuyển đổi Phase 03 sang IMPLEMENTED (event `phase-03-implemented`, sequence 11) trong `docs/app-tier-acceptance-ledger.json`, `validate-app-tier-acceptance.py` đạt PASS
13. [done 2026-09-16] Mở PAL-IMPLEMENTATION-CHECKPOINT: cả 6 điều kiện tiên quyết đã thỏa mãn, chuyển trạng thái sang UNBLOCKED / CONDITIONAL GO

14. [in-progress] RPi3:
    - SD Storage và HDMI trên RPi3-B [completed]
    - I2C/SPI: Controller BCM BSC1 và SPI0 loopback [completed trên board thật]; cần sensor vật lý (SHT3x/MPU6050) để đọc dữ liệu cảm biến trực tiếp
    - USB DWC2 & LAN9514 (Phase 05): Logic USB Policy v3, cấp quyền DWC2 MMIO và One-shot Level IRQ 9 đã [gỡ nghẽn 100% trong mã nguồn]; chờ cắm cáp Ethernet kiểm thử thực địa
15. [in-progress] RISC-V/x86 Board: Bringup thực tế trên VF2, Pioneer, MiniPC
16. [blocked] SDK relay client mutual TLS: only this two-real-broker relay path is blocked by the protected-persistence, authenticated-time, and reviewed pending-key-binding entry gates under frozen KMS opcodes 9–14 in `.agents/260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md`; reopen only when DEV_REFERENCE Phase 8 emits exact `GO: PHASE4_ENTRY_GATES_SATISFIED`. This is not a global blocker for single-guest or other approved local work; the attempted protocol scaffold was fully reverted after governance review, with no dead or unwired implementation remaining.
 
17. [blocked] App Tiers completion: cần phần cứng (RPi4b + secure controller riêng hoặc secure boot + remote CAS service)
    - Tier 1 baseline [completed 2026-09-16, event phase-03-implemented]
    - Tier 1 rust std [completed 2026-09-16]: PAL in-tree implementation (custom targets, sysroot overlay, PAL-019/031, workload parity PASS)
    - Tier 3

18. [in-progress] Chuẩn hóa manifest và tooling phía người phát triển. Về lâu dài cần tách rõ:
    - execution_tier: Tier 1/2/3.
    - runtime_profile: Rust, FFI/POSIX, Lua, Linux guest.
    - protection_class: trường tương thích hiện dùng cho PKU/floor.
    - capabilities: quyền thực tế.
    - admission evidence: chữ ký, provenance, owner authorization.

Manifest v2 và tooling tương thích đã [done]. Việc đổi field vật lý [blocked], chờ Manifest v3 và phê duyệt ABI riêng.

19. [done] Xây acceptance matrix chung. Mỗi tổ hợp có trạng thái PASS, BLOCKED, PLANNED:
    - Tier × runtime profile.
    - Kiến trúc CPU.
    - QEMU/KVM/phần cứng thật.
    - Signed/unsigned/admission mode.
    - IPC/grant/MMIO/DMA.
    - SDK module.
    - Build, boot, restart và security-negative tests.

20. [in-progress] Cổng hoàn tất cuối cùng, App tiers chỉ nên được coi là hoàn thiện khi:
    - Không còn dùng Tier 1b, Tier 3b, SDK L1/L2 ngoài compatibility/historical text.
    - Manifest terminology không còn đụng với application tier.
    - Tier 1 có baseline và production admission rõ ràng.
    - Tier 3 có ít nhất một lane hardware-qualified.
    - Tier 2 chỉ được công bố khi private-domain containment đã có negative evidence.
    - SDK có module/profile matrix và examples khớp code./

21. [partial] AI inference server demo = G2 Level A:
    - [done] Đường CPU (Spec 24 CP-1..CP-3): service `/bin/ai` (`service::AI = 15`) + engine GGUF/Q8_0/Rust, oracle QEMU PASS, model thật 30 layer chạy trên host (3.97 tok/s). Kế hoạch + bằng chứng: `.agents/260913-2002-g2-level-a-ai-inference/`.
    - [blocked] Phần NPU (RK3588) + P99 bound + front HTTP vẫn cần board RK3588 — mắt xích nối G2 sang G3.
    - [owed] Law 1 xác nhận 2 lần cho interface AI trước khi coi ABI là frozen.

22. [done 2026-09-18] Desktop compositor & ViUI:
    - [done] Scope bounded đã phê duyệt: exact clipped damage, một `ManagedSurfaceApp`
      xử lý configure/minimize/restore/close, và `viui-demo` Counter chạy như managed surface.
    - [done] Focused tests, compositor regressions, RISC-V build và scope guard `libs/api`.
    - [done] QEMU runtime evidence: 3/3 integration tests đều PASS trên QEMU (`viui-managed-surface` PASS 44.5s, `compositor-cursor` PASS 14.3s, `window-policy` PASS 63.1s); chính sách F1/F5 signing đạt chuẩn, unsafe allowlist hợp lệ.
23. [in-progress] Test board thật: RISC-V và mini pc x86 (Dell)

24. [blocked] Phần cứng (StarFive VisionFive 2 v1.3B, STM32H573I-DK Discovery Kit của STMicroelectronics, Infineon OPTIGA™ TPM 2.0 SLB9672 kit) và AWS DEV account/region để unlock KMS Silo

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
    - Storage guest: Đã mở rộng persistent disk `/mnt/sd/guest_disk.img` với dung lượng tùy biến (`--guest-disk-size`, mặc định 64M - 220M) và pre-format sẵn ext4 (`CELLOS_GUEST`) qua `scripts/format-disk-hv-arm.sh` (AArch64) và `scripts/build-ubuntu-wide-guest-x86.sh` (x86 3G ext4); VFS grant-addressed read/write mapping sạch sẽ.
    - Intel VMX chưa có VMCS/world-switch hoàn chỉnh.
    - RunVcpu scheduler preemption: Đã bổ sung `ostd::task::yield_now()` trong `ViVmExit::Preempted` ở cả `run_loop.rs` (AArch64) và `run_loop_x86.rs` (x86_64), đảm bảo hypervisor nhường thời gian CPU cho các cell khác (VFS, Net, Compositor) khi hết tick budget.
    - Pipeline ký cell cho hypervisor: Đã tích hợp `sign_cells` tự động nhận diện cross-objcopy (`aarch64`, `riscv64`, `x86_64`) vào `make-hypervisor-fs.sh` và `make-hypervisor-fs-x86.sh`, khắc phục lỗi cell unsigned bị kernel từ chối trên AArch64.
    - QEMU AArch64 machinery smoke: PASS (`PASS: machinery ran — VMM entered the guest; only the documented TCG address-size fault occurred`).
    - Boot-to-shell ARM64 nghiêm ngặt vẫn cần KVM/real hardware; QEMU TCG chỉ là machinery evidence.
    - x86 host shell pre-GUI: PASS trên QEMU-TCG (`HV_SMOKE_MODE=host-shell`, follow-up của `9d8e5eab`); lỗi `ReadDir` EOF làm kẹt probe `/bin/*` đã được sửa.
    - x86 Linux guest strict boot: PASS trên QEMU-TCG 10.2.0 ở 1 GiB và 2 GiB (`Linux 6.12.81` → `/bin/sh` → `~ #`); QEMU-TCG 8.2.2 vẫn BLOCKED bởi `CELLOS-HV-X86-TCG-001`.
    - x86 VirtIO MMIO/block/network: Đã hoàn tất kết nối trong Phase 10 với 27 hostile recovery scenarios.
    - Dùng ARM64 làm đường ngắn nhất để đóng gate: Alpine → nginx → HTTP request/response có log.
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
