# TODO

Việc **chưa xong** và con trỏ tới bằng chứng. Nội dung đã đóng nằm ở `CHANGELOG.md` và
`.agents/<plan>/` — file này không chép lại chúng. Trạng thái thật do source/test quyết định,
không phải bởi dòng chữ ở đây.

## Đang mở — làm được ngay, không cần gì thêm
- **[2026-09-26] `libs/ostd/src/console.rs` là file chết.** Không có `mod console` trong
  `libs/ostd/src/lib.rs`; `print!`/`println!` trong đó không được declare và cell log qua
  `ostd::io::println`. Hoặc xoá, hoặc declare tử tế — không để file "trông như API".
- **[2026-09-26] `network_reaches_the_internet_by_name` chưa vào allowlist boot suite** — cố ý:
  test cần internet thật của host (SLIRP chuyển DNS/TCP của guest ra host) nên nó `SKIP` khi host
  không có mạng, và luật allowlist ("pass 2/2 lần chạy đầy đủ, không retry tới xanh") không nhận
  SKIP làm bằng chứng. Nếu muốn có coverage thật ở CI: chạy 2 lần liên tiếp và xác nhận nó *chạy*
  chứ không skip, rồi mới thêm. (`network_resolve_answers_hostname_through_the_service` đã vào
  allowlist và đang xanh.)
- **[2026-09-26] Host harness của `ostd`/`cellos-kernel` chỉ có giá trị nếu tiếp tục chạy.** Hai harness
  vừa được bật lại và đã vào job host unit tests (xem mục "Đã đóng"); đừng để drift lại — một import
  cũ đủ để cả harness `cellos-kernel` không build, và 6 test chunking của `ostd` đã lệch khỏi session
  mà không ai thấy trong lúc chúng không chạy.
- **[2026-09-26] ~170 `#[test]` trong `cells/` vẫn không chạy ở đâu** (đã gỡ được `service-vfs` 57 test — xem mục "Đã đóng"; gốc của nhóm này: crate bin không có lib + entry không được gate theo target). Sau khi 474 suite host-test được
  nối vào CI, phần còn lại gồm các crate chỉ có target bin (không có lib) nên `cargo test --target
  x86_64-unknown-linux-gnu` chết ở dep bare-metal — ví dụ `service-vfs` (57 test) cần `driver-disk`
  host-build được (`no global memory allocator`, `#[panic_handler]`, unwinding), `service-httpd` (15)
  cần `ai-sdk/ostd-transport` vốn chỉ có ở `target_os = "none"`, `app-shell` (18), `service-hypervisor`
  (14). Trong toàn bộ `cells/` chỉ có **2** module cầu selftest chạy trong guest (`file_handles::selftest`,
  `access::selftest` của `service-vfs`). Hai hướng, chọn theo từng module: (a) cho các dep bare-metal một
  shim `#[cfg(test)]` (std + allocator + panic handler) để crate host-test được, hoặc (b) chuyển tính chất
  cần kiểm vào mẫu in-guest selftest đã có. Số đo: `grep -rn "#\[test\]" cells/ | wc -l` = 438.
- **[2026-09-26] `cargo test -p app-wasm` (không `--lib`) abort.** Bin target là entry bare-metal gọi
  `sys_exit`, nên test harness thoát process trước khi libtest báo cáo; CI phải gọi `--lib` (3 test ở lib).
  Muốn `cargo test` mặc định chạy được thì bin cần tách cổng vào khỏi entry.
- [in-progress] **RPi3**: SD storage + HDMI [done]; I2C/SPI BSC1 + SPI0 loopback [done trên board
  thật] nhưng cần sensor vật lý (SHT3x/MPU6050) để đọc dữ liệu cảm biến; USB DWC2 & LAN9514 (Phase
  05) đã gỡ nghẽn 100% trong mã nguồn (USB Policy v3, cấp DWC2 MMIO, one-shot level IRQ 9) — chờ
  cắm cáp Ethernet để kiểm thử thực địa.
- [in-progress] **Bringup board thật**: RISC-V (StarFive VisionFive 2, Pioneer) và mini PC x86 (Dell).
  Qualification AMD/Intel thật là gate độc lập; không suy diễn từ QEMU.
