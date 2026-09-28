# TODO

Việc **chưa xong** và con trỏ tới bằng chứng. Trạng thái thật do source/test quyết định,
không phải bởi dòng chữ ở đây.

Mục nào đã đóng thì **xoá khỏi file này** — không để lại dòng "đã đóng": lịch sử ở
`CHANGELOG.md` / `docs/project-changelog.md`, quyết định ở `docs/decisions/`, kế hoạch +
báo cáo ở `.agents/<plan>/`, cách làm ở `docs/guides/`. Chuỗi tiền lệ: các mục đã đóng
2026-09-19 → 2026-09-27 đã được gỡ, nội dung của chúng nằm ở bốn chỗ trên.

## Đang mở — làm được ngay, không cần gì thêm
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
- [open gap] **Tier 3**: Intel VMX chưa có
  VMCS/world-switch hoàn chỉnh. Boot-to-shell ARM64 nghiêm ngặt cần KVM/phần cứng thật — QEMU-TCG
  chỉ là machinery evidence.
- [open bug] **Tier 3 x86 virtio-blk: request nhiều segment hỏng, nên `seg_max` chưa được quảng cáo.**
  Bật `VIRTIO_BLK_F_SEG_MAX` (bất kỳ giá trị ≥ 2) làm guest đọc lại chính sector đó ra **zero** hoặc lỗi
  I/O, trong khi device báo thành công. Đã loại trừ: batching của P1 (code cũ + `seg_max` cũng hỏng), độ
  dài chain (`seg_max: 2` cũng hỏng), chain bị guard từ chối (không có dòng `reject descriptor-chain`),
  indirect descriptor (feature không được quảng cáo), và đường di chuyển dữ liệu (đọc lại frame của
  descriptor ngay sau scatter thấy đúng byte; không request nào kết thúc bằng `VIRTIO_BLK_S_IOERR`; ảnh
  đĩa trên host vẫn còn dữ liệu). Nghi vấn còn lại: cách công bố completion của chain nhiều descriptor.
  Đã thử và **không** phải nguyên nhân: `used.len` — báo `payload + 1` theo spec (đã giữ, lane vẫn xanh với
  `seg_max` tắt) cũng không cứu được trường hợp nhiều segment.

  Bug tách riêng đã sửa trong lúc truy: **ngân sách IPC của FLUSH** dùng chung mức cơ sở 200 tick vốn chỉ đủ
  cho một round trip 4 KiB, nên một flush chậm-mà-đúng bị timeout và guest nhận `[hv-blk-host] request failed
  type=4 sector=0 buffers=2 status=1` (dd `conv=fsync` báo `block-write`). Nay flush dùng
  `chunk_timeout_ticks(VFS_GRANT_CHUNK)` và in lý do khi hỏng.

  Hai dạng hỏng đã tách được, cả hai chỉ xuất hiện khi request lớn hơn 4 KiB:
  (A) **VFS trả short read, và đã tìm ra vì sao**: `[hv-blk] VFS read response: GrantDone { bytes: 15360 }` cho
  một request 64 KiB (offset 901120) ⇒ cell báo `status=1` ⇒ guest "I/O error, dev vda, sector 96". Truy vết
  trong VFS cho thấy fatfs **tưởng `guest_disk.img` chỉ còn 850944 byte** trong khi file thật trên host là
  16 777 216 byte: `[vfs-dbg] read_at EOF path=guest_disk.img offset=835584 total=15360 want=65536
  believed_len=850944` — và `believed_len` đúng bằng `offset + total` của lần đọc, tức kích thước mà fatfs
  thấy bị hạ theo vị trí (nghi đường `write`/`append` với "remove-then-create", hoặc fatfs cập nhật
  directory entry sai sau các lần ghi in-place của `write_at`). Request 4 KiB không bao giờ đọc tới vùng đó
  nên lane vẫn xanh; sửa chỗ hạ kích thước là hết dạng A. Bước kế: log kích thước fatfs thấy ngay sau mount
  và sau mỗi `write_at` để bắt thời điểm nó tụt.
  (B) **dữ liệu nguồn không nhất quán giữa hai cell**: dump `ReadGuestMemory` ngay sau scatter cho 12 request
  đọc đầu của lần boot thứ hai thấy *cùng* sector 0 (off=0) có request nhận đúng marker
  (`[67,69,76,76,79,83,95,88]`) và có request nhận **zero** ở frame của descriptor. Vậy zero đến từ *nguồn dữ
  liệu* (grant/VFS/đường block ngoài), không phải từ cách guest nhìn page — framing cũ "guest thấy page khác
  frame" đã bị số liệu này sửa. Không request nào `status≠0`, không EOF/ERR ở `read_at`, không dòng `VFS read
  response` bất thường. **Đã loại tiếp tầng block của VFS**: trace `read_raw_sector` + `PageCache` trong lần
  boot hỏng cho thấy raw read trả đúng marker ở sector của cluster đầu file (10114 = "CELLOSE2") và zero ở các
  sector đuôi (đúng, vì ngoài marker đĩa rỗng), cache hit nhất quán, không dòng zero bất thường nào ở tầng này.
  Vậy zero được tạo ra **phía trên** tầng block: đường grant → cell → frame của guest (scatter hoặc
  `WriteGuestMemory`), không phải thiết bị/cache. Bước kế: instrument phía cell, đối chiếu nội dung grant *và*
  frame trang cho cùng request, rồi tới phía guest. Bằng chứng + lý do khoá feature nằm tại
  `cells/services/hypervisor/src/virtio_blk.rs` (`config_read`).
  Lane tái hiện: `scripts/qemu-x86-virtio-e2e.sh` (đỏ khi bật, xanh khi tắt).

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
