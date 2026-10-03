# TODO

Việc **chưa xong** và con trỏ tới bằng chứng. Trạng thái thật do source/test quyết định,
không phải bởi dòng chữ ở đây.

Mục nào đã đóng thì **xoá khỏi file này** — không để lại dòng "đã đóng": lịch sử ở
`CHANGELOG.md` / `docs/project-changelog.md`, quyết định ở `docs/decisions/`, kế hoạch +
báo cáo ở `.agents/<plan>/`, cách làm ở `docs/guides/`. Chuỗi tiền lệ: các mục đã đóng
2026-09-19 → 2026-09-27 đã được gỡ, nội dung của chúng nằm ở bốn chỗ trên.

## Đang mở — làm được ngay, không cần gì thêm
- [open] **`cells/runtimes/lua/src/bindings_net.rs` chưa wire nên còn 9 chỗ coi `Ok(_)` là reply** — file
  **không** được khai báo trong `main.rs` (grep `bindings_net` không thấy) nên hiện không biên dịch; khi wire
  `vnet.*` phải đi qua `ostd::ipc::recv_from(net_tid, …)` như các client khác (entry `ipc:` trong `CHANGELOG.md`).
  Các witness trong `cells/tests/` (bench `smp`/`preempt_latency`, `pipe-test`, c2c oracle) đã tự so sender
  hoặc retry-đến-khi-decode-được nên **không** nằm trong lớp lỗi này.
- [resolved] **Lane `Tier 3 x86 VirtIO E2E + Persistence` flaky (~1/3 lượt CI)** — chuỗi nhân quả đọc được từ artifact
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
  **ROOT CAUSE + FIX (3d783288e):** trace QEMU (`QEMU_TRACE_EVENTS='pci_nvme_*'` — hook mới trong lane script) mở đầu bằng màn khởi tạo NVMe **không phải của cellos**: FLR, AQA=0xff003f (depth 256; driver dùng 64), ASQ=0x7ffdd000, ACQ=0x7ffde000, CC=0x460001, Identify, CREATE_CQ 0x7ffdc000, CREATE_SQ 0x7ffd8000 ⇒ **SeaBIOS** (firmware khởi tạo NVMe như thiết bị boot, để controller **enabled** với queue của nó). Địa chỉ lỗi đúng là một slot queue đó: `0x7ffdd0c0 = 0x7ffdd000 + 0xc0` (slot SQ thứ 4); trace ghi `pci_nvme_err_addr_read addr 0x7ffdd0c0` rồi `pci_nvme_err_cfs controller fatal status` ngay sau một doorbell. Trước khi cell cài IOMMU context cho thiết bị (`GrantDma` đầu tiên) các địa chỉ firmware đi xuyên không dịch; ngay khi context xuất hiện, một access cũ bị dịch ⇒ fault ⇒ controller khoá fatal status ⇒ mọi lệnh sau (kể cả Identify của driver) timeout ⇒ cell chết ⇒ mount một lần đã fail ⇒ lane fatal. **Fix:** authorize admin queue trước (context tồn tại), rồi mới reset controller. **Kiểm chứng:** 9 lượt lane liên tiếp bật trace — 0 VT-d fault, 0 respawn NVMe, 8 PASS (trước fix mọi lượt đỏ đều có fault + respawn). **Còn lại:** mode `bulk-mismatch` (1/8, không fault, không respawn) — hướng riêng (đường ghi/persist của guest).
  **Trạng thái sau fix (kiểm trên CI, run của `3d783288e`):** họ lỗi VT-d đã **hết** — 0 fault, 0 respawn NVMe, 0 mount failure, 0 `persistent disk open failed`. Mode còn lại (trước bị che khuất): **lệnh NVMe Flush không bao giờ hoàn tất** — `[nvme] io timeout opc=0 lba=0` ×3 (hết ngân sách retry của VFS) → `[hv-blk] VFS flush failed: Ok(Err(1))` → guest `VIRTIO_E2E_FAIL:block-write` (CI) / `bulk-mismatch` (local 1/8). Đã loại trừ: driver **có** tạo lại cặp I/O queue sau reset (trace: `CREATE_CQ addr=0x844000 cqid=1 qsize=63`), offset doorbell đúng (0x1000 SQ0 / 0x100c CQ1), page cache cập nhật tại chỗ khi write-through. Đã thêm log timeout kèm `cid/cq_head/sq_tail/phase/raw_status/depth` để lượt đỏ kế tiếp nói vì sao completion bị bỏ lỡ.
  **ĐÍNH CHÍNH (quan trọng):** các LBA vượt cuối đĩa KHÔNG phải nguồn lỗi — chúng là **probe phân vùng cell-store `/bin`**: `api::abi::disk::PART_CELLSTORE_BASE_LBA = 1_062_144` (`libs/api/src/abi/disk.rs:52`, chính là sector `out-of-range` đầu tiên) và `PART_FAT32_BASE_LBA = 2_048`; lane chỉ mkfs một phân vùng FAT32 ở 2048 nên mọi read của cell-store đều vượt cuối đĩa và **được tha** (mount thất bại, `/bin` lấy từ ramdisk). Lượt xanh cũng có đúng ~10 dòng `out-of-range` như lượt đỏ ⇒ không phân biệt được gì. So sánh IOVA cũng đã chạy: driver in `admin.sq=0x841000 admin.cq=0x842000 io.sq=0x843000 io.cq=0x844000 identify=0x845000` khớp **chính xác** các dòng `[vtd] … phys=0x841000…` của kernel ⇒ không có lệch IOVA/phys. Hướng còn lại của lỗi persist của guest: đường **ghi** (VFS `PageCache::write_sector`/`BlockStream::write_raw_sector` và backend virtio-blk của hypervisor) có thể báo thành công khi tầng dưới thất bại — cần log tại đó.