- [in-progress] **Manifest & tooling phía developer** (item 18): Manifest v2 + tooling tương thích
  [done]; đổi field vật lý [blocked] chờ Manifest v3 + phê duyệt ABI riêng. Đích: tách rõ
  `execution_tier`, `runtime_profile`, `protection_class`, `capabilities`, `admission evidence`.
- [in-progress] **Cổng hoàn tất App tiers** (item 20): không còn dùng Tier 1b/3b/SDK L1/L2 ngoài
  compatibility; terminology manifest không đụng application tier; Tier 1 có baseline + production
  admission; Tier 3 có ít nhất một lane hardware-qualified; Tier 2 chỉ công bố khi private-domain
  containment có negative evidence; SDK có module/profile matrix + examples khớp code.
- [in-progress] **Floor backend + production admission** (items 1, 2, 7, 8, 9): chọn floor backend &
  hardware; security review thiết kế floor/A-B; production-admission authenticated runner (Phase 07
  mới có bản software-only); hostile + physical power-loss matrix; evidence bundle bất biến.
  Items 3–6 và 10–13 [done 2026-09-16] (loader signature boundary, A/B slot record, owner anchor,
  task-creation wiring, umbrella/PAL approvals, ledger event, PAL-IMPLEMENTATION-CHECKPOINT) —
  chi tiết trong `docs/app-tier-acceptance-ledger.json` và `.agents/logs/260915-app-tier-ledger-schema-v5.md`.
- [in-progress] **D5 scale profiles**: re-run baseline N=64/128/256/512 **có heavy cells resident**;
  image sharing; demand stacks. Chi tiết + số đo n=8–9: `docs/roadmap/beam-parity-backend-roadmap.md`
  §2.2–2.3.
- [in-progress] **Beam-parity B1/B2** (B0 [done]): B1 concurrency trong cell + cancellation (cần ADR
  cancellation); B2 cost/scale per-request (WIP-limited cùng D5).
- [in-progress] **Cell-native portability (ADR-0018/0019, phase 01–07 [done])**: blocker class D còn
  lại là port bên thứ ba cần `fork`/`exec` (process tree); C `__thread`/`thread_local` hoãn vì cần
  TLS runtime (loader expose `PT_TLS` + block per-thread + offset `initial-exec`).
- [open gap] **mlibc** chưa hoàn tất: checkout không có `third_party/mlibc/build*/libc.a`; thiếu
  `malloc`, `printf`, `free`, `clock_gettime`.
- [open gap] **Tier 3**: gate "nginx chạy thật trong Linux VM" chưa xác minh; Intel VMX chưa có
  VMCS/world-switch hoàn chỉnh. Boot-to-shell ARM64 nghiêm ngặt cần KVM/phần cứng thật — QEMU-TCG
  chỉ là machinery evidence.

## Blocked (chờ phần cứng hoặc governance)
- [blocked] **SDK relay client mutual TLS**: chỉ đường relay hai real-broker này bị chặn bởi các entry
  gate protected-persistence, authenticated-time và reviewed pending-key-binding dưới KMS opcodes
  9–14 đã freeze trong `.agents/260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md`;
  chỉ mở lại khi DEV_REFERENCE Phase 8 phát ra đúng `GO: PHASE4_ENTRY_GATES_SATISFIED`. Đây không
  phải blocker toàn cục cho single-guest hay việc local khác; scaffold protocol đã được revert hoàn
  toàn sau governance review, không còn code chết hay chưa nối.
- [blocked] **Phase 04 two-node/direct-LAN** theo authority mTLS; không chặn single-guest gate.
- [blocked] **Phase 06 trên ARM64**: synchronous TCG fault xảy ra trước guest probe; không hạ gate
  và không suy diễn PASS từ kết quả x86.
- [blocked] **App Tiers completion** (item 17): cần phần cứng (RPi4b + secure controller riêng, hoặc
  secure boot + remote CAS service). Tier 1 baseline [done], Tier 1 rust std [done]; Tier 3 [blocked].
- [blocked] **AI inference server demo = G2 Level A** (item 21): đường CPU [done] (`/bin/ai`,
  `service::AI = 15`, engine GGUF/Q8_0, oracle QEMU PASS, model 30 layer chạy trên host 3,97 tok/s);
  NPU RK3588 + P99 bound + front HTTP chờ board; **[owed]** Law 1 xác nhận 2 lần cho interface AI
  trước khi coi ABI frozen.
