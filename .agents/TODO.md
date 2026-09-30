# TODO

Việc **chưa xong** và con trỏ tới bằng chứng. Trạng thái thật do source/test quyết định,
không phải bởi dòng chữ ở đây.

Mục nào đã đóng thì **xoá khỏi file này** — không để lại dòng "đã đóng": lịch sử ở
`CHANGELOG.md` / `docs/project-changelog.md`, quyết định ở `docs/decisions/`, kế hoạch +
báo cáo ở `.agents/<plan>/`, cách làm ở `docs/guides/`. Chuỗi tiền lệ: các mục đã đóng
2026-09-19 → 2026-09-27 đã được gỡ, nội dung của chúng nằm ở bốn chỗ trên.

## Đang mở — làm được ngay, không cần gì thêm
- [open] **Cùng lớp lỗi `sys_recv` còn ở các cell khác** — `sys_recv` luôn trả `Ok(x)`, với `x == tid` là reply thật còn `x == dead_tid` là
  *thông báo chết* (kernel ghi lý do vào 8 byte đầu buffer) ⇒ mọi chỗ chỉ kiểm tra `Ok(_)`/bỏ qua kết quả đều có thể đọc cái chết thành thành công:
  `cells/tools/shell/src/cmd_sys.rs:196`, `cmd_fs.rs:900`, `config_client.rs:98` (bỏ hẳn kết quả), `net-tools/src/bin/wget.rs:305`. Đường block của VFS
  đã sửa (chỉ `Ok(tid)` mới tính là reply + quên tid để probe lại); nên có một helper chung trong `ostd::ipc` để không lặp lại.