- [in-progress] **RPi3**: SD storage + HDMI [done]; I2C/SPI BSC1 + SPI0 loopback [done trên board
  thật] nhưng cần sensor vật lý (SHT3x/MPU6050) để đọc dữ liệu cảm biến; USB DWC2 & LAN9514 (Phase
  05) đã gỡ nghẽn 100% trong mã nguồn (USB Policy v3, cấp DWC2 MMIO, one-shot level IRQ 9) — chờ
  cắm cáp Ethernet để kiểm thử thực địa. EL2 monitor: bản coherent đã chạy trên board 2026-10-01 —
  hai dòng `first-run` của EL1/EL2 khớp từng trường, `HVC/MMIO/VI/PREEMPT smoke PASS; HypervisorCap
  open`, `[hv] vCPU ready — entering run loop` và Linux 6.12.13 khởi động trên Cortex-A53
  (`[0x410fd034]`, earlycon PL011); trace dừng trước `~ #` nên **shell trên phần cứng chưa quan
  sát**. Hai thiết bị low-speed ở cổng hub 3/5 hỏng vì mọi CSPLIT được phát trong đúng microframe
  của SSPLIT (`hfnum=0000184A->0000184A->0000184A`, hcint `0x22` rồi `0x42`×2, "NYET exhausted (2
  attempts)"): driver nay pace theo microframe (`wait_microframe`, cửa sổ 4 tick như U-Boot, 4
  lượt, in số lượt thật) + test host cho cửa sổ qua wrap counter, đã deploy trong `cellos.uimg`
  (SHA-256 `256f33df…`) — **đã kiểm chứng trên board 2026-10-01**: hai nửa cách nhau một microframe
  (`ss hfnum=00001930->…` rồi `cs1 00001931->…->00001AC0`), thiết bị low-speed ở cổng 3 enumerate
  (`10c4:0005`, interface class 3/subclass 1/protocol 2, `HID interface 0 class=3 boot=1`) và driver
  kết thúc `driving 1 HID interface(s)` trong khi LAN9514 vẫn lên (`Hardware MAC: B8:27:EB:12:34:56`).
  Còn mở, **không** claim: phím đi vào guest (chưa bấm phím nào và trace dừng trước `~ #`), thiết bị
  cổng 5 (mọi CSPLIT trả NAK — thiết bị không trả lời, không phải lỗi lịch split), và LED lock (bàn
  phím này STALL cả `SET_REPORT` LED lẫn đọc report descriptor — driver ghi nhận một lần rồi bỏ qua).
  Cổng QEMU `raspi3b` strict **nhạy tải**: cùng
  payload PASS 2026-09-30 nhưng treo 3 lượt 2026-10-01 (load average ~28/27 CPU), cả bản cũ chạy đối
  chứng cũng treo, log phía Cellos giống hệt — timeout ở đây là *không kết luận*, không phải hồi quy.
  Board 2026-10-02: guest **không tạo được** (`[hv] create_vm failed … 128 MiB guest RAM` rồi
  `[hv] service quiesced`) nên không có chỗ cho phím; hub lúc đó cổng 2/3/4 trống (`status=0x0100`)
  và thiết bị low-speed duy nhất ở cổng 5 trả NAK mọi CSPLIT. Nghi phạm của create_vm là bộ quét
  carve cũ (nhả `FRAME_ALLOCATOR` mỗi 256 frame nên allocation của cell khác cắt ngang run đang đếm):
  nay `allocate_guest_ram` giữ một lock, quét tuyến tính một lượt (`find_free_run`), thêm
  `largest_free_run` vào dòng lỗi và log lý do khi từ chối (`create_vm refused: EL2 monitor not
  verified`) — payload `8b4fa080…` đã deploy, chờ board chạy lại **kèm log kernel** (`[ ERROR]`/
  `[ WARN]`), vì capture vừa rồi chỉ có phía cell. Chuẩn để khôi phục: bản driver trước đã được xác
  minh vật lý với receiver `2a7a:8a53` trên profile **host-shell** (`a` vào shell, keypad Enter chạy
  `ls`, LED lock đổi theo CAPS/NUM) — profile Tier-3 không có host shell nên guest phải chạy trước.
  Board 2026-10-03: carve fix **đã kiểm chứng trên board** (`[hv] VM created vm_id=1` → `[hv] vCPU
  ready` → Linux lên trên Cortex-A53); bàn phím ở cổng 4 khai low-speed nhưng **NAK** mọi
  `GET_DESCRIPTOR` trong khi hai nửa split đúng nhịp (`ss 00001BB7` → `cs1 00001BB8 hcint=0x42`
  (NYET) → `cs2 00001BB9 hcint=0x12` (NAK)) → thiết bị im lặng, không phải lỗi lịch; driver nay
  re-reset port rồi thử lại 3 vòng (`enumeration failed; re-resetting hub port N`) và in `final
  status=0x…`, payload `deab6f59…` đã deploy. Thiết bị đã từng enumerate (`10c4:0005`) và receiver
  `2a7a:8a53` là mốc đối chiếu — cắm một trong hai vào cổng 1–4 trước khi kết luận.
  Mô hình Tier 3 (chốt 2026-10-03): **Cellos là OS riêng, Tier 3 chỉ để chạy một app Linux** — mỗi VM
  một app, VM chết theo app; không mở phiên Linux nhiều app. Profile Tier-3 nay boot tới dấu nhắc
  (VFS+Input+Net+`/bin/shell`, init **không** tự start hypervisor) và `hv` trong shell mới start cell;
  preload là tuỳ chọn cấp máy chủ (`app-init/hv-autostart`, `make-hypervisor-fs-rpi3.sh --autostart`).
  Còn mở: app name đi từ shell (spawn argv) → cmdline guest → init trong guest exec app, kèm initramfs
  riêng cho profile volatile (profile SD đã có `tools/prepare-rpi3-guest-initramfs.py`).
  Mô hình build (chốt 2026-10-03, bỏ 3 profile tier-shaped): **embedded-first** — mọi ảnh đều có nền
  `vfs + net + shell` (console UART) cộng các **option trực giao**: `input`, `ui`, `ai`, `supervisor`,
  `tier3`, `tier3-autostart`; driver theo board descriptor. Tier 3 có ba mức: **không đóng gói**
  (`--no-tier3`: không `/bin/hypervisor`/`vmlinuz`/`initrd.gz`, `hv` fail-closed; FAT 11→8 file, host
  gate PASS), **có cell nhưng idle** (mặc định, `hv` bật), **preload** (`--autostart`). Front end:
  `scripts/build-image.sh` (`--list`, `--dry-run`, `--board`, `--no-tier3`, `--autostart`, `--guest`,
  `--out`). HDMI là option `ui`, không phải thuộc tính profile; Pi Tier-3 không có display driver nên
  vẫn UART-only, và thứ HDMI nên hiện là framebuffer của guest (thuộc mốc một-app-một-VM).
  Option `--drivers minimal` đã có: kernel dựng với `board-rpi3-bring-up` (feature `bring-up-drivers`
  trong `cellos-boards` cắt còn console/IRQ/timer/SD), init bỏ `usb-host` nên không spawn cell USB, và
  ảnh bỏ `/bin/dwc2-usb` + `/bin/lan9514` (FAT 11→9 file) — host gate PASS, log **không còn** dòng
  `[dwc2]`/`[lan9514]`/`[usb-hid]`. Còn mở: trim driver cho các board khác (chưa có set bring-up).
  Guest profile (chốt 2026-10-03): `--guest alpine|alpine-wide|alpine-gui`, **một guest mỗi ảnh**,
  dùng lại khuôn x86 — `boot_arm_profile.rs` giữ carve/rdinit/cmdline, DTB nhận `bootargs` tham số,
  cell log `[hv] guest profile: <tên> (<MiB>)`, hai profile wide cùng lúc là compile error.
  `alpine-wide` (256 MiB) đã kiểm chứng end-to-end: machinery + boot gate PASS, guest Alpine tới `~ #`
  (`build/rpi3-gate/wide-{machinery2,boot}/`). Browser **headless** dùng `alpine-wide` — không cần
  display device, chỉ cần RAM (Chromium ~300–500 MiB nên sẽ cần lớp 512 MiB riêng); `alpine-gui` chỉ
  cho cửa sổ hiển thị, còn phụ thuộc đường trình bày virtio-gpu → compositor → panel.
  Còn mở: `--app` (stage app + `rdinit=/bin/<app>`) và lớp base cho browser.
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
- [in-progress] **D5 scale profiles**: **đo sạch 2026-10-03: 295 cell rồi kernel từ chối có tên** —
  `USER: [a2a3-probe] OOM_TYPED count=295` kèm `[loader] spawn OOM: op=SpawnPinned caller=20
  path=/bin/bench-probe`, **giống hệt ở 2 GiB và 512 MiB**, 0 panic, 0 `allocation error`, mọi self-test PASS,
  `bound=memory` ⇒ trần là **heap boot 4 MiB** (`HEAP_FRAMES=1024`, `kernel/src/main.rs:610`); spawn tính cho
  caller ≈6,4 KB/cell (`caller_charged=1900996`/295), `Task` chỉ 1 440 B. ⇒ **N=256 đã đạt được** trong lượt
  đồng nhất; **và đã đo với heavy cell thường trú**: lane `--heavy M` spawn M cell `/bin/heavy-probe` (nhị phân riêng:
  arena heap 20 MiB khai báo + touch 16 MiB, cộng grant 16 MiB, rồi park) trước sweep ⇒ trần nhẹ
  **295/280/265/234** ở **M=0/1/2/4** (mỗi heavy cell ≈**15** cell trần — giá kernel-side của arena 20 MiB:
  5 120 trang × 32 B ledger ≈ 160 KiB) ⇒ **N=64/128/256 giữ được với M≤2 heavy, gãy ở M=4** (234 < 256). Runner
  từ chối báo cáo lượt heavy nếu log thiếu `heap resident:`/`heavy resident: grant=` cho từng cell. Số đo +
  cách đo (watchpoint gdbstub, backtrace trong allocator) + bước kế: `.agents/reports/d5-cell-scale-remeasure-261003.md`;
  runner `scripts/qemu-cell-scale.sh` (dựng ảnh một lần với `CELLOS_INCLUDE_CAPACITY_PROBE=1`). **Số cũ 204/236 và
  193/194 đều là giả**: 204/236 đo trên kernel tràn stack ở scheduler init (xoá bớt accounting), 193/194 bị
  chặn bởi một allocation infallible. Xem mục audit bên dưới.
  Nền: `docs/roadmap/beam-parity-backend-roadmap.md` §2.2–2.3.