- [blocked] **Port Drivers phase 06**: cần board RK3588 (Radxa ROCK 5B) để boot và giữ UART log;
  chốt phiên bản/giấy phép/quyền phân phối RKNN SDK; chưa chạy inference thật (load → input → run →
  output → cleanup và các đường lỗi); chưa chứng minh buffer/DMA/cache lifecycle và quyền sở hữu
  IOMMU/SMMU của NPU; chưa có P50/P95/P99, memory-pressure, restart/fault-injection, stale DMA;
  chưa có X390 cho implementation thứ hai (ABI chung không bị đóng khung theo RKNN).
- [blocked] **Hardware + AWS cho KMS Silo**: StarFive VisionFive 2 v1.3B, STM32H573I-DK,
  Infineon OPTIGA TPM 2.0 SLB9672, và AWS DEV account/region.
- [blocked] **Port Drivers / floor**: x86 Platform Cell discovery [done], kernel-owned x86 paging
  [done], per-vector IDT stubs [done], production trust keys [chưa].

## Đã đóng — con trỏ, không phải việc còn lại
Desktop environment (2026-09-19) · CI gate restoration (2026-09-15, `.agents/260914-ci-gate-restoration/`) ·
Phase 07 authenticated software evidence (`.agents/` + run `33251921677:1`) · Phase 10 x86 Tier 3 VirtIO
27 hostile scenarios · Tier 1 Rust `std` PAL (2026-09-18) · Tier 2 fail-closed containment (2026-09-18) ·
Tier 3 hypervisor signing + vCPU preemption (2026-09-18) · App-tier acceptance ledger schema v5
(`.agents/logs/260915-app-tier-ledger-schema-v5.md`) · B0 actor/supervisor
(`.agents/260925-2214-beam-parity-b0-actor-supervisor/`, `docs/evidence/actor-supervisor-harts1-qemu.*`) ·
Compositor & ViUI + QEMU evidence (2026-09-18) · Acceptance matrix (item 19) · Cell-native portability
phase 01–07 + follow-on C thread/spawn ABI (`.agents/260922-1549-cell-native-portability-program/`) ·
Lane mạng + hai gate CI `init-giveup`/`idle_ipc_wake` (2026-09-26) · kernel host test harness sống lại
(2026-09-26: 111 test chạy được, `ostd` 60 test) — tất cả có chi tiết trong `CHANGELOG.md`.

## Reference

### App Layers (taxonomy — không phải trạng thái)
1. **Tier 1** — Trusted Native SAS Cell.
   - Rust no-std: đang hoạt động (core + alloc + ostd).
   - Rust std: mục tiêu G4, vẫn là Tier 1, dùng pure-Rust PAL.
   - FFI: C/C++ freestanding, Zig native, POSIX shim, mlibc, Rust có FFI, Lua VM viết bằng C, vendor
     SDK (RKNN/Hailo/codec).
   - Lua.
   - Lưu ý: C/FFI không được Rust LBI bảo vệ; vẫn chạy trong SAS nên phải được tin cậy. PKU/MTE chỉ là
     defense-in-depth, không biến nó thành sandbox portable trên mọi kiến trúc.
2. **Tier 2** — Native Domain Cell: unsigned/unverified/untrusted tier 1, arbitrary native ELF.
3. **Tier 3** — Virtual Machine: xem mục "Tier 3" ở trên cho trạng thái.
4. **Cellos Native SDK**: Tier 1 và Tier 2 dùng chung API nguồn càng nhiều càng tốt; khác biệt ở
   target/deployment profile (Tier 1 cho SAS zero-copy grants; Tier 2 dùng domain-safe IPC + explicit
   mapped grants); SDK có thể từ chối API không hợp lệ theo target profile tại compile time.

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
- **Layer**: lớp cấu trúc phần mềm — SDK Core, Service Clients, Middleware, Tooling; hoặc Hardware
  Isolation Layer A/B/C.
- **Stage G1–G5**: giai đoạn sản phẩm/roadmap, hoàn toàn độc lập với app tier.