- [in-progress] **Lane `Tier 3 x86 VirtIO E2E + Persistence` flaky (~1/3 lượt CI)** — chuỗi nhân quả đọc được từ artifact
  `x86-tier3-virtio-e2e-1` của run đỏ: (1) `vtd_iova_to_slpte … slpte=0x0 (iova=0x7ffdd0c0, write=0)` cho `dev=00:02:00` (đúng BDF NVMe)
  → (2) `[nvme] admin timeout after 1000000 polls` → cell nvme thoát → init respawn (instance 2 in `DMA authorized`) → (3)
  `[hv-x86] persistent disk open failed` — dòng này khớp `fatal_pattern` (`\[hv-x86\].*(fail|…)`) nên lane FAIL. `run1` (boot đầu) luôn xanh, 0 lỗi VT-d.
  Cell nvme chỉ DMA vào buffer *của chính nó* (bounce 512 B, không có request kiểu grant), nên nghi vấn nằm ở **map VT-d không còn hiệu lực ngay sau grant**
  (đường `unmap/revoke` mới của kernel) hoặc một địa chỉ cũ trong thiết bị. Observability đã push (`88df51b3f`): dòng grant DMA + map VT-d nay `warn!` kèm
  `DID/SLPT/phys/size` ⇒ lượt CI đỏ kế tiếp trả lời được IOVA lỗi có từng được grant cho thiết bị đó không. Local chưa repro: lane PASS 8/8, và 3 lane chạy
  song song cũng PASS, nên phải đọc từ CI (lane chạy local ~40 s/lượt nhờ QEMU 10.2.0 đã cache ở `~/.cache/cellos/qemu-10.2.0`).
  **Repro local được** (không cần chờ CI): chạy 6 lượt lane trong lúc 8 vòng `while :; do :; done` chiếm CPU — 1/6 lượt đỏ, ~2 phút/lượt,
  lane vẫn PASS 5/6 (nhanh hơn nhiều so với chờ CI). Lượt đỏ local cho: `VIRTIO_E2E_FAIL:bulk-neighbor-overwritten` + **đọc LBA 800000–1062144
  (390–518 MiB) trên đĩa 256 MiB** với `[nvme] io error opc=2 … status=16512` rồi `[nvme] io timeout opc=0 lba=0`. Wire format là `[op][sector:u64]`
  nên driver parse đúng sector của caller ⇒ **caller (VFS/FS) đã hỏi quá cuối thiết bị** — nghi chuỗi cluster FAT hỏng/lệch (bước 8 sector = 4 KiB/cluster).
  Đã thêm log `[nvme] out-of-range request: op=… sector=… namespace_sectors=…` để lượt sau chỉ đích danh.
  **Dữ liệu mới (có log thanh ghi):** lượt đỏ cho `[nvme] regs: aqa=0x3f003f asq=0x841000 acq=0x842000 (sq_virt=0x841000 cq_virt=0x842000)` ⇒ driver lập trình **đúng**, và view CPU (`sq_virt`) **trùng** view thiết bị (`sq_iova`) ⇒ loại giả thuyết lệch virt/iova. Địa chỉ lỗi `0x7ffdd0c0` = **2 GiB − 114 368** (lane chạy `-m 2G`) ⇒ nằm sát đỉnh RAM — đúng vùng **stack**; nó **không** thuộc bất kỳ trang nào trong 46 grant của lượt đó, và là **hằng số** (trùng cả CI lẫn local) ⇒ thiết bị DMA tới một địa chỉ **không ai authorize**, không phải giá trị ngẫu nhiên theo layout. Fault xảy ra **trước** timeout (dòng 166 < 170) ⇒ có tính nhân quả. Bước tiếp cần trace mức QEMU (`-trace pci_nvme_*`/`vtd_*`) để biết access này từ đâu. **Việc gọn tay nhất (cần quyết định thiết kế):** VFS mount **một lần** — driver chết thoáng qua trong lúc mount là `/mnt/sd` chết vĩnh viễn cả boot (log: `FAT32 /mnt/sd mount failed` rồi respawn xong vẫn không mount lại) ⇒ biến lỗi tạm thời thành fatal của lane; mount lại khi block driver register sẽ làm lane tự phục hồi.
  **ĐÍNH CHÍNH (quan trọng):** các LBA vượt cuối đĩa KHÔNG phải nguồn lỗi — chúng là **probe phân vùng cell-store `/bin`**: `api::abi::disk::PART_CELLSTORE_BASE_LBA = 1_062_144` (`libs/api/src/abi/disk.rs:52`, chính là sector `out-of-range` đầu tiên) và `PART_FAT32_BASE_LBA = 2_048`; lane chỉ mkfs một phân vùng FAT32 ở 2048 nên mọi read của cell-store đều vượt cuối đĩa và **được tha** (mount thất bại, `/bin` lấy từ ramdisk). Lượt xanh cũng có đúng ~10 dòng `out-of-range` như lượt đỏ ⇒ không phân biệt được gì. So sánh IOVA cũng đã chạy: driver in `admin.sq=0x841000 admin.cq=0x842000 io.sq=0x843000 io.cq=0x844000 identify=0x845000` khớp **chính xác** các dòng `[vtd] … phys=0x841000…` của kernel ⇒ không có lệch IOVA/phys. Hướng còn lại của lỗi persist của guest: đường **ghi** (VFS `PageCache::write_sector`/`BlockStream::write_raw_sector` và backend virtio-blk của hypervisor) có thể báo thành công khi tầng dưới thất bại — cần log tại đó.
- [in-progress] **RPi3**: SD storage + HDMI [done]; I2C/SPI BSC1 + SPI0 loopback [done trên board
  thật] nhưng cần sensor vật lý (SHT3x/MPU6050) để đọc dữ liệu cảm biến; USB DWC2 & LAN9514 (Phase
  05) đã gỡ nghẽn 100% trong mã nguồn (USB Policy v3, cấp DWC2 MMIO, one-shot level IRQ 9) — chờ
  cắm cáp Ethernet để kiểm thử thực địa.
- [in-progress] **Bringup board thật**: RISC-V (StarFive VisionFive 2, Pioneer) và mini PC x86 (Dell).
  Qualification AMD/Intel thật là gate độc lập; không suy diễn từ QEMU.