- [open] **Chi phí heap mỗi cell ≈9,7 KB — đo được, và đây là đòn bẩy của gate**: histogram size-class ở trần
  M=0 (295 cell) cho thấy mỗi cell nhẹ giữ ~3,8 KB ở lớp ≤4 KB (≈2,8 allocation ~1,3 KB) + ~4,6 KB ở lớp
  ≤16 KB (≈3,2 allocation) ⇒ **danh sách segment của ELF** (`CellSegments.pages`, 16 B/trang × ~267 trang ≈ 4,3 KB)
  là phần lớn nhất; **ledger của address space KHÔNG per-page** (không lặp lại danh sách segment — ước lượng
  "32 B/trang" trước đây sai). Muốn N=256 cùng M=4 heavy (hiện 234 < 256) thì phải giảm chi phí này hoặc tăng
  heap kernel (`HEAP_FRAMES=1024` = 4 MiB, `kernel/src/main.rs:610`) — đo lại bằng `--heavy` sau mỗi thay đổi.
  Hướng: chia sẻ trang immutable của ELF (bước (2) trong roadmap §2.2) hoặc nén danh sách segment.
- [open] **Heap phía cell là arena tĩnh, chưa grow được**: `ostd::heap` cấp vùng `static` cố định (mặc định
  1 MiB; cell khai báo thêm bằng `declare_custom_heap!` — `/bin/heavy-probe` dùng 20 MiB). Profile heavy §2.3
  ("heap lớn + grant 16 MiB") nay **đã đủ** nhờ arena khai báo, nhưng cell sống lâu cần *lớn dần* (data cell,
  VFS cache…) thì chưa có: cần syscall cấp thêm vùng (`brk`/`mmap`-style) + allocator phía `ostd` grow theo.
  Đây cũng là điều kiện để N=512 khả thi.
- [in-progress] **Fail-closed cho toàn bộ đường spawn (audit allocation infallible)**: đo tiếp trong ngày cho thấy
  từ chối ở 193 là **lỗi liên tục (contiguity)** của một transient — `[signing] OOM: signed payload of 78760 bytes`
  với 299 KiB còn trống mà không có lỗ hole 78 KiB; payload nay dùng lại `PAYLOAD_SCRATCH` (bỏ 78 KiB churn mỗi
  spawn). Đã sửa thêm các allocation infallible: 3 `collect::<Vec<_>>()` trong `task/elf_prepare.rs` (4–12 KiB theo
  số trang ELF), `Vec<LoadedPage>` trong `loader/elf.rs::load_segments` (512 entry = 12 288 B), `Vec` của
  `measurement_log` (256 entry = 12 288 B), cùng bộ diagnostic giữ lại: tên stage cho mọi OOM (`[signing]/[fs]/[loader]/[mem]`),
  lý do allocator từ chối (`null_from_quota` vs `null_from_heap`), heap dùng sau mỗi SpawnPinned, và histogram
  **Trạng thái**: binder cuối cùng đã định danh và sửa — **`PendingMailbox::new()`** cấp sẵn
  `Vec::with_capacity(HOTSWAP_MSG_QUEUE_DEPTH)` = **6 656 byte cho MỌI task** (kể cả cell chỉ park, không bao giờ
  nhận message) bằng `Vec::with_capacity` infallible ⇒ vừa là allocation giết sweep ở ~150 cell, vừa là ~40% chi
  phí heap mỗi cell. Nay container **lazy** (`Vec::new()`; `try_push` đã fallible sẵn, tính cho cell 0) ⇒ sweep
  từ *halt ở ~150* thành **295 cell với `OOM_TYPED`** (giống nhau ở 2 GiB/512 MiB). Tìm ra nhờ **chụp call chain
  ngay trong `QuotaAlloc::alloc`** (`alloc caller[0..8]`) — bản scan trong `alloc_error_handler` thấy frame cũ
  (stale) nên chỉ vào nhầm `into_task`/`Stack::allocate`. Đã sửa kèm: `try_box` (fallible `Box`) cho
  `into_task`/`scheduler`, `aligned_elf::bytes` (copy ELF khi lệch 8 byte), ledger `push` ở
  `map_private_page`/`map_existing_task_stacks`/`map_grant_page` + builder reserve theo số mapping.
  Còn lại (chưa bind, ghi để không quên): `BTreeMap::insert` (node ~200 B — `try_insert` còn unstable), `queue.clone()`
  trong `scheduler::exit_task` (đường cell chết, hàm `void`), `Box::new` trong `spawn_with_stacks_configured`
  (đổi chữ ký dây chuyền qua 4 caller, có đường x86 + test), và các `to_vec`/`clone` nhỏ khác trong test-hooks.
  Đã sửa trong lượt này: `state_stash::stash` + `stage_spawn_argv` (copy fallible, trả 0 + warn — cùng sentinel với
  nhánh "stash full"), `scheduler::spawn_thread` (clone `allowed_drivers` fallible), `address_space` (thôi
  `Vec::with_capacity` cho `registrations`). Sweep vẫn **295** sau các sửa này. Chi tiết: `.agents/reports/d5-cell-scale-remeasure-261003.md`.