- [in-progress] **Manifest & tooling phía developer** (item 18): Manifest v2 + tooling tương thích
  [done] (ledger: Phase 05 `IMPLEMENTED` từ 2026-09-29, `85df7fc0f`); đổi field vật lý [blocked] chờ Manifest v3 + phê duyệt ABI riêng. Đích: tách rõ
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
- [open gap] **Tier 3 hardware gate**: Intel VMX chưa có VMCS/world-switch hoàn chỉnh.
  Boot-to-shell ARM64 nghiêm ngặt cần KVM/phần cứng thật — QEMU-TCG chỉ là machinery evidence.
  RPi3 hiện **không** là đích Tier 3: `board-rpi3` vào EL2 thì chủ động `eret` xuống EL1 trước
  `kmain`; `HypervisorCap` ARM chỉ mở khi `el2::is_el2()` và vCPU hiện chạy trực tiếp ở EL2.
  Cortex-A53 có EL2 về mặt kiến trúc nhưng không thể dùng nguyên EL2-host/TGE path của repo;
  không nâng quyền bằng cách bỏ EL1 handoff. Sau cổng QEMU, chọn board AMD SVM (backend x86 đã
  có) hoặc thiết kế riêng đường ARM EL1-host + EL2 trampoline và kiểm chứng board ARM phù hợp.
- [in-progress] **Tier 3 application evidence (Python trước, browser sau).** CPython 3 trong Alpine
  256 MiB đã PASS 3/3 lượt QEMU-TCG 10.2.0: `scripts/qemu-x86-python-gate.sh` cài qua `apk` HTTPS,
  chuyển JSON → CSV bằng `decimal`/`csv`, đối chiếu output và cho tiến trình Python thứ hai đọc nó.
  128 MiB OOM tại bước cài package (apk bị kill); profile 256 MiB mới qua đúng đường lỗi này.
  Job `qemu-x86-tier3-python` đã nối CI, nhưng chưa có kết quả hosted; nginx giữ làm regression
  thứ hai trong cùng job, không làm cổng ứng dụng chính. Cổng **browser** còn mở: cần xác minh RAM,
  virtio-gpu scanout, virtio-input và tương tác thực qua compositor, không suy từ việc device
  model đã tồn tại. Python gate dùng đĩa volatile; độ bền qua reboot do lane VirtIO riêng chứng
  minh, không phải do CSV tạm trong `/tmp`.
  Sau khi bật `seg_max=2`, một ISO Python mới hết `BOOT_WINDOW=1000` sau DNS
  mà chưa có `APK_PASS` (`build/tier3-python-segmax/`); bản này giấu log apk,
  nên chưa biết download hay cài đặt dừng ở đâu, **không** quy cho block (guest
  Python không hề probe virtio-blk). Bốn lượt sau với cửa sổ 1200 s PASS
  (`build/tier3-python-segmax-retry/`, `build/tier3-python-nettrace{,-2,-3}/`);
  hai pcap đầu ghi ~20 MiB TLS trả về trong 181/411 s, không chứa lần lỗi.
  Fixture nay in tiến trình `apk` ra UART, runner hỗ trợ `QEMU_NET_CAPTURE`;
  lần lỗi kế tiếp cần capture cùng thời điểm mới phân biệt CDN/SLIRP/guest.
  Nginx secondary rebuild trên cây mới PASS (`build/tier3-nginx-segmax/`).
  Lane persistence `scripts/qemu-x86-virtio-e2e.sh` PASS 5/5 trước profile mới
  (`build/tier3-qemu-stability-1790640945358/`) và 10/10 trên cây hiện tại
  (`build/tier3-stability-current-1790644936314/attempt-{1..10}/`): mỗi lượt dựng
  đĩa 256 MiB mới, ghi + FLUSH guest, boot lại, đọc marker ở guest và host. Batch này
  chạy trước khi bật `seg_max`; không tái hiện A/C/D, cũng không kiểm chứng B lúc đó.
  Không coi 10/10 là chứng minh xác suất lỗi bằng zero hay là qualification board.
  Dựng lại ISO persistence trong workdir riêng rồi chạy hai boot PASS
  (`build/tier3-persistence-isolated/`); dựng ISO hostile riêng, 27/27 tình huống
  + ghi recovery bền qua reset PASS (`build/tier3-hostile-current/`). Cả hai runner
  không ghi đè `kernel/src/embedded-hv-x86/init` tracked; CI hosted chưa quan sát.
  Sau khi bật `seg_max=2` và sửa vòng xử lý whole-chain, fixture mới kiểm thêm
  payload 16 KiB cùng các sector kế bên phải giữ zero: fresh-build PASS và
  15/15 lượt fresh-disk/two-boot PASS (`build/tier3-segmax-neighbor/`,
  `build/tier3-segmax-soak-{1..15}/`); hostile 27/27 PASS (`build/tier3-hostile-segmax/`).