- [open] **`Scheduler::cell_owners` nên là map thưa, không phải bảng đặc**: sau khi sửa tràn stack (inline
  `[CellOwnerSlot; MAX_CELLS]` → `Vec`) bảng này chiếm **cố định 160 KiB heap** ở profile experiment
  (4096 slot × 40 byte; production 2,5 KiB) — tức ~8 cell trong trần 193, và là chi phí *cố định* chứ không phải
  per-cell. `Scheduler` đã dùng `BTreeMap` cho các map tương tự (`cell_owner_watches`), nên dạng đúng là
  `BTreeMap<u32, CellOwnerSlot>` (~0 khi rỗng); đổi 12 call site `get`/`get_mut` sang khoá `&(id as u32)`.
  Việc này làm con số gate D5 sạch hơn (bỏ 160 KiB chi phí cố định khỏi 4 MiB heap).
- [open] **Cell không load được khi khách có ≥4 GiB RAM** (đo 2026-10-03, `-m 4G`): `[ERROR] ELF: load VA
  0x100000000 already mapped — rejecting spawn` cho cả `/bin/platform` lẫn init ⇒ `Failed to spawn init`, không có shell.
  VA base của cell (`0x1_0000_0000`) đụng mapping của kernel khi RAM chạm 4 GiB. Đây là lỗi riêng, không phải D5;
  chặn mọi phép đo ở 4 GiB. Log: `build/cell-scale-*/qemu.log`.
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
  **Cùng lớp, tái hiện được (2026-10-03)**: FAT **nhúng** (VIFS1) đọc `/bin/bench-probe` **78 760** byte trong khi
  directory entry ghi **78 824** (short 64 byte, lặp lại mọi lượt); nguồn chưa chốt (mkfat32 hay `fatfs` read) —
  chi tiết ở `.agents/reports/d5-cell-scale-remeasure-261003.md`.

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