- [open bug] **Tier 3 x86 persistent backend: A/C/D vẫn cần bằng chứng khi tái hiện.**
  Lỗi `seg_max` (B) đã được tái hiện và sửa riêng, có trace đỏ/xanh trong `CHANGELOG.md`;
  hai boot với marker và payload 16 KiB nay PASS khi quảng cáo tối đa hai data segment.
  Những trường hợp dưới đây **không** được quy cho cùng nguyên nhân nếu không có trace.
  (A) **VFS trả short read; nguồn sai kích thước chưa xác định**: `[hv-blk] VFS read response:
  GrantDone { bytes: 15360 }` cho request 64 KiB ⇒ guest thấy I/O error. Lần trace trước, fatfs
  thấy `guest_disk.img` dài 850944 byte ở offset 835584, trong khi file trên host là 16 777 216
  byte. Log `[vfs] short read ... fatfs_size=...` nay ghi kích thước fatfs thấy khi lỗi xảy ra;
  chưa chứng minh metadata trên đĩa, cache hay handle nào đã tạo ra sự khác biệt. Bước kế: đối
  chiếu kích thước ở mount và sau `write_at` với directory entry trên host ở cùng boot.

  (C) **flush ngắt quãng thất bại ở tầng raw**: `[hv-blk] VFS flush failed: Ok(Err(1))` — VFS *trả lời*
  `Err(1)` (không phải timeout), tức `FatBackend::sync` → `blk_router::blk_flush()` phía ngoài trả false, kèm
  `[hv-blk-host] request failed type=4 … status=1`; guest `dd … conv=fsync` báo `block-write` dù dữ liệu đã
  nằm trên đĩa (host-side marker check sau run1 vẫn PASS). NVMe cell đã log lỗi completion; VFS
  `blk_router::blk_flush` đã phân biệt lỗi recv với cell trả khác 0. Trong batch 5 lượt trước lỗi
  flush không tái hiện (4 xanh, 1 đỏ vì (D)); batch mới 5/5 xanh. Cơ chế "driver bận retry các
  read ngoài phạm vi" vẫn chỉ là giả thuyết. Bước kế: thu log cả hai tầng ngay khi tái hiện rồi
  sửa đúng nhánh thất bại; không thêm retry khi chưa phân loại được nguyên nhân.

  (D) **flaky boot phía Cellos**: 1/5 run đỏ với `FAIL: evidence rdinit was not selected in run 1` — guest
  evidence chưa được chọn, tức boot Cellos không đi tới bước đó (khác A/C). Chưa điều tra.


  Ghi chú: các `[nvme] io error opc=2 lba=800000…1062144 status=16512` xuất hiện đều đặn là **đúng** — read
  vượt quá namespace 256 MiB (do probe volume + chuỗi cluster đi lạc); log giới hạn 3 dòng mỗi boot, timeout
  luôn log.

  Fixture e2e xoá page cache trước khi đọc marker; bản mới kiểm cả payload 16 KiB
  sau flush và reboot. Lane: `scripts/qemu-x86-virtio-e2e.sh`.

## Blocked (chờ phần cứng hoặc governance)
- [blocked] **`cohort.dirty_bundle` còn worktree-sensitive** (cùng họ với lỗi evidence đóng băng file vừa sửa,
  nhưng khác ngữ nghĩa): nó so bytes của `patch.path` với `git diff --binary <revision>` của *worktree hiện tại*,
  nên một claim `dirty: true` sẽ đỏ ngay khi có commit sau đó. Ledger thật hiện chỉ có 1 claim và claim đó không
  có `dirty`/`dirty_bundle`, nên chưa bị ảnh hưởng; đổi ngữ nghĩa (nếu muốn) là quyết định thiết kế của owner.
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
  secure boot + remote CAS service). Tier 1 baseline [done], Tier 1 rust std [done] (ledger: Phase 06 `IMPLEMENTED` 2026-09-29, promotion/approvals vẫn blocked); Tier 3 [blocked].
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
