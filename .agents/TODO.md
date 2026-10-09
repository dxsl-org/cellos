# TODO

Việc **chưa xong** và con trỏ tới bằng chứng. Trạng thái thật do source/test quyết định,
không phải bởi dòng chữ ở đây.

Mục nào đã đóng thì **xoá khỏi file này** — không để lại dòng "đã đóng": lịch sử ở
`CHANGELOG.md` / `docs/project-changelog.md`, quyết định ở `docs/decisions/`, kế hoạch +
báo cáo ở `.agents/<plan>/`, cách làm ở `docs/guides/`. Chuỗi tiền lệ: các mục đã đóng
2026-09-19 → 2026-09-27 đã được gỡ, nội dung của chúng nằm ở bốn chỗ trên.

## Current queue — Intel x86-64 C2C Anywhere only (2026-10-08)

[ADR-0022](../docs/decisions/0022-intel-x86-64-c2c-only-direction.md) supersedes
earlier queue ordering. Use the [portfolio](plan-portfolio.md) and
[current focus](../docs/roadmap/current-focus.md) before selecting any task.

- [ ] **Phase 03 steps 2–4, plus the two Phase-02 remainders, await admission.** Slices A and B and
  Phase-03 step 1 all landed 2026-10-09 (history in `CHANGELOG.md`; records in
  [`phase-02-local-boundary.md`](260927-1100-c2c-anywhere-tier-aware/phase-02-local-boundary.md) §§
  *Slice A/Slice B progress*; evidence `docs/evidence/c2c-sdk-binding-x86.{txt,log}` and
  `docs/evidence/c2c-cross-tier-exchange-x86.{txt,log}`).
  Phase-03 **step 1** (measured prototype, `docs/evidence/c2c-async-lifecycle-x86.{txt,log}`) shows
  the shipped primitive carries the local multi-outstanding shape — bounded, exactly-once, one `wait`
  round — so steps 2–4 are *not* justified as a new public submission syscall by measurement and are
  unadmitted; they still own multi-source waiting (`WaitCompletion` v1 is `NET_RX`/`TIMER` only),
  cancellation of a dispatched operation, the two-hart wake proof, retained-reply lifetime and
  queue/fairness reservation, and need the kernel file-owner handoff if they touch those paths.
  Phase 03 step 3 also owns moving `LocalEndpoint::call` onto the bounded primitive.
  Two Phase-02 remainders are **not** claimed by any slice and need their own decisions:
  a registry-**named** Tier-2 service (a private-root Cell cannot `RegisterService`; that authority is
  a decision, not a bug fix) and a wrong-**user-buffer** witness on the syscall copy path (needs a
  raw-pointer fixture plus its own unsafe-allowlist entry).
  Law-1 is complete — checkpoint 2 recorded 2026-10-08, surface **FROZEN**, drift caught by
  `scripts/check-lookupservicebound-law1-digests.sh`.
  Resolved 2026-10-08: the x86_64 `test-hooks` alignment ledger check now warms to the
  ledger's fixed point (the frame allocator builds the low RAM identity map on demand
  on x86_64) and fails only if it never settles, so `scripts/x86/qemu-domain-test.sh`
  runs to its own end. AArch64 and RV64 `test-hooks` re-run green. Evidence
  `docs/evidence/atomic-publication-ledger-x86-settling.{txt,log}`.
- [ ] Reconcile one exact Intel machine against the HCL; physical bring-up and
  acquisition remain separately gated. No AMD/new ARM/RISC-V hardware program.
- [ ] Close x86 Tier 2 admission/C/C++ gaps and Intel VMX/EPT prerequisites for
  the corresponding C2C consumers; preserve existing ABI/security review gates.
- [ ] Qualify two independently verified Intel nodes for LAN C2C after first-node bring-up,
  including restart, authority denial, disconnect/reconnect and uncertain outcomes.
- [ ] Close explicit all-tier adapters and relay identity/time/persistence gates;
  measure a bounded workload against Linux on identical hardware/semantics.

Every task must name a direct dependency or measured defect of this direction,
its acceptance scenario and evidence ceiling. Other work is **parked**.
Old `[in-progress]`, `next` and unchecked entries below are historical diagnostics,
not automatic permission to resume. Preserve source/tests/evidence; do not erase
unresolved defects or rerun them just to reconfirm recorded failures.

## Historical backlog and evidence — subject to the current queue
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
- [parked 2026-10-08 — ưu tiên chuyển sang board x86] **RPi3** (giữ nguyên toàn bộ hồ sơ bên dưới, không xoá):
  Trạng thái: SD + HDMI + I2C/SPI + EL2 monitor + USB DWC2 + LAN9514 driver **đều chạy trên board** (bàn phím, guest Alpine tới `~ #`, console UART dùng được);
  mục **chưa** xong duy nhất là NIC, và blocker là **thiết bị tự rời bus**, không phải code ta:
  `GINTSTS.DISCONNINT` (bit29) bật, `HPRT0` về **đúng giá trị default lúc boot** (`0x400`: `CONNSTS/PRTENA/PRTPWR=0`), `HFNUM` đóng băng — trong khi `GINTMSK=0x23000008` **vẫn đúng** ⇒ core sống và giữ cấu hình, *thiết bị* (LAN9514 upstream) mất; và việc đó xảy ra trong cửa sổ mà driver **không phát transfer USB nào** (ba bước ở giữa là IPC thuần).
  Ghi thanh ghi LAN9514 của ta **trùng U-Boot** (`LED_GPIO_CFG=0x01110000`, `BURST_CAP=5`, `BULK_IN_DLY=0x2000`, `HW_CFG BIR|MEF|BCE`, `AFC_CFG=0x00F830A1`); front-end `/bin/lan9514` không có MMIO; không nguồn nào ghi `HPRT0`/`PCGCCTL`/`GRSTCTL` lúc chạy; không nguồn nào chạm power-domain USB của SoC.
  Ba luồng còn mở, ghi lại để không mất:
  (1) **recovery re-enumeration** khi mất port (tách bring-up trong `cell_main` thành hàm gọi lại được: `reset_port` → re-enumerate hub + bàn phím + LAN9514 → `init()` lại chip → reset toggle/FIFO → đăng ký lại front-end) — fix bền cho *triệu chứng* và dùng được cho **mọi** NIC USB, không riêng Pi;
  (2) hai phép thử điện/nguồn (nguồn 5 V ≥2.5 A ngoài cổng USB của PC; một lượt boot **không** cắm bàn phím wireless) để chốt brownout;
  (3) cổng chẩn đoán `no-hid-poll` để tách "split HID có phá bus/port không".
  Ảnh cuối: `cellos.uimg` `7389e532…` (lượt 56) + chuỗi rollback trong mục dưới. Lý do park: mốc "LAN end-to-end" **đã có bằng chứng trên x86** (`igb_x86_dhcp`: `[net] DHCP acquired` + `[net] IP address: 10.0.2.15`, kèm biến thể VT-d), nên RPi3 NIC không còn là đường duy nhất tới mốc đó; giá trị còn lại của Pi là USB-host trên SoC khác (đa kiến trúc), không phải điều kiện tiên quyết.
- [in-progress] **RPi3** (hồ sơ tới 2026-10-08): SD storage + HDMI [done]; I2C/SPI BSC1 + SPI0 loopback [done trên board
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
  Board 2026-10-05 (lần 14): **bàn phím trở lại** ✓ (port 4 → `2a7a:8a53` → `driving 2 HID interface(s)` →
  `registered as an input event source`) và probe chốt dữ liệu: `direct[0x00]=[0x64]=[0x6C]=0x98021E11` — **ba offset
  khác nhau cùng một giá trị** ⇒ chip không chọn thanh ghi theo `wIndex` (dù setup packet đúng và hub trả đúng
  theo port), còn `csr[ADDRL/ADDRH]=0`. Giá trị **đổi giữa boot** (`0xB8021E11` → `0x98021E11`, ba byte thấp
  giống nhau) ⇒ không phải chip ID mà là state chưa khởi tạo — hợp với việc Pi để **firmware VideoCore nạp MAC**
  từ OTP, còn Cellos boot bare-metal thì không. Bước kế: đo placement index ở `wValue` (đã thêm vào probe).
  Payload `bf10cced…` (lanprobe) đã deploy + machinery PASS; rollback `cellos.uimg.before-lanprobe-ac87d4a5`.
  Board (lần 15) trả lời: dạng **`val` bị chip STALL** (`control data failed … bRequest=0xA0 wValue=0x14/0x64/0x6C wIndex=0`
  ⇒ chip **có** phân tích field, cách đặt index ở `wIndex` của ta đúng) nhưng **mọi offset trả cùng giá trị**
  `0x98021E11`; `csr[ADDRL/ADDRH]=0`. `recv_data` trả `Ok(0)` cho data phase rỗng và buffer là mảng zero mới ⇒
  giá trị nhận được là **4 byte thật** từ chip, không phải rác ⇒ chip trả cùng nội dung cho mọi offset.
  Đo tiếp (payload `lanscratch`): **ghi scratch 0x7C = 0x5A5AA5A5 rồi đọc lại** + in **số byte thật** mỗi transfer
  (thứ `control_transfer` đang bỏ qua). Kết quả quyết định: ghi-đọc lại được ⇒ còn sai addressing/datasheet;
  không ghi được ⇒ **register file của chip chưa sống** (khớp giả thuyết firmware VideoCore thường bring-up chip
  này), tức là bài toán bring-up sâu hơn chứ không phải sửa driver.
  Payload `155c95c2…` (lanscratch) đã deploy; board trả lời (lần 16) rằng dạng `val` **bị STALL** ⇒ chip phân tích
  field ⇒ cách đặt index ở `wIndex` đúng.
  **Board 2026-10-05 (lần 17) — người dùng chỉ ra chìa khoá: LAN hoạt động lúc netboot ⇒ hardware OK, lỗi ở driver.**
  Tham chiếu nằm ngay trong repo: `.agents/debug/u-boot-v2026.07/drivers/usb/eth/smsc95xx.c` (chính driver netboot
  board này). Ba lỗi so ra được: (1) request **đọc `0xA1` / ghi `0xA0`**, offset ở **`wIndex`**, `wValue=0` — code gốc
  đúng số request nhưng để offset ở `wValue`, còn "fix" của tôi thì **đảo hai request** ⇒ mọi đọc thành lệnh ghi
  chiều IN và trả cùng một giá trị rác cho mọi offset; (2) bảng offset bị bịa: `ID_REV=0x50`, `HW_CFG=0x74` + cửa sổ
  `MAC_CSR` không tồn tại — bảng thật: `ID_REV=0x00`, `INT_STS=0x08`, `TX_CFG=0x10`, `HW_CFG=0x14`, `MAC_CR=0x100`,
  `ADDRH=0x104`, `ADDRL=0x108`, `MII_ADDR=0x114`, `MII_DATA=0x118`, `FLOW=0x11C`, `VLAN1=0x120`, `COE_CR=0x130`;
  (3) trình tự init nay theo U-Boot (LRST + PHY reset có bound → MAC từ `ADDRL/ADDRH` (firmware đã nạp; fallback
  local nếu chip trống) → `BURST_CAP=5` → `BULK_IN_DLY=0x2000` → `HW_CFG BIR|MEF|BCE` → clear `INT_STS` → LED →
  `FLOW`/`AFC_CFG` → `MAC_CR` (+`MCPAS` cho ND của guest) → `VLAN1` → COE off → PHY AN → `TX_CFG`/`MAC_CR` bật TX+RX
  → chờ link). Payload `7a521062…` (lanuboot) đã deploy; board (lần 18) cho kết quả quyết định:
  `scratch write=1 read=0x0000A5A5` ⇒ **register file sống** (ghi 0x5A5AA5A5, đọc lại đúng nửa thấp — `VLAN1` chỉ giữ
  16 bit, so sánh 32 bit của tôi mới là gate sai) và `ID_REV=0xEC000002` đọc **trước** reset = cặp
  `idProduct`/`bcdDevice` của chính hàm USB, không phải chip ID ⇒ thứ tự sai. Đã sửa: **reset trước** (LRST + PHY
  reset như U-Boot) → đọc `ID_REV` chỉ để **log** (U-Boot cũng không kiểm) → scratch gate so **16 bit** → rồi mới
  MAC/buffer/HW_CFG/LED/FLOW/MAC_CR/PHY/TX-RX. Payload `20ecfc2795c8cbc41a78527302338f42338514c9b9df09170199b54641bfbc0c` (lanorder2) đã deploy + machinery PASS; rollback
  `cellos.uimg.before-lanorder2-7a521062`. Board (lần 19) xác nhận chuỗi đó **chạy hết**: scratch OK,
  `PHY link up (BMSR=0x0000782D) auto-negotiation complete` ✓, `LAN9514 transport ready` ✓ — nhưng
  `no MAC in the chip` (chip không giữ MAC ⇒ ta nạp MAC local `02:00:00:00:00:01`) và TX vẫn `accepted=false`.
  Payload `2fe5b1cf0d9e5060e605ff8ab1a024591fa2dbc2571f15078c5acd6488916eef` (lantx) đã deploy + machinery PASS; rollback `cellos.uimg.before-lantx-20ecfc27`. Thêm
  chẩn đoán để chốt: `ADDRL/ADDRH` thô, read-back `MAC_CR`/`TX_CFG`/`HW_CFG` sau khi ghi (TXEN=0x08, RXEN=0x04,
  TX_CFG_ON=0x04, BIR|MEF|BCE), và **lỗi đầu tiên của bulk OUT/IN** (NAK quá 50 lần ⇒ chip chưa nhận được gói;
  lỗi kênh ⇒ vấn đề transport DWC2).
  Board (lần 20) trả lời: **cấu hình latch hết** — `MAC_CR=0x0008000C` (MCPAS|TXEN|RXEN), `TX_CFG=0x00000004`,
  `HW_CFG=0x00001022` (BIR|MEF|BCE) — `PHY link up (BMSR=0x0000782D)` ✓ và **`[dwc2-usb] TX packet transmitted OK`**
  ⇒ bulk OUT chạy, **host đã phát được frame ra LAN**. `accepted=false` chỉ là lần thử ĐẦU của net service (chạy đua
  với NIC init, in một lần) ⇒ đã thêm dòng `first e1000 TX accepted len=…`. `ADDRL/ADDRH = 0xFFFFFFFF/0x0000FFFF`
  ⇒ chip thật sự trống MAC; ta nạp `02:00:00:00:00:01` — **placeholder phải thành duy nhất theo board** trước khi hai
  Pi Cellos chung LAN (lấy từ serial board khi có đường đọc).
  Còn cần xác nhận trên board: `[net] DHCP acquired — IP configured` (lease) và `[net-bridge] first e1000 RX len=…`
  (chiều nhận) ⇒ rồi mới tới guest `ifconfig eth0 up` + RX đếm lên.
  Payload `745928f5…` (lantxok) đã deploy. Board (lần 21): net service là binary **mới** (303864 B) nhưng **không in**
  dòng `first e1000 TX accepted` ⇒ chip phát xong mà bridge vẫn thấy `accepted=false`. Nguyên nhân (đọc code): reply đi
  vòng qua front-end `/bin/lan9514` nên **sender là tid của front-end**, còn bridge `sys_recv_timeout(tid_nic, …)` ⇒
  **timeout**, không phải chip từ chối. Đã sửa: host cell **trả lời trực tiếp client** (đường request vẫn qua front-end
  để validate envelope ⇒ giữ nguyên tính chất isolation), xoá nhánh response của front-end + envelope
  `encode_response`/`decode_response` (dead), và bridge **nói rõ lý do** lần đầu reply không được nhận
  (`no reply from tid N within the timeout` / `reply from tid N, expected M`).
  Payload `412fdb4148b9d027e95d107b698e7ed550ca8025f2fa1b94c31eb391dbc5a376` (lanreply) đã deploy + machinery PASS; rollback `cellos.uimg.before-lanreply-745928f5`.
  Board (lần 22) xác nhận **cả hai chiều**: `first e1000 RX len=64` ✓ (nhận được frame), `TX packet transmitted OK` ✓,
  `first e1000 TX accepted len=304` ✓ (và dòng evidence mới in ra ✓) — nhưng lộ bug kế: `reply from tid 0, expected 6
  (status 0x01)` = **kernel exit-watch wakeup** (sender 0, không payload) rơi vào giữa request và reply ⇒ bị đọc là
  reply hỏng ⇒ `invalidate_nic_driver` giữa DHCP. Đã sửa: bỏ qua wakeup, chờ tiếp (tối đa 4 lần), chỉ mismatch thật
  hoặc timeout mới kết thúc. Payload `79e6985d…` (lanwakeup) đã deploy.
  Board (lần 23): vẫn `reply from tid 0` ⇒ **đọc sai doc**: `Ok(0)` của `sys_recv_timeout` là **timeout** (không có
  message), không phải wakeup ⇒ vòng "skip 4" chỉ nhân hạn lên 800 ms và in sai chữ. Đã sửa: **một hạn cho mỗi lần
  thử**, message đúng (`no reply from tid N within the timeout`), chỉ sender ≠ 0 mới là mismatch; và driver cell
  **trả lời `not ready` ngay** khi front-end chưa spawn (trước đây nuốt request ⇒ client chờ hết hạn) ⇒ lần TX đầu
  thất bại nhanh và lần retry của client mới được phục vụ. Payload `6193d625…` (lanready) đã deploy.
  Board (lần 24): `TX not accepted: no reply from tid 6 within the timeout` **cùng lúc** `TX packet transmitted OK`
  ⇒ công việc xong, ack về muộn; RX `len=64` ✓ và lần sau `accepted=true` ✓. Round-trip NIC = client → cell → front-end
  → cell → USB → reply, mà vòng cell còn phục vụ HID xen kẽ ⇒ vượt 200 ms ⇒ đã nới `DRV_REPLY_TIMEOUT_TICKS` 20 → 100
  (1 s). Payload `ae92401e…` (lanbudget) đã deploy.
  Board (lần 25): vẫn timeout ở TX đầu ⇒ **tìm ra chỗ rơi request thật**: front-end `/bin/lan9514` chỉ nhận request
  **sau** handshake attach (vòng đầu chỉ nhận `is_attach`), nhưng driver cell forward ngay khi có `lan_worker_tid`
  ⇒ request bị **bỏ**, không frame nào ra dây, client chờ hết hạn. Đã sửa: gate `lan_worker_tid == 0 || lan_attach_pending`
  ⇒ trả `not ready` ngay ⇒ client retry mới được phục vụ. Payload `ad0a72c5…` (lanattach) đã deploy.
  Board (lần 26) vẫn thấy `no reply from tid 6 within the timeout` ⇒ **đó là dấu hiệu nhận biết build cũ**: với
  `lanattach`, lần TX đầu thất bại bằng **status `not ready`** nên chỉ có `accepted=false` mà **không có** dòng
  `TX not accepted:` (dòng đó chỉ in ở nhánh timeout/mismatch). VIFS1 của payload hiện tại có `/bin/dwc2-usb` 199856 B,
  `/bin/net` 304800 B; `sha256sum tools/rpi3-netboot/root/cellos.uimg` phải là `ad0a72c5…`.
  DHCP client được poll **mỗi vòng lặp** (`service-runtime.rs:181`) nên lease sẽ tới ngay khi TX thông; log thành công
  in `[net] DHCP acquired — IP configured` + `[net] IP address: a.b.c.d`.
  Board (lần 27) với `lanattach`: **lần TX đầu `accepted=true` ✓** (gate hiệu quả), RX ✓ — nhưng lần TX *sau* timeout
  trong khi frame **đã phát** ⇒ độ trễ ack không đều. Nguồn: `poll_split` (đường low-speed của bàn phím sau hub) dùng
  `SPIN_POLLS=200_000` × `SPLIT_ATTEMPTS=8` ⇒ hàng trăm ms mỗi lần poll HID, mà cell dùng chung vòng lặp với NIC ⇒
  round-trip vượt hạn. Đã sửa: `SPLIT_SPIN_POLLS=20_000`, `SPLIT_ATTEMPTS=4` (giữ `split_pending` qua các lần gọi ⇒
  spin ngắn chỉ là tiếp tục ở vòng sau). Payload `a524de5f…` (splitbudget) đã deploy.
  Board (lần 28): vẫn timeout lần đầu ⇒ **hazard reply mồ côi**: TX và RX cùng chờ từ tid 6 và payload không được tag, nên reply tới **sau** khi hạn hết sẽ nằm
  trong mailbox và bị **request kế tiếp** đọc nhầm (lần retry thấy `accepted` ✓ đúng vì thế). Đã sửa: nhánh fail **drain tối đa 4 message** từ tid đó trước khi
  `invalidate_nic_driver` (đúng vì request bị bỏ chính là request mà reply đó thuộc về). Payload `0c7eedfdbe3f7364f5c6071e9af5bc1c897c5ce54674283a9f5b0253784452e7` (lanstale) đã deploy + machinery PASS;
  rollback `cellos.uimg.before-lanstale-a524de5f`.
  Board (lần 29) — **test trong guest**: `ifconfig eth0 up` ✓, `TX packets:12` ✓ nhưng **`RX packets:0`** ✗ và
  `udhcpc: not found` ✗ (busybox **có** applet `udhcpc` nhưng thiếu symlink — tôi bỏ sót). Đã sửa: thêm symlink
  `udhcpc`, `nc`, `traceroute`, `arp` (tổng 77 link) và **chia đôi chuỗi guest-RX bằng 2 dòng once-only**:
  `[net-bridge] first guest RX len=…` (net service đưa frame cho hypervisor) và `[hv-virtio-net] first RX frame
  len=… into the guest` (hypervisor bơm vào virtio-net của guest). Dòng nào thiếu ⇒ biết đứt nửa nào.
  Payload `8d01295b…` (guestrx) đã deploy.
  Board (lần 30): guest đặt **IP tĩnh** `192.168.42.50/24` ✓ rồi `ping 192.168.42.1` — ARP/ping **đã phát** (`TX packet
  transmitted OK` + `accepted` ✓) nhưng **cả hai dòng chẩn đoán đều không in** ⇒ chưa biết hypervisor có poll `L2Recv`
  hay không (guard `rx_available` của tôi có thể đang chặn). "Treo" người dùng thấy là **`ping` chặn shell guest**
  (busybox ping không `-c` chạy mãi; dòng `ifconfig` gõ vào chỉ được echo chứ chưa chạy) — không phải Cellos treo;
  dùng `ping -c 3` hoặc Ctrl-C.
  Thêm 2 mốc: `[net-bridge] first guest L2Recv — guest MAC registered` (net service nhận request của hypervisor) và
  `[hv-virtio-net] guest RX buffers posted — polling the net service` (guard mở). Payload `e70f6f74…` (guestrxc) đã deploy.
  Board (lần 31) — **chốt nguyên nhân gốc**: guard **đã in** (`guest RX buffers posted` ✓ ⇒ guest có buffer ✓, hypervisor
  poll ✓) nhưng **không hề có** `first guest L2Recv` ⇒ **IPC hypervisor→net service chết im lặng**: `Connection::active_tid()`
  **poison tid vĩnh viễn** sau một lỗi `IpcError::Recv` (đúng theo doc "must poison because a late reply may arrive") mà
  **không có đường gỡ** (registry vẫn trỏ tid đó, service không restart) ⇒ mọi lời gọi sau short-circuit ⇒ guest mất mạng
  cả 2 chiều, không log gì. Đã sửa: gỡ poison sau khi **thu reply muộn** (bounded, non-blocking, 4 message) rồi dùng lại
  generation đó — cùng lý luận với drain ở net service. Payload `47bc14dcbe7c73a96ca25d74afe48a015fce8cc84d165541a6dbb1e1baddb2f2` (netunpoison) đã deploy + machinery PASS;
  rollback `cellos.uimg.before-netunpoison-e70f6f74`.
  Board (lần 32, 2026-10-06) — sau netunpoison, dòng thật hiện ra: `[hv-net-rx] first L2Recv tid=5 result=send deadline`
  (tức `IpcError::Send` = **admission bị từ chối suốt hạn 2 s**, không phải request tới rồi không có reply) và
  `[net-bridge] TX not accepted: no reply from tid 4 within the timeout`. **Nguyên nhân gốc (cùng một họ, hai chiều):**
  kernel chỉ nhận `sys_try_send` (rendezvous) khi target đang ở `TaskState::Recv{mask==0 || mask==caller}`
  (`kernel/src/task.rs:ipc_try_send`), nên mọi lời gọi dùng rendezvous tới một cell **không** đang park ở `Recv` đúng lúc đó
  đều bị **bỏ**, không phải bị trả lời chậm:
  (a) **driver → net service:** dwc2-usb trả lời NIC bằng `sys_try_send(client_tid, …)` (`main.rs:402/405/408`), nhưng trên
  một hart driver chạy tiếp ngay sau khi nhận request và trả lời **trước khi** net service kịp park ở `Recv{mask=4}` ⇒ reply
  bị bỏ ⇒ net service tiêu hết hạn 20 tick mỗi lệnh. Bằng chứng QEMU (ảnh trace): `[ipc-trace] try_send refused n=… caller=4
  target=5 reason=not-recv state=ready` lặp tới n=768+, `[net-loop] drv_cmd==drv_to` tăng 1 mỗi lượt (mỗi lượt ~200 ms) và
  `[net-bridge] NIC driver reply timeout`. Đây cũng là lời giải cho "TX packet transmitted OK **cùng lúc** not accepted" (lần 24):
  ack **không** về muộn, nó bị bỏ.
  (b) **hypervisor → net service:** `service_call_typed_bounded` chào `L2Send`/`L2Recv` bằng rendezvous, mà trạng thái nghỉ
  của Net Cell là `WaitCompletion{NET_RX}` (`completion_wait.rs:publish_wait_state_locked`) ⇒ bị từ chối suốt hạn
  (đúng dòng `send deadline` của board). `first e1000 TX accepted=true` (lần 27) **không** phải bằng chứng ngược:
  `flush_l2_replies` cũng phát frame DHCP của chính net service (`interface.rs:303`, `reply: None`).
  **Đã sửa (queue thay vì rendezvous, đúng như đường `sys_send` mà net service và front-end `/bin/lan9514` vốn dùng):**
  `ostd::ipc::service_call_bounded_queued` (+ bản `_typed_`) — `sys_send` xếp vào mailbox rồi chờ reply bằng recv có hạn,
  masked; net service dùng nó cho L2; dwc2-usb trả lời client bằng `sys_send` (3 nhánh `NicReply` + status `not ready`).
  **Kiểm chứng QEMU (machinery gate PASS, ảnh trace):** sau khi driver đăng ký (tid 4) `drv_cmd=45 446` và **`drv_to=0`**
  (trước: `drv_cmd==drv_to`), 0 dòng `NIC driver reply timeout`, và chỉ còn **1** dòng `[ipc-trace]` trong cả log (một cặp
  khác, lúc boot) — rendezvous driver→net đã biến mất.
  **Ảnh debug đã deploy:** `cellos.uimg` SHA-256 `ba7f723a3a34457a4e44f0ea310fef3ae58ea943696674d85770d47f6b7d5025`,
  payload `kernel8.img` `47b2aa6961d2f6dad2d5c39b0d80b4abff3cfc9a16018bad9c644a20b3f6ae8c` (60 579 840 B);
  rollback `cellos.uimg.before-l2trace-aa3cdc1f` (ảnh board cuối cùng trước đó). Dựng lại bằng
  `CELLOS_DEBUG_TRACE=1 bash scripts/build-image.sh --board raspberry-pi/3-model-b --volatile-disk --autostart --skip-fetch`
  ⇒ bật 3 feature chẩn đoán (rate-limited, không đổi hành vi): `cellos-kernel/ipc-trace`, `service-net/loop-trace`,
  `service-hypervisor/l2-trace`; ảnh có `--autostart` nên guest tự lên (machinery gate đòi `[hv] vCPU ready`).
  Board cần xác nhận, theo thứ tự: `[net-loop] drv_cmd` tăng nhanh với **`drv_to=0`** (transport driver đã thông) →
  `[net-bridge] first e1000 TX accepted=true` + `first e1000 RX` (LAN host) → guest `ifconfig eth0 192.168.42.50/24 up`
  rồi `ping -c 3 192.168.42.1`: chuỗi guest RX phải in `[net-loop] l2recv>0`, `[net-bridge] first guest L2Recv — guest MAC
  registered`, `[hv-l2] n=… L2Recv tid=… result=ok`/`data`, `[hv-virtio-net] first RX frame len=… into the guest`, và
  `ping` có reply. Nếu `l2recv=0` mà `[hv-virtio-net] guest RX buffers posted` đã in ⇒ hypervisor vẫn không tới được
  net service; khi đó đọc `[ipc-trace] … caller=<hv> target=<net tid> reason=not-recv state=?` (nếu còn dòng nào).
  Board (lần 33, 2026-10-06, ảnh `ba7f723a…`) — **một nửa đã thông trên phần cứng, một nửa lộ ra cái bẫy khoá hai chiều:**
  (1) **F1 (hypervisor → net service) THÔNG.** Sau `ifconfig eth0 192.168.42.50 netmask 255.255.255.0 up`:
  `[hv-virtio-net] guest RX buffers posted` → `[net-bridge] first guest L2Recv — guest MAC registered` →
  `[hv-l2] n=1 L2Recv tid=25 result=ok`, và `[net-loop] … l2recv` leo cùng `recv` tới 42. Dòng `first guest L2Recv`
  **chưa từng** in ở bất kỳ lượt nào trước ảnh này; nay chuỗi nhận của guest sống tới tận net service.
  (2) **F2 fix bằng `sys_send` gây khoá lẫn nhau.** Board in `[heartbeat] task 5 (cell 5) missed liveness deadline`
  với `state at kill: Sending { target: 4, delivery_id: 684 }` **và** `send target 4 state: Sending { target: 5,
  delivery_id: 685 } pending_msgs=1` ⇒ net service chờ driver nhận request (request đang nằm trong mailbox driver),
  driver chờ net service nhận reply — **khoá hai chiều**, heartbeat giết net service mỗi ~0,7–2 s và init restart liên
  tục. QEMU không bắt được vì driver ở đó trả lời tức thì (không bao giờ chạm nhánh chặn). Vận chuyển thì đúng:
  `drv_cmd` leo tới 156 với `drv_to` đứng ở 1.
  **Đã sửa (kernel `ipc_send_kernel`):** một target đang `Sending { target: caller }` là nửa còn lại của vòng gửi —
  nó không thể nhận message này cho tới khi send của chính nó hoàn tất, mà send đó chỉ hoàn tất khi caller quay lại
  nhận. Kernel nay **xếp message vào mailbox rồi trả `Ok(0)`** thay vì chặn caller (không thêm syscall, không đổi ABI).
  Thêm self-test thường trực `send_to_a_target_blocked_sending_to_us_queues` vào suite `IPC-PENDING` (QEMU in PASS).
  Kèm: net service **drain tối đa 4 reply muộn** từ tid driver khi hết hạn (`command()`), vì reply nay không còn bị bỏ
  nên reply muộn sẽ nằm trong mailbox và bị lệnh kế đọc nhầm (reply không có tag TX/RX).
  **Còn mở:** `ping -c 3 192.168.42.1` = 100% loss và `[net-loop] l2send=0` suốt cửa sổ ⇒ frame TX của guest
  **không tới** net service (guest báo `3 packets transmitted`); ảnh mới thêm dòng một-lần
  `[hv-net-tx] guest TX frame reached the backend len=…` để chốt frame có tới backend hay không. Ngoài ra driver vẫn
  có `[lan9514] first bulk-OUT failure: IO` và nhiều `[dwc2] TIMEOUT` (phía USB).
  **Ảnh kế tiếp đã deploy (2026-10-06, sau fix khoá):** `cellos.uimg` SHA-256
  `ffd8fc00a4b5fbbb41bbe9e75fd231d6991a603e4bad5e7a7a520bfa54b6b640`, payload `kernel8.img`
  `b9bb192852f597397393515f5adfa1ffbd410c304f7ae0d131a4afddcd8c68f0` (60 583 936 B); rollback vẫn là
  `cellos.uimg.before-l2trace-aa3cdc1f`. QEMU `raspi3b` machinery gate PASS + `IPC-PENDING` PASS (self-test vòng gửi),
  `drv_to` khác 0 = 0 dòng trên 44 708 lệnh, 0 lần heartbeat giết cell. Board cần xác nhận: **không còn**
  `missed liveness deadline` / `Init: service died — restarting` cho tid net; `[net-loop]` chạy liên tục với `drv_to`
  gần như đứng yên; rồi tới `ping -c 3 192.168.42.1` có reply (kèm `l2send>0` khi guest phát).
  Board (lần 34, 2026-10-06, ảnh `ffd8fc00…`) — **fix khoá hiệu lực, guest TX tới đích, nhưng net service vẫn bị giết vì
  send chặn, và lộ một bug đọc reply:**
  (1) Dump lúc kill đổi từ `send target 4 state: Sending { target: 5 }` (khoá hai chiều) sang
  `send target 4 state: Running pending_msgs=1` ⇒ driver **không còn** chặn trong reply; chỉ còn net service chặn trong
  send request của chính nó. Fix khoá chạy đúng như thiết kế.
  (2) **Guest TX tới đích:** `[hv-net-tx] guest TX frame reached the backend len=86`, `[net-loop] … l2send` leo 1→7, và
  sau khi net service restart: `[hv-net-tx] L2Send accepted len=70 tid=10` + `[hv-virtio-host] net-tx-complete`. Guest RX
  vẫn tốt (`first guest L2Recv`, `[hv-l2] L2Recv … result=ok`).
  (3) **Nhưng net service vẫn bị heartbeat giết** (`task 5 … Sending { target: 4, delivery_id: 672 }` giữ ~4,9 s): driver
  đang `Running` giữa một transfer USB nên không ở `Recv`, mà `command()` dùng `sys_send` **chặn** ⇒ net service park ở
  `Sending{4}` quá cửa sổ liveness ⇒ bị giết, mọi frame trong hàng đợi chết theo (ping 100% loss).
  **Đã sửa:** `command()` nay gọi `offer()` — `sys_try_send` lặp **có hạn 20 tick (200 ms)** + `sys_yield()`, không bao
  giờ vào `Sending`; driver bận chỉ tốn một lượt, và `drain_replies()` (tối đa 4 message) chạy ở **cả hai** nhánh fail
  để release driver đang chặn trong reply.
  (4) **Bug đọc reply (mới lộ):** `[net-bridge] first e1000 RX len=1` — driver trả lời **1 byte status** (`NicReply::Status`)
  cho `OP_RX` khi front-end chưa attach, net service ghép byte đó với byte cũ còn trong buffer thành độ dài ⇒ route một
  "frame" 1 byte vào stack. Sửa: `reply.fill(0)` trước mỗi lệnh + từ chối `n < MIN_FRAME (14)` (khung Ethernet ngắn nhất
  là 14 byte header). QEMU tái hiện được `first e1000 RX len=1` trước fix và **hết** sau fix (`drv_cmd` giảm 45 092 →
  1 757 trên cùng cửa sổ vì thôi lặp RX vô ích).
  (5) **Chưa xử lý:** `[hv-l2] L2Send … result=wrong sender` (n=1..4) — reply không đến từ tid đang chờ; nghi net service
  bị giết/restart giữa lúc hypervisor chờ (tid 5 → 10), cần đọc lại sau khi (3) hết giết.
  **Ảnh mới deploy:** `cellos.uimg` SHA-256 `39fac3594676f5d593940a8fcc9b36a8944334a192dfce401cef1588886937ac`,
  payload `kernel8.img` `669a0c1ebf8e3f543169464095605795aba790b7a0f2624fbf4d7e5ef0e9dca5` (60 583 936 B); rollback vẫn
  `cellos.uimg.before-l2trace-aa3cdc1f`. QEMU machinery PASS, `IPC-PENDING` PASS, `drv_to` khác 0 = 0 dòng, 0 lần giết.
  Board cần xác nhận: không còn `missed liveness deadline`; `[net-loop]` liên tục; `ping -c 3 192.168.42.1` có reply.
  Board (lần 35, 2026-10-06, ảnh `39fac359…`) — **fix liveness hiệu lực trên phần cứng, cả hai chiều L2 chạy, và trần còn lại
  là độ sẵn sàng của driver:**
  (1) **Không còn kill/restart nào** trong suốt lượt (trước đó net service chết mỗi ~1–2 s). `[net-loop]` chạy liên tục
  tới turns=315.
  (2) **Guest TX + RX đều qua được biên hypervisor↔net service:** `[hv-net-tx] guest TX frame reached the backend len=90`
  → `l2send` leo → `[hv-net-tx] L2Send accepted len=90 tid=5` + `[hv-virtio-host] net-tx-complete`; và
  `[hv-virtio-net] guest RX buffers posted` → `[net-bridge] first guest L2Recv — guest MAC registered` →
  `[hv-l2] n=2 L2Recv tid=5 result=ok`, `l2recv` leo.
  (3) **Nghẽn thật:** `[ipc-trace] try_send refused caller=5 target=4 reason=not-recv state=ready` — net service chào request
  cho driver (4) trong khi driver đang `ready` (chạy vòng lặp, không park ở `Recv`) ⇒ ~45% lệnh không tới được driver
  (`drv_cmd=291 drv_to=131`), mỗi lần hỏng tốn hết hạn 200 ms. **Đo trong QEMU:** driver park ở `Recv` mỗi lượt và lượt chỉ
  ~10–12 ms (`[dwc2-loop] max_turn_ms=12`), nên ở QEMU 0 lệnh hỏng; board chắc chắn lâu hơn nhiều (USB thật + split + NAK).
  (4) **Chẩn đoán của tôi tự làm hại:** `[ipc-trace]` in mỗi 256 lần ⇒ ~98 800 dòng chiếm gần nửa log và ăn thời gian UART
  ngay trong vòng lặp đang cần tiến triển. Nay mẫu mỗi 4096 lần.
  (5) **Frame TX không còn bị bỏ** khi driver bận: `flush_l2_replies` giữ frame ở đầu hàng đợi và chào lại lượt sau (trước
  đây bỏ ngay lần đầu hỏng ⇒ ARP/echo của guest mất đúng lúc driver bận).
  (6) **Chưa xử lý:** `[hv-l2] L2Send … result=wrong sender` (n=1..4) vẫn còn dù **không** còn restart ⇒ reply đến từ tid
  khác thật. Nghi kernel phục vụ death-notification bất kể mask (tài liệu `recv_from` nói rõ điều này) hoặc nhánh
  `flush_l2_replies` gửi reply hoãn bằng `sys_try_send` (rendezvous — rơi nếu hypervisor chưa park). Cần in tid của sender.
  **Ảnh mới deploy:** `cellos.uimg` SHA-256 `1cf320de6e7444e5e6ff8742ece32fefa1bf46062204b4af5fd2de98d501134e`,
  payload `kernel8.img` `be5c72acc1ce2a76de7e5e24c08b590f69d44dfa2987051b6d9b66d3d5391e58` (60 583 936 B); rollback vẫn
  `cellos.uimg.before-l2trace-aa3cdc1f`. QEMU machinery PASS, `IPC-PENDING` PASS, `drv_to` khác 0 = 0 dòng,
  `[ipc-trace]` chỉ 5 dòng, `[dwc2-loop] max_turn_ms=12`. Board cần trả lời: `[dwc2-loop] max_turn_ms` / `turns_over_100ms`
  trên phần cứng là bao nhiêu (đó là trần thật), và `ping -c 3` có reply chưa.
  Board (lần 36, 2026-10-07, ảnh `1cf320de…`) — **trả lời hai câu hỏi của lần 35 (trần thật là transport USB, `ping` vẫn 0
  reply), và lộ ra rằng console dùng chung không đọc được khi gõ trong guest:**
  (1) `[dwc2-loop] turns=… max_turn_ms=6108 last_turn_ms≈3,4 s turns_over_100ms=14` — trên board driver giữ **6,1 giây** một
  lượt (QEMU: `max_turn_ms=12`) ⇒ mọi lệnh NIC của net service nằm chờ trong lúc đó, kể cả khi IPC đã xếp hàng đúng.
  (2) **Không còn kill/restart**, và cả hai chiều L2 vẫn chạy: `[hv-net-tx] guest TX frame reached the backend len=90`,
  `[net-loop] l2send` leo 1→15, `l2recv` leo 1→10, `[hv-l2] … L2Recv result=ok`, `[net-bridge] first e1000 RX len=64`,
  `[net-bridge] first e1000 TX accepted=true`.
  (3) **`[hv-l2] n=1..4 L2Send … result=wrong sender` vẫn còn** dù không restart ⇒ đúng là reply tới từ tid khác (chưa sửa;
  cần in tid của sender).
  (4) **Trong đoạn log này guest RX không in `[net-bridge] first guest RX len=…`** (dù `first e1000 RX` và `first guest
  L2Recv` đã in) và `guest_q=0` ở mọi dòng `[net-loop]` ⇒ chưa có frame nào được chọn cho guest. **MAC không phải nguyên
  nhân:** DTB sinh ra không khai `local-mac-address` (`dtb.rs` mục 10) nên eth0 lấy MAC từ config space của virtio-net, cùng
  `GUEST_MAC` mà net service dùng để lọc (`virtio_net.rs:13` ↔ `net_backend.rs` L2Recv).
  (5) **Chưa loại trừ:** 192.168.42.1 có thể không tồn tại trên LAN thật — guest đặt tĩnh 192.168.42.50/24; nếu LAN của host ở
  subnet khác thì ARP broadcast không có ai trả lời và ping 0 reply **bất kể** driver. Cần `ifconfig eth0` trong guest (RX có
  nhích vì broadcast của LAN không?) rồi ping **gateway thật**.
  (6) Phía USB hỏng thật: `[dwc2] TIMEOUT ch=3/4 hcint=0x20` với `HCINT=0` mà `HAINT≠0`, `[lan9514] first bulk-OUT failure: IO`.
  `[dwc2] channel error: STALL` là `SET_IDLE` (bRequest 0x0A) của HID interface 1 — driver in một lần rồi bỏ qua, **không** phải
  nguyên nhân.
  (7) **Nhiễu log đến từ chính ảnh chẩn đoán:** `CELLOS_DEBUG_TRACE=1` bật cả ba trace mỗi-lượt nên console ngập, và mỗi dòng
  host còn cắt nát dòng đang gõ trong guest (`USER: ping -c 3 192.16[net-loop] turns=215 …`). Đã sửa trong lượt này: console
  có **chủ dòng** (record của writer khác phải đóng dòng đang mở trước) và hai công tắc tách ra — `CELLOS_DEBUG_TRACE=1` chỉ
  còn `l2-trace` nhịp thấp, trace mỗi-lượt chuyển sang `CELLOS_DEBUG_LOOP=1`; hai mốc một-lần
  (`[hv-net-tx] guest TX frame reached the backend`, `[dwc2-usb] TX packet transmitted OK`) nay in cả trong ảnh thường.
  (8) **Lời giải cho đường RX không bao giờ có reply — chip LAN9514 lọc unicast theo MAC của chính nó:** driver xoá
  `MAC_CR.PRMS` khi cấu hình chip (`lan9514.rs`: `mac_cr &= !(MAC_CR_PRMS | MAC_CR_HPFILT)`, chỉ bật `MCPAS`), nên chip chỉ nhận
  unicast tới **chính nó** — mà OTP không có MAC (`ADDRL=0xFFFFFFFF ADDRH=0x0000FFFF` ⇒ ghi cục bộ `02:00:00:00:00:01`), trong
  khi front-end bridge **hai stack mỗi bên một MAC** (smoltcp của net service `52:54:00:12:34:56`, virtio của guest
  `52:54:00:AA:BB:CC`). Khớp toàn bộ bằng chứng của lượt này: TX được chấp nhận, witness vào duy nhất là multicast
  (`first e1000 RX len=64`), `[net] DHCP: deconfigured` (không OFFER nào tới stack host), `guest_q=0` mọi mẫu, guest RX không có
  mốc nào, ping 0 reply. Đã sửa: bật `PRMS` (giữ `MCPAS`) — frame nào tới stack nào vẫn do `interface.rs::route_rx` quyết định
  theo MAC đích. **QEMU không kiểm chứng được** (`raspi3b` không có USB Ethernet) ⇒ board là nơi xác nhận: `[net] DHCP acquired`
  / `[net] IP address: …` → `[net-bridge] first guest RX len=…` + `[hv-virtio-net] first RX frame len=… into the guest` →
  `ping -c 3 <gateway thật>` có reply.
  Board cần xác nhận sau khi flash: gõ `ifconfig eth0` trong guest **đọc được** (không còn dòng host xen giữa), ảnh thường vẫn
  in đủ chuỗi mốc một-lần của guest TX/RX, và (8) có dẫn tới reply đầu tiên không.
  Board (lần 37, 2026-10-07, ảnh `b3853467…` = lượt 36 + chủ-dòng console + `PRMS`) — **console đã đọc được trên phần cứng,
  `PRMS` đã vào chip, và log này chốt được nguyên nhân gốc của đường RX:**
  (1) **Chủ dòng console hiệu lực:** `ifconfig eth0` và output `ping` in nguyên dòng, hết `USER: ping -c 3 192.16[net-loop] …`
  ⇒ gõ lệnh trong guest đọc được (đúng vấn đề báo ở đầu lượt).
  (2) **`PRMS` đã vào chip:** `[lan9514] read-back MAC_CR=0x000C000C` (trước `0x0008000C`) — bit `0x4_0000` có mặt.
  (3) **Guest lên với MAC `52:00:00:00:BB:00`, không phải `GUEST_MAC` `52:54:00:AA:BB:CC`** ⇒ `route_rx` (so MAC đích với giá
  trị `L2Recv` đăng ký) đẩy reply sang `rx_queue` của smoltcp ⇒ `RX packets:0`, không bao giờ có `[net-bridge] first guest RX`.
  (4) **Nguyên nhân gốc (đã sửa, verify được trên QEMU):** `VirtioMmio::mmio_read` (ARM) đưa **offset byte** của guest vào
  `config_read`, nhưng device chỉ trả lời offset 0 và 4; Linux đọc MAC 6 byte bằng `memcpy_fromio` (từng byte) nên các byte
  1,2,3,5 ra 0 ⇒ đúng `52 00 00 00 BB 00`. Dispatch **x86 đã có phép dịch `size == 1` từ trước** nên lane x86 không lộ. Nay
  `mmio_read` lấy word chứa offset (`off & !3`) rồi dịch `(off & 3) * 8` ⇒ đúng cho byte/word/word-bên-trong, và vì ở tầng
  transport nên áp cho mọi device (blk `capacity` 8 byte + `seg_max`, field `u8` của input, `status` của net).
  **QEMU xác nhận đầu-cuối** (gõ `hv` ở prompt Cellos rồi vào guest): `cat /sys/class/net/eth0/address` → `52:54:00:aa:bb:cc`,
  `ifconfig eth0` → `HWaddr 52:54:00:AA:BB:CC` (trước fix cùng lane: `52:00:00:00:bb:00`).
  (5) Phòng thủ kèm theo: net service **học MAC của guest từ chính frame `L2Send`** (địa chỉ nguồn) và `set_guest_mac` chỉ
  điền khi chưa biết gì ⇒ routing đúng cả khi guest tự đổi MAC lúc chạy.
  (6) **Còn lại trên board (transport USB):** `[dwc2] channel error: XACTERR` + dump, `[lan9514] first bulk-OUT failure: IO`,
  `[net-bridge] first e1000 TX accepted=false`, `[dwc2-usb] NIC request not forwarded to the front-end`,
  `[net-bridge] NIC driver reply timeout` ⇒ TX đầu tiên **không đi hết** qua chip; `[hv-l2] L2Send … wrong sender` vẫn chưa sửa.
  (7) **Hop driver→front-end cũng bỏ frame (đã sửa ở lượt này):** request tới `/bin/lan9514` là rendezvous `sys_try_send`, chỉ
  xong khi front-end park ở `Recv`; nó ở trong transfer USB suốt lượt dài (board `max_turn_ms≈6 s`) nên request rơi vào cửa sổ
  đó bị **bỏ** — đúng dòng `NIC request not forwarded to the front-end` ngay sau `VM created` kèm `NIC driver reply timeout`.
  Driver nay giữ một request hoãn và chào lại mỗi lượt (khuôn mẫu như handshake attach), xoá khi front-end được spawn lại; kèm
  mốc một-lần `[net-bridge] first guest TX accepted=…` để lần sau biết ngay frame của guest có được driver nhận không.
  **`raspi3b` không có LAN9514 nên nhánh này chỉ board kiểm chứng được** (QEMU chỉ xác nhận compile/boot).
  Board cần xác nhận sau khi flash ảnh mới: `ifconfig eth0` trong guest hiện `HWaddr 52:54:00:AA:BB:CC`, `[net-bridge] first
  guest RX len=…` + `[hv-virtio-net] first RX frame len=… into the guest` xuất hiện, `first guest TX accepted=true`, và
  `ping -c 3 <gateway thật>` có reply. Nếu vẫn 0 reply thì đọc hai số: `first guest TX accepted=?` (frame có ra khỏi board
  không) và trên **PC** `arp -a 192.168.42.50` khi guest đang ping (PC có thấy guest không) — cặp đó tách "chưa đi" khỏi
  "đi rồi mà không ai trả lời".
  Board (lần 38, 2026-10-07, ảnh mới nhất — log không có dòng nào chỉ riêng v7 nên có thể là v6 hoặc v7) — **MAC `52:54:00:AA:BB:CC`
  đã đúng trên board, TX của guest qua được hypervisor, nhưng RPC `L2Send` hỏng nên descriptor TX không bao giờ hoàn tất:**
  (1) `eth0 HWaddr 52:54:00:AA:BB:CC` ✔ (fix virtio-mmio config hiệu lực trên phần cứng), console vẫn sạch từng dòng.
  (2) Phía host: `[lan9514] first bulk-OUT failure: IO` rồi `[dwc2-usb] TX packet transmitted OK` + `[net-bridge] first e1000 TX
  accepted=true` ⇒ lần hỏng đầu được thử lại và thành công (retry hoạt động), xảy ra trước khi guest lên.
  (3) `[hv-net-tx] guest TX frame reached the backend len=90` **nhưng KHÔNG có `[hv-virtio-host] net-tx-complete`** (lượt 37 có) ⇒
  `transmit` trả `false` ⇒ descriptor TX của guest không được đánh dấu đã dùng ⇒ ping chết trong hàng đợi của guest. Chiều RX vẫn
  tốt (`[hv-net-rx] first L2Recv tid=5 result=ok`), chỉ L2Send hỏng ⇒ không phải "đường L2 chết" mà là "reply của L2Send không tới".
  (4) **Đã sửa:** `service_call_bounded_queued` nay **nhịn message lạ** (thông báo chết — kernel phục vụ bất kể mask) trong **cùng
  hạn** thay vì coi nó là `WrongSender` rồi bỏ cả request; đồng hồ `sys_get_scheduler_ticks`, tối đa 4 message lạ, và in tên tid lạ
  **một lần** (`[ipc] message from tid N, expected M`). Thêm mốc một-lần cho chiều TX trong ảnh quiet:
  `[hv-net-tx] first L2Send failure: <reason> tid=…` (chiều RX đã có `[hv-net-rx] first L2Recv …` từ trước).
  Board cần đọc lần tới: `[hv-net-tx] first L2Send failure: ?` — nếu vẫn còn, và `[ipc] message from tid N, expected M` để biết
  **ai** gửi vào mailbox hypervisor; cộng `[net-bridge] first guest TX accepted=…`, `[hv-virtio-host] net-tx-complete`, rồi reply
  của `ping`.
  Board (lần 39, 2026-10-07, ảnh v8 `c6b3ad14…`) — **fix IPC hiệu lực: TX của guest nay hoàn tất; câu hỏi thu hẹp còn "chip làm gì":**
  (1) `[net-bridge] first guest TX accepted=true` + `[hv-virtio-host] net-tx-complete` ✔ (lượt 38 thiếu cả hai) và **không** có
  `[hv-net-tx] first L2Send failure` ⇒ `transmit` thành công, L2Send thôi hỏng vì message lạ trong mailbox.
  (2) Fix request hoãn ở driver cũng chạy trên board: `[dwc2-usb] NIC request deferred until the front-end is parked`, rồi sau đó
  `[dwc2-usb] TX packet transmitted OK` + `[net-bridge] first e1000 TX accepted=true` ⇒ request hoãn được chào lại và thành công.
  (3) Vẫn **không** có `[net-bridge] first guest RX len=…` và `ping` 0 reply ⇒ chưa frame nào được route vào guest.
  (4) Trần còn lại nằm ở tầng chip/USB: `[dwc2] control setup failed … bRequest=0x00000009 (SET_REPORT/LED) … XACTERR` lặp lại cho
  HID interface 0 kèm `[usb-hid] WARN: iface 0 deferred the LED output report … too often` — transfer qua hub vẫn chập chờn (không
  phải nguyên nhân của ping, nhưng cùng một transport).
  (5) Ảnh lần này là **ảnh chẩn đoán** (`CELLOS_DEBUG_TRACE=1 CELLOS_DEBUG_LOOP=1`): dòng `[dwc2-loop]` nay mang counters
  `nic_tx_ok/nic_tx_fail/nic_rx/nic_rx_empty` ⇒ board trả lời thẳng "frame có ra khỏi chip không" và "chip có frame vào không".
  Board cần đọc: `[dwc2-loop] nic_tx_*` khi guest ping (`nic_tx_ok` tăng?), `nic_rx` khi có traffic từ PC (thử `ping 192.168.42.2`
  từ PC cùng lúc), `[net-loop] guest_q/rx_q`, `[hv-l2] … result=…`, và trên PC `arp -a 192.168.42.50` lúc guest đang ping.
  Board (lần 40, 2026-10-07, ảnh chẩn đoán v9 `aca65d8f…`) — **counters tầng chip chốt: tầng hỏng là USB, không phải IPC/routing/MAC:**
  (1) `nic_tx_ok=0` suốt lượt trong khi `nic_tx_fail` leo 1→15 ⇒ **mọi frame ra đều hỏng ở USB** (lượt trước còn có
  `TX packet transmitted OK`, lượt này không có lần nào).
  (2) `nic_rx=5` từ sớm rồi **đứng im** (`nic_rx_empty` chỉ +16 trong cả lượt) ⇒ sau 5 frame đầu, chip không đưa thêm frame nào vào.
  (3) `drv_to=340` / `drv_cmd=379` (~90% lệnh hết hạn) với `max_turn_ms=8163` ⇒ driver bị giữ tới **8,1 s/lượt** (QEMU 12 ms), nên
  net service không thể chào request trong lúc đó (`[ipc-trace] caller=5 target=4 reason=not-recv state=ready`).
  (4) `[ipc] message from tid 0, expected 5 — consumed, still waiting for the reply` ⇒ "message lạ" chính là **wakeup exit-watch của
  kernel**, và nhờ nhịn nó `[hv-l2] … L2Send … result` đổi từ `wrong sender` (lần 35–36) thành **`receive deadline/error`** — lý do
  hỏng giờ phản ánh đúng tầng dưới thay vì đổ lỗi cho reply.
  (5) **Kết luận:** trần là **transport USB qua hub trên phần cứng thật** — cùng một họ với `[dwc2] control setup failed …
  bRequest=0x00000009 (SET_REPORT/LED) … XACTERR` và `[usb-hid] iface 0 deferred the LED output report … too often`. Không còn nghi
  vấn nào ở MAC, routing hay IPC. Ảnh mới thêm counters theo **nguyên nhân** (`ch_xacterr/ch_stall/ch_other/ch_timeout`).
  Board cần đọc: `[dwc2-loop] … ch_xacterr=? ch_stall=? ch_other=? ch_timeout=?` — `ch_xacterr` chiếm gần hết ⇒ hướng sửa là lịch
  split (SS/CS qua hub); `ch_timeout` chiếm phần lớn ⇒ ngân sách poll của driver; và `max_turn_ms` cần xuống dưới ~200 ms thì net
  service mới chào được request giữa hai lần thử.
  Board (lần 41, 2026-10-07, ảnh chẩn đoán v10) — **counters theo nguyên nhân chốt: lỗi là *hết ngân sách poll*, không phải bus
  protocol — và một lượt driver treo tới 404 giây:**
  (1) `ch_xacterr=0 ch_stall=0 ch_other=0` suốt lượt trong khi `ch_timeout` leo 26→230 ⇒ toàn bộ lỗi transfer là **timeout theo
  ngân sách poll**; XACTERR chỉ thuộc đường HID/LED ở các lượt trước, không phải NIC.
  (2) `max_turn_ms=404405` ⇒ **một lượt giữ driver 404 giây**; `drv_to=312`/`drv_cmd=327` (~95% lệnh hết hạn) là hệ quả trực tiếp.
  (3) `nic_tx_ok=0` vẫn vậy (`nic_tx_fail` 1→12), `nic_rx` 2→4.
  (4) **Lỗi vận hành của lượt này (đã sửa):** ảnh bật trace được deploy để người vận hành *tương tác* ⇒ console lại ngập và không đọc
  được `ping`. Ảnh **quiet** đã dựng lại + deploy (`b3805ebe…`); muốn đo lại thì rollback `cellos.uimg.before-quiet-return-20261007`
  (ảnh chẩn đoán v10) hoặc dựng lại với `CELLOS_DEBUG_TRACE=1 CELLOS_DEBUG_LOOP=1`.
  **Hướng sửa tiếp (xác định rồi, chưa làm):** `wait_channel_with`/`wait_channel_spin` (`usb_channel.rs`) dùng **số vòng poll** làm
  ngân sách — `WAIT_POLLS=50_000`, `SPIN_POLLS=200_000`, `SPLIT_SPIN_POLLS=20_000 × SPLIT_ATTEMPTS=4` — mà mỗi vòng có yield, nên
  một channel kẹt tốn **giây** thay vì mili-giây. Đổi sang **hạn theo thời gian host** (HFNUM — driver đã đọc nó cho cửa sổ split)
  để một transfer kẹt tốn cỡ chục ms; khi đó `max_turn_ms` xuống dưới ~200 ms, net service mới chào được request giữa hai lần thử.
  Board (lần 42, 2026-10-07, ảnh quiet v11) — **console đọc được trở lại và sáu dòng đã kể đủ câu chuyện:**
  (1) `[net-bridge] first guest L2Recv` + `[hv-net-rx] first L2Recv tid=5 result=ok` ⇒ chiều poll RX thông.
  (2) `[hv-net-tx] guest TX frame reached the backend len=90` ⇒ frame của guest ra tới hypervisor.
  (3) `[ipc] message from tid 0 … consumed, still waiting` ⇒ wakeup của kernel được nhịn (fix IPC chạy đúng trên board).
  (4) **`[hv-net-tx] first L2Send failure: receive deadline/error tid=5`** ⇒ reply của net service không tới trong hạn 2 s, và
  `[dwc2-usb] NIC request deferred until the front-end is parked` chỉ ra vì sao: driver bận giữa transfer (lượt dài tới 404 s đo ở
  lần trước) ⇒ `transmit` trả false ⇒ descriptor TX của guest không hoàn tất ⇒ ping 0 reply.
  (5) **Đã sửa gốc của (4) trong lượt này:** `wait_channel` nay bị chặn bằng **thời gian bus** (`WAIT_MICROFRAMES=800` ≈ 100 ms, đọc
  từ `HFNUM`) + `WAIT_YIELDS=8`; `WAIT_POLLS=50_000` (mỗi vòng yield ~20 ms ⇒ hàng phút) đã bị xoá. Đánh đổi: transfer cần hơn
  ~100 ms sẽ **thất bại sớm** thay vì chờ hàng phút (mọi caller đều thử lại).
  Board cần xác nhận: `max_turn_ms` (ảnh chẩn đoán) xuống cỡ trăm ms, `drv_to/drv_cmd` giảm mạnh, `ping` có reply; và nếu enumeration
  của LAN9514/bàn phím hỏng thì hạn 100 ms là quá ngắn cho chúng — log sẽ cho thấy ngay vì các dòng enumeration là một-lần.
  Board (lần 43, 2026-10-07, ảnh quiet v12 = hạn-thời-gian-bus) — **hạn mới cắt được trần thời gian: TX của guest đi trọn.**
  (1) `[net-bridge] first guest TX accepted=true` + `[hv-virtio-host] net-tx-complete` ✔ và **không** còn dòng `first L2Send failure`
  ⇒ RPC `L2Send` nay kịp trong hạn 2 s; `[dwc2-usb] TX packet transmitted OK` cũng xuất hiện ⇒ chip có phát thật.
  (2) Vẫn **không** có `[net-bridge] first guest RX len=…` và ping 0 reply ⇒ chưa frame nào về tới guest.
  (3) Đã thêm mốc một-lần để tách hai khả năng còn lại (ảnh v13 `7cc0de32…`):
  `[net-bridge] first frame while guest registered: dst=… guest=… len=…` — **có** dòng đó ⇒ chip **có** đưa frame vào nhưng không cho
  guest (đầu dây bên kia không trả lời ARP của guest); **không** có dòng nào ⇒ chip không đưa gì vào (đường nhận của chip/PHY/dây).
  Board cần đọc: dòng mới đó (nếu có) + `[net-bridge] first guest RX len=…`, và phép thử trên PC: `arp -a 192.168.42.50` lúc guest
  ping, hoặc `ping 192.168.42.2` từ PC (để chắc chip nhận được và để biết LAN có tới được board không).
  Board (lần 44, 2026-10-07, ảnh quiet v13) — **mốc mới trả lời câu hỏi nó được thêm vào để trả lời: KHÔNG có frame nào vào cả.**
  (1) Trong cả lượt không có `[net-bridge] first frame while guest registered` **và** cũng không có `[net-bridge] first e1000 RX len=…`
  (mốc một-lần, chưa từng in ở lượt này) ⇒ **chip không đưa vào một frame nào** suốt lượt, kể cả frame broadcast/multicast của LAN.
  ⇒ Nhánh "LAN đang nói chuyện nhưng không nói với guest" bị loại; trần nằm ở **đường nhận của chip / PHY / đầu dây**, hoặc LAN im lặng
  tuyệt đối về phía board.
  (2) Phía ra vẫn tốt và đã trọn: `first guest TX accepted=true` + `net-tx-complete` ✔ (không có `first L2Send failure`).
  (3) **Người vận hành xác nhận hiệu ứng người dùng:** gõ trong guest **nhanh hơn rõ rệt** kể cả khi cổng LAN đã mở ⇒ hạn-thời-gian-bus
  cũng chính là thứ chặn độ trễ gõ phím (mỗi phím xếp sau các lượt USB dài).
  Board cần làm phép thử **hai chiều từ PC** (rẻ, 0 build): `ping 192.168.42.2` từ PC trong lúc guest đang ping (chip có nhận được
  không) và `arp -a 192.168.42.50` (PC có thấy `52:54:00:aa:bb:cc` không). Nếu cả hai đều không ⇒ đầu dây cable/PHY/interface PC phía
  board-netboot; nếu PC thấy guest mà board không thấy PC ⇒ đường nhận của chip.
  Nếu không chạy được phép thử PC: bước kế tiếp là **loopback ở chip** (đặt PHY `BMCR` bit 14 rồi phát một frame, xem `nic_rx` có tăng
  — tách đường TX/RX của chip khỏi dây) — cần một build có cờ chẩn đoán.
  PC-side (lần 45–46) — **`ping 192.168.42.2` KHÔNG kết luận được gì** (không ai trên board giữ địa chỉ đó dưới Cellos: net cell không có IP
  tĩnh, DHCP thất bại), còn `ping 192.168.42.50` (IP của guest) trả **`Destination host unreachable`** ⇒ PC đã broadcast ARP cho guest mà
  **không có ARP reply nào**. Kết hợp với log board (không mốc frame-vào nào) ⇒ chưa chứng minh được chip có nhận hay không, vì cần log
  board *trong lúc* PC ARP. PC cũng không có `arp` (Windows: `arp.exe -a`, `netsh interface ipv4 show addresses/neighbors`, hoặc
  `Get-NetNeighbor -AddressFamily IPv4`).
  **Ảnh lần này tự trả lời, không cần PC:** `CELLOS_DEBUG_LOOPBACK=1` bật self-test loopback PHY (`BMCR` bit 14) một lần lúc bring-up —
  in `[lan9514] loopback diag: chip_tx_ok=… chip_rx_bytes=…` rồi **phục hồi** PHY. `chip_tx_ok=true` + `chip_rx_bytes>0` ⇒ MAC TX + MAC RX
  + RX FIFO + bulk-IN đều chạy ⇒ trần ở dây/đầu bên kia (mâu thuẫn với việc U-Boot netboot được ⇒ phải xem lại PHY/duplex sau re-init);
  `chip_rx_bytes=0` ⇒ lỗi nằm trong cấu hình nhận của chip hoặc chính transfer bulk-IN dưới Cellos — hướng sửa tiếp theo nằm ở đó.
  Board (lần 47, 2026-10-07, ảnh loopback diag v14) — **loopback chứng minh đường nhận của chip chạy; nghi phạm còn lại là chính init của driver:**
  (1) `[lan9514] loopback diag: chip_tx_ok=true chip_rx_bytes=64` ⇒ một frame do chip phát **quay về được** qua đường nhận: MAC TX + PHY +
  MAC RX + RX FIFO + **bulk-IN của driver** đều chạy ⇒ loại bỏ "chip không nhận".
  (2) Ghép ba sự thật: board **netboot qua chính dây đó** (U-Boot: dây + PHY + chip hoạt động), loopback chạy (nội bộ chip hoạt động), và
  dưới Cellos **không frame nào từ dây vào tới net service** ⇒ nghi phạm duy nhất còn lại là **thứ driver ghi vào chip/PHY làm chết đường
  dây so với trạng thái U-Boot để lại** (lite reset + PHY reset + AN restart + MAC_CR/HW_CFG).
  (3) Ảnh mới in **snapshot chỉ-đọc trước mọi ghi** của driver: `pre-init: HW_CFG/MAC_CR/PM_CTL/ADDRL/ADDRH` và
  `pre-init phy: BMCR/BMSR/ANAR/ANLPAR/PHYSCS`, để so với read-back sau init (`read-back MAC_CR=… TX_CFG=… HW_CFG=…`, `PHY link up (BMSR=…)`).
  Kỳ vọng: nếu `pre-init phy` cho thấy U-Boot để lại một cấu hình khác (tốc độ/duplex/AN, hoặc ADDRL có MAC thật), thì chính các bước
  reset/AN của ta là thứ phải đổi — hướng sửa đã nằm sẵn: **bỏ reset/AN khi PHY đã link up** (bám trạng thái firmware, như bootloader→OS).
  Board (lần 48, 2026-10-07, ảnh v15 × **hai lượt trùng khít**) — **tái lập được, và lộ một race thật trong cách driver xử lý link:**
  (1) `pre-init` giống hệt nhau ở cả hai lượt: `HW_CFG=0x00000000 MAC_CR=0x00040000 PM_CTL=0x000001C0 ADDRL=0xFFFFFFFF ADDRH=0x0000FFFF` +
  `BMSR=0x00007829` (link **down**, AN chưa xong). ⇒ Chip đang ở trạng thái **sau reset**, không phải trạng thái U-Boot (U-Boot vừa TFTP 57,8 MiB với MAC
  thật + TX/RX bật) ⇒ **port reset của driver USB làm LAN9514 re-enumerate về mặc định** (nó là USB device sau hub) và PHY restart AN.
  (2) Cả hai lượt in `PHY link not up yet / check the cable` trong lúc AN còn chạy rồi **không bao giờ nhìn lại**; lượt v14 lại kịp in `PHY link up`
  ⇒ **race**. `loopback rx=0` (lượt link-down) so với `rx=64` (lượt link-up) cho thấy self-test cũng phụ thuộc trạng thái link, nên lượt link-down
  **không dùng để so sánh** (nó còn `bulk-OUT failure`, `guest TX accepted=false`, request register 0xA0/0xA1 XACTERR).
  (3) **Đã sửa:** bring-up chờ AN ~600 vòng và gọi đúng tên trạng thái ("not up yet", không phải lỗi cáp); vòng phục vụ **kiểm link định kỳ**, in **một
  lần mỗi lần chuyển** (`PHY link up`/`PHY link down` + `BMSR`) và **bật lại TX/RX** khi link lên.
  Board cần đọc lượt tới: `[lan9514] PHY link up` có xuất hiện (sau đó mong đợi frame từ dây vào + `first guest TX accepted=true`); nếu link **vẫn** không lên
  dù U-Boot vừa TFTP được thì kiểm giắc/port phía PC (dây rõ ràng tốt ngay trước đó) — nhưng đừng kết luận "lỗi cáp" chỉ vì một lần đo.
  **Ảnh mới deploy:** `tools/rpi3-netboot/root/cellos.uimg` SHA-256
  `30b916df39668c5bb2c01bb67b3b0ff4d52e890e92298f5e0d16d11ebe4c27c6`, payload `kernel8.img`
  `3556aadb4ea93ba54cf9cb968f724f352d94e3cac1b7936923934648a8ce4422` (60 616 704 B) — **ảnh QUIET** + theo dõi link (một dòng mỗi lần chuyển,
  `PHY link up`/`down` + `BMSR`, và bật lại TX/RX khi link lên); dựng bằng `bash scripts/build-image.sh --board raspberry-pi/3-model-b
  --volatile-disk --autostart --skip-fetch` rồi bọc bằng `python3 tools/rpi3-netboot/rpi3-uimage.py --input … --output …` + `--verify`.
  Rollback: `cellos.uimg.before-linkpoll-20261007` (ảnh v15 `4149cd9e…`), `.before-phy-snapshot-20261007` (v14 `b3cd260b…`), các mốc cũ hơn.
  Board (lần 49, 2026-10-08, ảnh v16 `30b916df…`) — **lượt đầu của vòng kiểm link: link lên thật và frame từ dây vào, nhưng chính vòng kiểm đó là nguồn nhiễu:**
  (1) `[lan9514] PHY link up (BMSR=0x0000782D), auto-negotiation complete` + read-back `MAC_CR=0x000C000C TX_CFG=0x00000004 HW_CFG=0x00001022`
  + `Hardware MAC: 02:00:00:00:00:01 (Ready)` ⇒ fix chờ AN (lần 48) hiệu lực trên board.
  (2) **Frame từ dây vào tới net service** (lượt 44 không hề có): `[net-bridge] first e1000 RX len=64` ⇒ đường nhận của chip + bulk-IN + routing chạy
  được **trong ảnh quiet** (không phải frame loopback của ảnh chẩn đoán).
  (3) **Nhưng kiểm link mỗi lượt là sai thiết kế:** một lượt là `RecvTimeout` 1 tick (10 ms) khi rảnh và ngắn hơn khi bận, nên ≥100 lượt/s × (2–3
  control transfer cho một lần đọc PHY: chờ MII idle → ghi `MII_ADDR` → đọc `MII_DATA`) = hàng trăm transfer/s chỉ để hỏi link — và log cho thấy đúng
  họ lỗi đó lặp hàng chục lần (`bRequest=0xA1/0xA0 wIndex=0x114/0x118` = `MII_ADDR`/`MII_DATA`) kèm 4 dump `state at error`.
  (4) **Và transfer hỏng bị đọc thành `BMSR=0`:** `[lan9514] PHY link down00000000` → `up0000782D` (dòng transition của v16 còn thiếu dấu ngoặc nên hex
  dính liền) ⇒ mỗi lần bus hỏng là một cặp down/up giả, kèm một `enable_data_path()` (đọc + ghi `MAC_CR`) ngay giữa lúc NIC có thể đang truyền. `read_phy_reg`
  trả `0` cho transfer không hoàn tất, còn `wait_mii_idle` đọc `0` thành "MII rảnh".
  (5) HID: `[dwc2] TIMEOUT ch=3 hcint=0x20` (3 dump, đã bound từ trước) và dump cho thấy `HAINT=0x2` trong khi `HCINT` của kênh đang chờ = 0 ⇒ chưa
  biết kênh nào giữ interrupt; nay dump in thêm dòng `HAINT ch=N HCCHAR/HCINT` cho các kênh mà `HAINT` chỉ.
  **Đã sửa (lượt này):** (a) `read_phy_reg`/`link_bmsr` trả `Option`, `write_phy_reg` trả `bool`, `wait_mii_idle` phân biệt "bus hỏng" với "MII rảnh";
  access thử lại 4 lần (`read_reg_retry`) rồi báo `None` thay vì trả 0 ⇒ read lỗi **không** đổi trạng thái link, không in cặp down/up, không ghi lại `MAC_CR`;
  (b) vòng phục vụ kiểm link theo nhịp riêng `LINK_POLL_TURNS=64` (≥ ~0,6 s khi rảnh) ⇒ bỏ hàng trăm transfer/s; (c) dòng transition in đúng dạng
  `PHY link up/down (BMSR=0x…)`; (d) lỗi control transfer báo **một lần cho mỗi (phase, nguyên nhân)** (`claim_failure_report`, thứ tự nguyên nhân theo
  `describe_hcint`) và `report_channel_error` cũng dedup theo nguyên nhân (dump vẫn 4 lần đầu) — trước đó mỗi transfer hỏng là một dòng; (e)
  `ID_REV=0xEC000002` nay được nhận là **LAN9512/9514** (bảng Linux `smsc95xx`: `0x9500` = LAN9500, `0xEC00` = LAN9512/9514) thay vì "unrecognized".
  **Kiểm chứng (không thay bằng suy luận):** `cargo test -p driver-dwc2-usb --lib --target x86_64-unknown-linux-gnu` 10/10 PASS (thêm test bất biến:
  các nguyên nhân khác nhau **không** được dùng chung slot báo cáo); `cargo check` + `clippy` cho `aarch64-unknown-none-softfloat` (kể cả
  `--features loop-trace,loopback-diag`) sạch cảnh báo mới; QEMU `raspi3b` **machinery gate PASS**. Nhánh link poll **chỉ board** kiểm chứng được
  (`raspi3b` không có LAN9514).
  **Ảnh deploy:** `tools/rpi3-netboot/root/cellos.uimg` SHA-256 `7d86b3019af8c52980706b00c24f1c249cf588a120ef8529da2fe83edaa7adb7`, payload
  `kernel8.img` `998efe3142aa97b1505db53b16730f90ec4c8275e104aa5fdf9d78c213a04fdb` (60 616 704 B) — **QUIET** (không `CELLOS_DEBUG_*`), dựng bằng
  `bash scripts/build-image.sh --board raspberry-pi/3-model-b --volatile-disk --autostart --skip-fetch` rồi
  `python3 tools/rpi3-netboot/rpi3-uimage.py --input … --output …` + `--verify`; rollback `cellos.uimg.before-linkcadence-30b916df` (v16).
  Board cần đọc: (a) **không còn** cặp `PHY link down/up` (chuyển thật thì mỗi lần chuyển đúng 1 dòng, có `BMSR`) và **không còn** loạt
  `[dwc2] control … failed wIndex=0x114/0x118`; (b) `ID_REV=0xEC000002 … (SMSC/Microchip LAN9512/9514)`; (c) `first e1000 RX` + `first guest TX
  accepted=…` + `[hv-virtio-host] net-tx-complete`; (d) trong guest `ifconfig eth0` (để biết subnet thật) rồi `ping -c 3 <gateway thật>` — nếu vẫn 0 reply
  thì đọc dòng `[dwc2] HAINT ch=… HCCHAR/HCINT` ở dump kế tiếp để biết kênh nào giữ interrupt.
  Board (lần 50, 2026-10-08, ảnh `7d86b301…` = lần 49) — **ba fix của lần 49 đều ăn trên board; bàn phím USB gõ được; và log chốt được chỗ frame của guest bị bỏ.**
  (1) `ID_REV=0xEC000002 … (SMSC/Microchip LAN9512/9514)` ✓; **không còn** cặp `PHY link down/up`; loạt `control … failed wIndex=0x114/0x118` từ hàng chục dòng + 4 dump xuống còn **1 dòng + 1 dump** ⇒ console sạch và vòng phục vụ không còn bị chính vòng kiểm link chiếm bus.
  (2) **Bàn phím USB gõ được trong guest** (người vận hành xác nhận; log có `Cellos > hv` rồi trong guest `ifconfig eth0 192.168.42.50 …`, `ping -c 3 …`) ⇒ mốc "một app một VM" nay điều khiển được bằng bàn phím thật.
  (3) **Cả hai chiều L2 sống:** `[net-bridge] first e1000 RX len=64`, `[dwc2-usb] TX packet transmitted OK`, `[net-bridge] first e1000 TX accepted=true`, `[hv-net-rx] first L2Recv tid=5 result=ok`, `[hv-virtio-net] guest RX buffers posted`, `[hv-net-tx] guest TX frame reached the backend len=90`.
  (4) **Chỗ frame guest bị bỏ (nguyên nhân ping 0 reply):** `[net-bridge] first guest TX accepted=false` rồi `[hv-net-tx] first L2Send failure: ok tid=5`.
      - Dòng thứ hai là **bug chẩn đoán**: `l2_status` chỉ phân loại `Ok(_)` nên in "ok" cho một lần **bị từ chối** (`Ok(NetResponse::Err)` = net service trả lời "driver không đưa được frame ra dây").
      - Dòng thứ nhất là **bug data-path**: `pump_rx_split` **pop frame** khi driver trả `status != 0`, kể cả khi đó chỉ là "chưa sẵn sàng/bận" — trái với chính sách đã ghi cho nhánh bận. ARP của guest bị bỏ ở lần từ chối đầu, không có cơ hội thứ hai.
      - Cùng lượt, tầng USB: `[lan9514] first bulk-OUT failure: IO` và `[dwc2-usb] NIC request deferred until the front-end is parked` + `[net-bridge] NIC driver reply timeout; frame not acknowledged` ⇒ hạn 2 s vẫn bị vượt khi driver ở giữa transfer (hiếm hơn nhiều so với ~95% trước lần 43).
  (5) **Instrumentation HAINT chạy đúng:** `HAINT ch=00000001 HCCHAR=0x80988A00 HCINT=0x00000010` ⇒ **kênh 1 (bulk IN EP1 của LAN9514) còn `CHENA` với `ACK` chưa được thu** — một transfer RX bị bỏ dở, không phải lỗi HID. Cần đọc dòng này mỗi khi có `TIMEOUT ch=3/4`.
  **Đã sửa (lượt này):** (a) net service: frame bị driver từ chối **giữ nguyên đầu hàng đợi** và chào lại lượt sau, tối đa `MAX_TX_REFUSALS=8` rồi mới bỏ (dòng một-lần `dropped a frame the driver refused Nx (status S)`); verdict trả cho `L2Send` **đúng một lần** (`reply` xoá ở lần từ chối đầu ⇒ lần thành công sau không trả lời lần hai); status byte của driver đi xuyên suốt (`NetResponse::Err(status)`) thay vì bị ép thành `0xFF`.
  (b) hypervisor: `l2_status` nay nêu **response** (`refused by the net service (driver status N)`) chứ không chỉ `Ok(_)`; mốc `[hv-net-tx] L2Send accepted len=… tid=…` nay in **cả trong ảnh quiet** (trước chỉ dưới `l2-trace`) ⇒ một lượt quiet đủ trả lời cả hai nửa TX.
  (c) driver: mã status có tên (`STATUS_OK`/`STATUS_FAILED`/`STATUS_NOT_READY`); nhánh "front-end chưa park" trả **2** thay vì 1 ⇒ log phân biệt được "chưa tới chip" với "chip từ chối".
  **Kiểm chứng:** `cargo test -p service-net` **42/42 PASS** (thêm 2 test bất biến cho chính sách hàng đợi: từ chối thì giữ, chạm trần thì nhả; accepted thì rời ngay và lần từ chối xoá reply), `-p driver-dwc2-usb --lib` 10/10 PASS, `-p service-hypervisor` 15/15 PASS; `cargo check`/`clippy` cho `aarch64-unknown-none-softfloat` (kể cả `service-net/loop-trace,service-hypervisor/l2-trace`) không phát sinh cảnh báo mới; QEMU `raspi3b` machinery gate PASS.
  **Ảnh deploy:** `tools/rpi3-netboot/root/cellos.uimg` SHA-256 `79d0dbc0d13568c988b20fdc379937c26bac88c443c9a0c243877665e635b814`, payload `kernel8.img`
  `1c2d4100c4f59fefcf887603d9eb33c51039fe56bea8826568fbd27f8316207e` (60 616 704 B) — QUIET, dựng bằng
  `bash scripts/build-image.sh --board raspberry-pi/3-model-b --volatile-disk --autostart --skip-fetch` rồi `python3 tools/rpi3-netboot/rpi3-uimage.py --input … --output …` + `--verify`;
  rollback `cellos.uimg.before-txretry-7d86b301` (= ảnh lần 50) và `.before-linkcadence-30b916df` (v16).
  Board cần đọc: (a) `[net-bridge] first guest TX accepted=true status=0` — nếu vẫn `false` thì `status=` nói ngay là 1 (USB) hay 2 (front-end chưa park), và nếu có `dropped a frame the driver refused 8x` thì khung đó bị bỏ thật; (b) `[hv-net-tx] L2Send accepted len=…` (mốc mới, có ⇒ frame đi trọn hypervisor→net service→driver); (c) `ping -c 3 <gateway thật>` có reply; (d) khi có `TIMEOUT ch=3/4`, đọc `HAINT ch=… HCCHAR/HCINT` — kênh 1 còn `CHENA`+`ACK` là transfer bulk-IN bị bỏ dở (hướng sửa riêng, chưa làm).
  Board (lần 51, 2026-10-08, ảnh `79d0dbc0…` = lượt 50) — **hai fix chẩn đoán in đúng thứ cần in, và chúng chốt: trần nằm ở tầng USB bulk, không phải hop IPC/queue.**
  (1) `[net-bridge] first guest TX accepted=false status=1` + `[hv-net-tx] first L2Send failure: refused by the net service (driver status 1) tid=5` ⇒ nhãn "ok" đã hết; **status 1 = transfer USB hỏng**, không phải "front-end chưa park" (2).
  (2) `[net-bridge] dropped a frame the driver refused 8x (status 1)` ⇒ chính sách giữ-và-chào-lại chạy **và đo được**: driver từ chối **8 lần liên tiếp** cùng một frame ⇒ bulk-OUT hỏng **bền (stateful)**, không phải xác suất rải rác. Ping vẫn 100% loss; guest `TX packets:0` trước khi `ifconfig`; `HWaddr 52:54:00:AA:BB:CC` ✓ và chiều nhận vẫn sống (`first guest L2Recv`, `[hv-net-rx] first L2Recv tid=5 result=ok`).
  (3) Bằng chứng mới phía chip: `[dwc2] control data failed … bRequest=0xA1 wIndex=0x114 … err=IO - STALL - endpoint refused the request` ⇒ **LAN9514 chủ động từ chối (STALL)** một lần đọc thanh ghi, và driver cũ **không hề gỡ halt**.
  (4) `HAINT ch=00000001 HCCHAR=0x00988A00 HCINT=0x00000002` ⇒ kênh 1 (bulk-IN EP1) đã halt còn `CHHLTD` treo; `bulk_receive` cũ **chỉ kiểm `XFRC` và `NAK`** nên mọi lỗi `STALL/XACTERR/DTERR` trên đường nhận bị trả về "không có frame" — một endpoint bị halt là im lặng vĩnh viễn.
  (5) `[usb-hid] WARN: iface 0 deferred the LED output report too often; waiting for lock-state change` ⇒ bound của đường LED có tác dụng (hết retry vô hạn).
  **Đã sửa (lượt này):** (a) tên nguyên nhân lỗi bulk: `UsbHostEngine::failure_name()` (theo `describe_hcint`) ⇒ dòng một-lần in `first bulk-OUT failure: STALL|XACTERR|no status bit…` thay vì chỉ `IO`; (b) **gỡ halt theo chuẩn USB**: `clear_endpoint_halt()` = `CLEAR_FEATURE(ENDPOINT_HALT)` (bmRequestType 0x02, bRequest 0x01, wIndex = endpoint) + thử lại đúng packet **một lần** khi lỗi là `STALL`, ở **cả hai** đường bulk (OUT đưa data toggle về DATA0 theo §8.5.3.4); control-EP không cần vì `SETUP` kế tự gỡ halt; (c) `bulk_receive` nay phát hiện lỗi (`STALL/XACTERR/BBLERR/DTERR`) thay vì im lặng trả `Ok(0)`; (d) ngân sách poll RX nay theo `HFNUM` + `WAIT_YIELDS` (≈100 ms) như `wait_channel` — trước là **1000 `sys_yield()`** (hàng chục giây), đúng trong cửa sổ hạn 2 s mà net service đang chờ ⇒ một nguồn của `NIC driver reply timeout`; (e) mỗi lần thử một chunk ở DMA mode nay **trỏ lại `HCDMA` về đầu chunk**: core tự đẩy `HCDMA` khi fetch packet, nên retry chỉ ghi `HCTSIZ` sẽ fetch **packet kế tiếp** như thể là chunk này (khung ra dây lệch byte) — sửa cho cả nhánh retry NAK đã có từ trước.
  **Kiểm chứng:** `cargo test -p driver-dwc2-usb --lib` 10/10 PASS; `cargo check`/`clippy` cho `aarch64-unknown-none-softfloat` (kể cả `--features loop-trace,loopback-diag`) sạch cảnh báo mới; QEMU `raspi3b` machinery gate PASS. Đường bulk **chỉ board** kiểm chứng được (`raspi3b` không có LAN9514).
  **Ảnh deploy:** `tools/rpi3-netboot/root/cellos.uimg` SHA-256 `763e37ac285cc1e5912266ba6b3722d3c2842d6ab61a41ed43ce6e9e16ab1603`, payload
  `kernel8.img` `b5763bd825cdf990527cc28ee11d5b48a1881bcd1e26d10926a3a799924637ab` (60 616 704 B), QUIET; rollback `cellos.uimg.before-haltclear-79d0dbc0` (= ảnh lượt 51).
  Board cần đọc: (a) `[lan9514] first bulk-OUT failure: <nguyên nhân>` — nếu `STALL - endpoint refused the request` thì fix (b) sẽ in `[dwc2] cleared a halted bulk-OUT endpoint after STALL` và frame đi tiếp; nếu `no status bit, the poll budget ran out` thì trần là lịch/ngân sách chứ không phải device; (b) `first bulk-IN failure:` tương tự trên đường nhận; (c) `first guest TX accepted=true status=0` + `[hv-virtio-host] net-tx-complete`; (d) `ping -c 3 <gateway thật>` có reply.
  Board (lần 52, 2026-10-08, ảnh `763e37ac…` = lượt 51) — **tên nguyên nhân chốt được root cause của đường nhận: `DTERR - data toggle mismatch`.**
  (1) `[lan9514] first bulk-OUT failure: XACTERR - transaction error` (nay có tên, không còn chỉ `IO`) rồi `[dwc2-usb] TX packet transmitted OK` ⇒ bulk OUT **có** lúc thành công, nhưng vẫn `dropped a frame the driver refused 8x (status 1)`.
  (2) **`[lan9514] first bulk-IN failure: DTERR - data toggle mismatch`** — dòng này chỉ in được vì `bulk_receive` nay kiểm bit lỗi; trước đây nó chỉ kiểm `XFRC`/`NAK` nên trả `Ok(0)` im lặng.
  (3) **Root cause (đã sửa): driver không quản lý data toggle theo endpoint.** USB 2.0 §8.6: toggle thuộc *endpoint* và sống qua các transfer; DWC2 trả lại giá trị thật trong `HCTSIZ.PID`. Driver dùng **DATA0 cố định** cho mọi bulk/interrupt IN và **reset DATA0** mỗi lần gọi bulk OUT ⇒ từ packet thứ hai trở đi sai toggle. Khớp toàn bộ bằng chứng cũ: lần 40 `nic_rx=5` rồi đứng im, lần 44 "không frame nào vào", bàn phím mất report xen kẽ. Đối chiếu chuẩn ngay trong repo: `.agents/debug/u-boot-v2026.07/drivers/usb/host/dwc2.c` giữ `in_data_toggle[dev][ep]`/`out_data_toggle[dev][ep]` và đọc lại PID trong `wait_for_chhltd` — chính driver netboot board này.
  **Đã sửa:** bảng toggle theo `(dev_addr, endpoint)` (`DataToggles`, 128×16×2 như U-Boot) + `next_pid()`/`adopt_pid()` (đọc lại `HCTSIZ.PID` sau mỗi packet, không tự đoán) dùng ở **bốn** đường: bulk IN (ch1), bulk OUT (ch2), interrupt IN non-split (ch3/4), `poll_split` (bàn phím low-speed sau hub); `bulk_transmit` nay tính PID **trong** vòng retry (ngoài vòng thì flip/reset vô hiệu); `clear_endpoint_toggle` khi gỡ halt STALL (toggle về DATA0, §8.6); `flip_pid` + `last_was_dterr` để một toggle lệch **tự sửa** thay vì hỏng vĩnh viễn (DTERR trước đây bị bỏ qua hoàn toàn ở đường interrupt).
  **Kiểm chứng:** `cargo test -p driver-dwc2-usb --lib` **11/11 PASS** (thêm test bất biến: slot toggle không alias khi địa chỉ ngoài bảng); `check`/`clippy` cho `aarch64-unknown-none-softfloat` (kể cả `loop-trace,loopback-diag`) sạch cảnh báo mới; QEMU `raspi3b` machinery gate PASS. QEMU không có thiết bị downstream ⇒ đường này **chỉ board** kiểm chứng.
  **Ảnh deploy:** `tools/rpi3-netboot/root/cellos.uimg` SHA-256 `7a02e07252aaa7cb500c559b5c2a27c54ae32473d1c713cae68571e756cbf767`, payload
  `kernel8.img` `f6402d555309ba7d98d4f4bee581d86c26e814f273aa650d522602340012f9e8` (60 616 704 B), QUIET; rollback `cellos.uimg.before-toggle-763e37ac` (= ảnh lượt 52).
  Board cần đọc: (a) **không còn** `first bulk-IN failure: DTERR`; nếu có `flipped a stale … toggle after DTERR` đúng **một** lần rồi thôi ⇒ self-heal chạy đúng; (b) đường nhận không chết sau frame đầu: `[net-bridge] first guest RX len=…` + `[hv-virtio-net] first RX frame len=… into the guest` xuất hiện; (c) `first guest TX accepted=true status=0` + `[hv-virtio-host] net-tx-complete`; (d) `ping -c 3 <gateway thật>` có reply; (e) gõ bàn phím không mất ký tự.
  Board (lần 53, 2026-10-08, ảnh `7a02e072…` = lượt 52) — **fix toggle ăn (hết `DTERR`), và log chốt nốt cơ chế làm chết bulk-OUT.**
  (1) **`first bulk-IN failure` không còn xuất hiện** (lượt trước là `DTERR`) ⇒ bảng data-toggle theo endpoint đúng; `[net-bridge] first e1000 RX len=64` ✓, `[hv-net-rx] first L2Recv tid=5 result=ok` ✓.
  (2) Bulk OUT vẫn hỏng, nhưng **theo trình tự**: TX đầu tiên **thành công** (`[dwc2-usb] TX packet transmitted OK` + `[net-bridge] first e1000 TX accepted=true`) *trước* cơn bão lỗi HID; sau đó `[lan9514] first bulk-OUT failure: XACTERR - transaction error` và `dropped a frame the driver refused 8x (status 1)` ⇒ **trạng thái bị nhiễm**, không phải xác suất.
  (3) Ngay trước lần hỏng đầu tiên: `control setup failed … bRequest=0x09 (SET_REPORT, control OUT có split) … XACTERR` ⇒ gói của một transfer OUT bị abort còn nằm trong **non-periodic Tx FIFO**; core tiếp tục phục vụ endpoint từ dữ liệu cũ ⇒ mọi OUT sau đó lỗi cùng kiểu. Đối chiếu: Linux `dwc2_hc_cleanup` flush Tx FIFO của kênh OUT sau abort (`if (!chan->ep_is_in)`), U-Boot chỉ flush lúc core init (nên run không abort OUT thì không cần).
  **Đã sửa (lượt này):** `flush_tx_fifo()` (`GRSTCTL.TXFFLSH|TXFNUM_ALL`, chờ bounded, có witness một-lần) + `flush_tx_fifo_after_abort(ch)` gọi ở **ba** chỗ: nhánh lỗi của `channel_outcome` (CHHLTD + bit lỗi), `channel_timeout` (đọc `EPDIR` **trước** khi halt vì halt xoá nó), và nhánh bỏ cuộc sau 50 NAK của `bulk_transmit`.
  **Kiểm chứng:** `cargo test -p driver-dwc2-usb --lib` **11/11 PASS**; `check`/`clippy` cho `aarch64-unknown-none-softfloat` (kể cả `loop-trace,loopback-diag`) sạch cảnh báo mới; QEMU `raspi3b` machinery gate **PASS** và log gate có **đúng 1** dòng `[dwc2] flushed the Tx FIFO after an aborted OUT transfer` ⇒ nhánh mới **có chạy thật** (không chỉ compile).
  **Ảnh deploy:** `tools/rpi3-netboot/root/cellos.uimg` SHA-256 `b64ba8ee4850049afd9f0885926007aa9489937ce1da0d97e2d972e54f7584f0`, payload
  `kernel8.img` `16af65fb8a1d1a2d4b2bce9e6783e206bb6145b9f1e12d86ddaf2c1f41c99914` (60 616 704 B), QUIET; rollback `cellos.uimg.before-txflush-7a02e072` (= ảnh lượt 53).
  Board cần đọc: (a) `[dwc2] flushed the Tx FIFO after an aborted OUT transfer` (một lần) rồi **sau đó** `first e1000 TX accepted=true` **lặp lại được** (nhiều frame ra dây) — đây là câu trả lời quyết định cho giả thuyết FIFO; (b) `first guest TX accepted=true status=0` + `[hv-virtio-host] net-tx-complete`; (c) `[net-bridge] first guest RX len=…` + `[hv-virtio-net] first RX frame len=… into the guest`; (d) `ping -c 3 <gateway thật>` có reply. Nếu (a) vẫn có `XACTERR` **sau** khi đã flush ⇒ loại giả thuyết FIFO và hướng tiếp theo là lịch split/non-periodic (bão XACTERR của đường HID control xen vào).
  Board (lần 54, 2026-10-08, ảnh `b64ba8ee…` = lượt 53) — **flush chạy đúng, `DTERR` đã hết, và log lộ ra thứ lớn hơn: core đã DISABLE root port.**
  (1) `[dwc2] flushed the Tx FIFO after an aborted OUT transfer` in **đúng một lần** ⇒ nhánh mới chạy thật; `first bulk-IN failure` **không còn** (mismatch toggle đã hết).
  (2) **`HPRT0=0x0000140B` trong mọi dump lỗi của lượt này**, so với `0x0000100F` ở mọi lượt trước ⇒ **bit2 `PRTENA` = 0: port bị vô hiệu.** Đây là lời giải cho *cả họ* lỗi "bền": port chết thì mọi transfer đều hỏng **không có status** — `first bulk-OUT failure: no status bit, the poll budget ran out`, `control … wIndex=0x114 … unknown`, `TIMEOUT ch=3/4`.
  (3) Bản chất lỗi bulk-OUT **đổi sau flush**: `XACTERR` → `no status bit` ⇒ giả thuyết FIFO đúng phần của nó (không còn XACTERR), nhưng trần lúc đó là port đã chết, không phải bulk.
  (4) Không code nào của ta ghi `HPRT0` sau boot (`dwc2.rs` chỉ power-on/`reset_port` trong đường boot) ⇒ **core tự xoá `PRTENA`** (sự kiện bus, USB 2.0 §11.8 disconnect). `PRTCONNDET`/`PRTENCHNG` latch từ boot và **chưa bao giờ được xoá** nên log không đọc được *thời điểm*.
  (5) Trả lời câu hỏi vận hành (ping từ Cellos shell **trước** `hv`): **không làm được hôm nay** — `/bin/ping` chỉ là stub (`ICMP socket data path not yet wired (Phase 15 data path)`) và host stack không có IP (`[net] DHCP: deconfigured`). Nhưng câu hỏi gốc ("đường host có sống không, tách khỏi guest") đã có câu trả lời ngay trong log: frame của **chính net service** đi ra được (`first e1000 TX accepted=true` + `TX packet transmitted OK`) và có frame vào (`first e1000 RX len=64`) **trước** khi port chết ⇒ guest không phải nguyên nhân. Cách kiểm tra host-side rẻ nhất mà không cần viết code: cho PC chạy DHCP server ⇒ Cellos lấy lease và in `[net] DHCP acquired — IP configured` (chứng minh cả hai chiều, không cần guest).
  **Đã sửa (lượt này):** observability cho port — `port_enabled()` (đọc thanh ghi local, không tốn transfer), `clear_port_change_bits()` gọi **sau enumeration** (change-bit của boot không còn che thay đổi về sau; **không** ghi `PRTENA` vì field này write-1-to-change), vòng phục vụ theo dõi port **cùng nhịp `LINK_POLL_TURNS`** và in **một lần mỗi lần chuyển** (`[dwc2] root port DISABLED (HPRT0.PRTENA=0) …` / `[dwc2] root port enabled`); hai dòng một-lần của bulk thêm hậu tố `(the root port is DISABLED, PRTENA=0)` khi đúng.
  **Kiểm chứng:** `cargo test -p driver-dwc2-usb --lib` 11/11 PASS; `check`/`clippy` cho `aarch64-unknown-none-softfloat` (kể cả `loop-trace,loopback-diag`) sạch cảnh báo mới; QEMU `raspi3b` machinery gate **PASS** (log gate có witness flush; **không** có dòng `root port DISABLED` giả — QEMU không có thiết bị nên port không đổi trạng thái).
  **Ảnh deploy:** `tools/rpi3-netboot/root/cellos.uimg` SHA-256 `cc5686bfdaf45cbdad483c35755eff1e73976865953408d9b12c2daf8d40ab3c`, payload
  `kernel8.img` `4061316e4c162eb13fec7fff5d51cc97f8ee35045eae8e4dd055249bb5e3470b` (60 616 704 B), QUIET; rollback `cellos.uimg.before-portstate-b64ba8ee` (= ảnh lượt 54).
  Board cần đọc: **thời điểm** `[dwc2] root port DISABLED …` so với cơn bão HID/LED (ngay sau `iface 0 LEDs 0x00`/`SET_REPORT` lỗi? hay muộn hơn?) và so với frame TX đầu tiên. Hai nhánh:
  (a) **tái diễn mỗi lượt cùng một chỗ** ⇒ bước kế là **recovery thật**: tách đường bring-up trong `cell_main` thành hàm gọi lại được (`reset_port()` root → re-enumerate hub + bàn phím + LAN9514 → `init()` lại chip → reset bảng toggle/FIFO), rồi gọi nó khi phát hiện port chết;
  (b) **chỉ một lần rồi thôi** ⇒ nghi nguồn ngoài (glitch bus/nguồn phía LAN9514) và cần snapshot `HPRT0` + `GINTSTS` **ngay trước** lần hỏng đầu tiên để bắt dấu.
  Lưu ý kèm: kể cả khi (a)/(b) xong, `ping` từ guest vẫn phụ thuộc TX bulk sống qua cơn bão HID — nếu cần một phép thử **tách HID khỏi NIC**, cách rẻ là ảnh chẩn đoán tạm **không poll HID** (feature `no-hid-poll`): NIC còn TX được hay không sẽ trả lời dứt điểm "HID split có phá non-periodic không".
  Board (lần 55, 2026-10-08, ảnh `cc5686bf…` = lượt 54) — **disconnect tái diễn (lần này có bằng chứng core), và lộ một lỗi vận hành nặng do chính tôi thêm vào: flood + treo driver.**
  (1) **Core báo mất thiết bị:** `GINTSTS=0x24000029`/`0x24400029` (bit29 = `GINTSTS_DISCONNINT`, đã có tên trong `regs.rs`) so với `0x05000029` lúc chạy được ⇒ **disconnect detected**; `HPRT0=0x00000400` — **đúng bằng giá trị đọc lúc boot trước khi cấp power** (`HPRT0  =0x00000400`): `PRTCONNSTS=0`, `PRTENA=0`, `PRTPWR=0` ⇒ port về trạng thái mặc định sau reset, thiết bị không còn; `HFNUM=0x1D4B3FFF` **giống hệt trong mọi dump** ⇒ bộ đếm frame không chạy (lượt 54: `0x140B` = còn connect nhưng ENA=0, HFNUM vẫn tăng). Hai trạng thái chết khác nhau giữa hai lượt ⇒ sự kiện bus, không phải một đường code cố định.
  (2) **Mốc thời gian:** dump lỗi *đầu tiên* của serving loop (ch3) đã có port chết, trong khi các lần đọc thanh ghi LAN9514 ngay trước đó (`PHY link up`, `read-back …`, `Hardware MAC`) đều thành công ⇒ port chết **trong/ngay sau bring-up**, chưa khoanh được chính xác (đã thêm mốc, xem (4)).
  (3) **Không phải power management của ta:** grep toàn bộ nguồn cho `PM_USB`/`USB_PWR`/`power_domain`/`0x3f100000` — không có chỗ nào chạm domain USB của SoC.
  (4) **Lỗi vận hành do tôi thêm ở lượt 53 (đã sửa):** `[dwc2] bulk_transmit: exceeded 50 NAK retries` in **hàng trăm lần** — mỗi lần bỏ cuộc tốn 50 attempt × `sys_yield()` (~20 ms) ≈ 1 s, và net service còn retry 8 lần/frame ⇒ console ngập và mỗi frame tốn hàng chục giây; guest gõ `ifconfig`/`ping` bị chôn trong log. Kèm đó: dòng `root port DISABLED` của lượt 54 in ra **có dấu `\` và xuống dòng** (tôi để `\\` trong generator) ⇒ dòng bị cắt làm hai.
  **Đã sửa (lượt này):** (a) `BULK_NAK_RETRIES = 8` (thay 50) + dòng bỏ-cuộc **một lần** (`the endpoint kept answering NAK`) — net service đã retry frame nên trần nằm ở đó; (b) **fail-fast khi port chết**: `port_refuses()` ở đầu `bulk_transmit`/`bulk_receive` (một dòng một-lần `a transfer was refused: the root port is disabled`) ⇒ không attempt nào tốn ngân sách; (c) vòng phục vụ đọc `HPRT0.PRTENA` **mỗi lượt** (thanh ghi local): khi port chết thì **bỏ hẳn** HID poll + LED flush và trả `STATUS_NOT_READY` cho request NIC (net service giữ frame theo chính sách lần 50) — trước đây vẫn thử hết ngân sách trên bus đã chết; (d) sửa dòng DISABLED thành một dòng; (e) **khoanh thời điểm**: sau enumeration in `[dwc2] end of bring-up: root port enabled|DISABLED` + nếu đã chết thì `the serving loop starts with the root port already disabled`.
  **Kiểm chứng:** `cargo test -p driver-dwc2-usb --lib` 11/11 PASS; `check`/`clippy` cho `aarch64-unknown-none-softfloat` (kể cả `loop-trace,loopback-diag`) sạch cảnh báo mới; QEMU `raspi3b` machinery gate **PASS** và log gate in **đúng hai** dòng mới (QEMU không có thiết bị downstream ⇒ `end of bring-up: root port DISABLED` + `serving loop starts with the root port already disabled`) ⇒ nhánh fail-fast chạy thật, không có flood.
  **Ảnh deploy:** `tools/rpi3-netboot/root/cellos.uimg` SHA-256 `8a366e932a1764d081533398774a6e40a034449263d94963c40fa57507119a24`, payload
  `kernel8.img` `a6247f3a4c2ad9ebb53e4241bf7abaf82936ed504fcebfedfb8ea18f36984287` (60 616 704 B), QUIET; rollback `cellos.uimg.before-deadport-cc5686bf` (= ảnh lượt 55).
  Board cần đọc: (a) `end of bring-up: root port enabled` **có** in (kỳ vọng) — nếu **DISABLED** ngay đó thì port chết *trong* bring-up; nếu `enabled` mà sau đó có dòng `root port DISABLED` thì port chết *trong serving loop*; (b) console **không còn** flood `exceeded 50 NAK retries` (giờ tối đa một dòng `kept answering NAK` + một dòng `a transfer was refused`), và prompt guest vẫn gõ được trong lúc bus chết; (c) nếu port chết **lặp lại mỗi lượt ở cùng một chỗ** ⇒ bước kế chắc chắn là **recovery re-enumeration** (tách bring-up trong `cell_main` thành hàm gọi lại được: reset port → re-enumerate hub + bàn phím + LAN9514 → `init()` lại chip → reset bảng toggle/FIFO); (d) nếu cần tách biến "HID split có phá bus/port không": ảnh chẩn đoán `no-hid-poll` (bỏ hẳn poll HID) — NIC còn sống hay không là câu trả lời.
  Board (lần 56, 2026-10-08, ảnh `8a366e93…` = lượt 55) — **mốc mới khoanh được cửa sổ: port chết mà driver KHÔNG hề phát transfer USB nào; và log tự bác bỏ "core bị reset".**
  (1) Trình tự: `end of bring-up: root port enabled` → `[dwc2-usb] Entering NIC + HID serving loop...` → `[usb-hid] registered as an input event source` → `[input] registered raw event source kind=2` → `[dwc2] root port DISABLED …`. Ba bước giữa là **IPC thuần** (đăng ký input source, handshake attach, retry request hoãn) — không có transfer USB nào ⇒ **không phải một transfer của ta** làm port chết.
  (2) **Core không bị reset:** trong mọi dump lỗi của cả lượt 55 lẫn lượt này, `GINTMSK=0x23000008` **vẫn đúng giá trị ta lập trình**; soft reset (`GRSTCTL.CSRST`) xoá `GINTMSK` (Linux phải ghi lại sau `dwc2_core_reset` vì lý do đó) ⇒ core còn sống và giữ cấu hình, chỉ **port mất thiết bị**; `HPRT0=0x00000400` (đúng giá trị đọc lúc boot *trước* khi cấp power) và `HFNUM=0x1D4B3FFF` đóng băng là **hệ quả**, không phải nguyên nhân.
  (3) Guard của lượt 55 chạy đúng như thiết kế: **hết flood**; chỉ `first e1000 TX accepted=false` + **một** `dropped a frame the driver refused 8x (status 2)` (`status 2 = STATUS_NOT_READY` ✓), console vẫn dùng được, guest gõ `ifconfig`/`ping` bình thường.
  (4) Loại bớt nghi phạm: các ghi thanh ghi LAN9514 của ta **trùng U-Boot** (`LED_GPIO_CFG=0x01110000`, `BURST_CAP=5`, `BULK_IN_DLY=0x2000`, `HW_CFG BIR|MEF|BCE`, `AFC_CFG=0x00F830A1`, bit `MAC_CR`); front-end `/bin/lan9514` **không có MMIO** (capability-free, chỉ relay envelope) ⇒ không chạm được controller; và không nguồn nào trong cây ghi `HPRT0`/`PCGCCTL`/`GRSTCTL` lúc chạy (`dwc2.rs` chỉ trong đường boot).
  **Đã thêm (lượt này):** `core_state()` in `HPRT0`/`GINTSTS`/`GINTMSK`/`GRSTCTL` **ngay tại dòng transition** ⇒ lượt sau tự trả lời "core reset hay chỉ mất port" từ log, không cần suy luận.
  **Kiểm chứng:** 11/11 test; `check`/`clippy` cho `aarch64-unknown-none-softfloat` (kể cả `loop-trace,loopback-diag`) sạch cảnh báo mới; QEMU machinery **PASS** (log gate: `GINTMSK=0x23000008` + `end of bring-up: root port DISABLED` như kỳ vọng — QEMU không có thiết bị).
  **Ảnh deploy:** `tools/rpi3-netboot/root/cellos.uimg` SHA-256 `7389e532975e3d3ab4592ccfa28af7a0b944142f69ca9d8d765dd12d6c989225`, payload
  `kernel8.img` `ef748240ebe62f78e93cf4274a1e77a56161e59ee2c02092d3ffc9d94c2f9298` (60 616 704 B), QUIET; rollback `cellos.uimg.before-corestate-8a366e93` (= ảnh lượt 56).
  **Kết luận tạm + hai việc song song:** thiết bị LAN9514 (upstream) **tự rời bus**; lớp khả năng còn lại là điện/nguồn (brownout khi chip bật TX, hoặc nguồn cấp yếu + bàn phím wireless cùng rút trên cùng rail 3V3) hoặc glitch bus. Phép thử **rẻ, không cần code**: (a) cấp Pi bằng nguồn 5V ≥2.5 A thay vì cổng USB của PC; (b) một lượt boot **rút bàn phím USB**: port còn sống ⇒ nghi điện, vẫn chết ở cùng chỗ ⇒ loại điện. Song song, fix bền cho *triệu chứng* (bất kể nguyên nhân) là **recovery re-enumeration**: tách đường bring-up trong `cell_main` thành hàm gọi lại được (`reset_port` → re-enumerate hub + bàn phím + LAN9514 → `init()` lại chip → reset toggle/FIFO → đăng ký lại front-end), gọi khi phát hiện port chết; **chưa làm** vì đây là thay đổi lớn và cần biết nguyên nhân có tái diễn cùng chỗ hay không.
  rollback `cellos.uimg.before-lanattach-ae92401e`. Còn lại trên board: `[net] DHCP acquired — IP configured` (lease) ⇒ rồi
  mới tới guest `ifconfig eth0 up` + RX đếm lên, rồi quay lại mốc `--app`. Board cần xác nhận **LAN của host trước VM**:
  `ID_REV=0xEC00xxxx bytes=4` (kỳ vọng cũ `0x9504xxxx` là sai: `0xEC00` mới là LAN9512/9514), `scratch write=1 read=0x5A5AA5A5`,
  `PHY link up … AN complete`, `[net-bridge] first e1000 TX … accepted=true` + `first e1000 RX`, và `[net] DHCP` có lease.
  Đánh giá: LAN9514 đã ngốn nhiều vòng boot; nếu scratch test không sáng thì **park** subsystem này (ghi evidence
  vào TODO) và quay lại mốc `--app` — mốc đó không cần LAN của host.
  Board 2026-10-04 (lần 13): **fix LAN9514 làm chết bàn phím** — đọc thanh ghi thật ⇒ các vòng chờ thành
  transfer USB thật ⇒ `init()` nằm *trong* vòng quét port (port 1) nên bàn phím ở port 4 không bao giờ enumerate
  (mất LED, mất phím) trong khi phần còn lại vẫn boot. Đã sửa: nhớ candidate trong lúc quét, init **sau** vòng
  quét; siết bound (CSR 50, PHY 20, soft-reset 200, link 100); chip ID lạ ⇒ báo rồi **bỏ qua** thay vì cấu hình
  qua addressing vừa fail; thêm probe addressing (direct `0x00/0x64/0x6C` vs CSR `ADDRL/ADDRH`) — vì lần này
  đọc được `0xB8021E11` (khác 0 ⇒ vendor read đã thông) nhưng không khớp ID LAN9514.
  Payload `ac87d4a58c55c9d1417789a5fe7d96ed72431d76d7c23852e8254c6bbcdc2204` (lanorder) đã deploy + machinery PASS;
  rollback `cellos.uimg.before-lanorder-88743ced`. Board cần xác nhận: bàn phím trở lại (LED + gõ) **và** dòng
  probe `[lan9514] probe direct[0x00]=… [0x64]=… [0x6C]=… csr[ADDRL]=… csr[ADDRH]=…` để chốt addressing.
  Board 2026-10-04 (lần 12): **Ethernet của Pi chưa từng chạy — hai số bị đảo trong LAN9514.** Log
  `unexpected chip ID: 0x00000000` + MAC `B8:27:EB:12:34:56` (placeholder trong driver) ⇒ `ADDRL/ADDRH` không
  đọc được. `read_reg` phát vendor request `0xA1` (là *ghi*) theo chiều IN và đặt index vào `wValue`; chuẩn
  LAN95xx là `0xA0` đọc / `0xA1` ghi, index ở `wIndex`, `wValue=0` ⇒ mọi lần đọc fail, buffer 0, nên soft-reset
  MAC / PHY AN / `MAC_CR` TXEN|RXEN đều ghi vào chip không nghe. Bulk-OUT vẫn xong ⇒ TX "OK" nhưng frame chết
  trong chip, `first e1000 RX` chưa từng xuất hiện, `eth0` RX=0. Đã sửa đúng protocol + thêm log chip MAC và
  `PHY link up/down (BMSR)` + bound mọi vòng chờ (chip không clear BUSY thì fail thay vì treo cell).
  Payload `88743ced865fa4e55710099be43544b5491f8342a51ec554386d23efb8a3bb9e` (lanfix) đã deploy + machinery PASS;
  rollback `cellos.uimg.before-lanfix-2c4466d6`. QEMU **không** kiểm chứng được (raspi3b mô phỏng không có USB
  Ethernet) ⇒ board là nơi xác nhận: mong đợi `Verified SMSC/Microchip LAN9514` (hết WARN chip ID), `chip MAC`
  thật, `PHY link up … auto-negotiation complete`, rồi `[net-bridge] first e1000 RX` khi có traffic LAN và
  `ifconfig eth0` RX đếm lên.
  Board 2026-10-04 (lần 11): **phím chậm hơn khi NIC sống** — mọi lần guest thoát idle (`WFI`/preempt) chạy
  theo thứ tự: lookup compositor → **IPC có timeout tới Net Cell** (`try_receive`) → mới `forward_input_events`,
  nên mỗi ký tự echo phải xếp sau một round-trip mạng; NIC lên ⇒ guest thoát idle thường xuyên hơn nhiều ⇒ lộ rõ.
  Đã sửa: input chuyển tiếp **trước** RX poll; RX poll có guard `NetDev::rx_available` (đọc avail ring, chỉ hỏi
  Net Cell khi guest có buffer nhận) ⇒ bỏ IPC khi guest không nhận được **và** sửa lỗi mất frame im lặng (thứ tự
  cũ lấy frame rồi vứt khi `push_rx_frame` không có buffer). Guard là superset của điều kiện fail sớm của
  `push_rx_frame` nên không thể giữ lại frame mà guest nhận được. Payload
  `bf10cced6dc655b52602ddabc3ed7a093ef3a7d506e09877f61ec3c867827d1c` (rxguard) đã deploy + boot/machinery PASS
  (`NET_UP_OK` với `ifconfig eth0 up`); rollback `cellos.uimg.before-rxguard-16ec1ae2`.
  Board 2026-10-04 (lần 10): **guest shell có tools trên board** (`ls`, `uname -a`, `ps`, `ifconfig -a` chạy
  thật; `initrd=8744017 B` = bản repack) và **NIC của guest xuất hiện**: DTB khai báo virtio-mmio net (slot 2)
  + cell ARM có `NetDev`/`virtio_net.rs`, nhưng bypass Alpine `/init` ⇒ không ai modprobe ⇒ chỉ có `lo`. `/init`
  nay `modprobe virtio_mmio` + `virtio_net`; QEMU boot gate xác nhận `virtio0..virtio4` và
  `eth0 Link encap:Ethernet HWaddr 52:00:00:00:BB:00` (`/sys/class/net` = `eth0 lo`). Chưa chạy DHCP client nên
  eth0 chưa cấu hình — đường traffic tới net service của host thuộc mốc app-launch.
  Payload `16ec1ae2b256fef082f2ce2e968d858e2833a0e0680ff0a36a2b313529510158` (guestnet) đã deploy +
  machinery PASS; rollback `cellos.uimg.before-guestnet-26ff8c02`.
  Board 2026-10-04 (lần 9): **guest shell "not found" = initramfs thiếu symlink applet**, không phải PATH:
  `initramfs-virt` gốc chỉ có `busybox kmod modprobe sh`, không có `bin/ls|cat|ps`, không `/etc/profile`; bảng
  applet của busybox có đủ 66 tên cần (nên `busybox ls /` chạy, `ls` thì không). Đã sửa:
  `tools/prepare-rpi3-shell-initramfs.py` repack initramfs guest thêm 73 symlink (kiểm tra từng tên với bảng
  applet) **+ `/init`** mount `/proc`,`/sys`,`/dev` rồi `exec /bin/sh`; `RDINIT` của profile volatile đổi
  `/bin/sh` → `/init` (đây cũng là chỗ app-launch sẽ exec); kernel đã cấp PATH mặc định cho `rdinit` nên
  không cần profile script. Thêm hook `INITRD_OVERRIDE` (giống lane x86) và gate bằng initramfs chạy
  `ls / uname -a ps mount ifconfig -a` trước `exec /bin/sh` → QEMU `boot` gate PASS với `LS_ROOT_OK`,
  `PS_OK`, `MOUNT_TABLE_OK`, `IFCONFIG_OK`. **Check này bắt được bug mà `ls` archive không thấy**: bản đầu
  link `sbin/mount -> busybox` (target cùng thư mục) ⇒ resolve `/sbin/busybox` không tồn tại ⇒ guest báo
  `mount: not found`; nay target tính tương đối theo thư mục của link. Lưu ý gate: QEMU chạy guest chậm ~4×
  so với board (70 s guest-time ở 300 s wall) nên `boot` gate cần `BOOT_WINDOW=900`; các lần `machinery`
  trước chưa từng đòi prompt guest nên cửa sổ 300 s trông như fail.
  Board 2026-10-04 (lần 8): **phím đã vào tới guest** — `~ # uname -a` tới **nguyên vẹn** (không còn
  `[71;11R`) và shell trong guest trả `/bin/sh: uname: not found`, tức guest đã parse + thực thi dòng lệnh
  (chỉ thiếu applet `uname` trong busybox của initramfs Alpine gốc); filter tự báo một lần
  `[hv] suppressed 1 cursor-position quer(ies)…`; VM tạo bình thường ⇒ smoke + cache maintenance OK.
  Chuỗi bàn phím USB → input → hypervisor cell → PL011 guest → shell guest **đã chứng minh trên phần cứng**.
  Còn lại: guest chứa gì (busybox tối giản) và tốc độ — thuộc mốc một-app-một-VM + speed.
  Board 2026-10-04 (lần 7): **smoke EL2 fail ngắt quãng** — cùng payload `42d2b22d` trước đó đã PASS và
  boot được guest, nay `exit=[0x82000005,0x200]` với **mọi input y hệt** (root/page/s2/insn) và QEMU PASS
  cùng kernel ⇒ hiệu ứng **cache thật** (TCG không có cache): kernel ghi blob/ảnh guest qua mapping EL1,
  guest fetch qua Stage-2 (VA+ASID khác) nên `dc cvau` + `ic ivau` theo VA **không phủ alias**. Đã sửa:
  smoke clean trang blob tới PoC + `ic iallu` trước khi vào guest; `run_vcpu` clean **cả cửa sổ guest RAM**
  một lần cho mỗi VM ở lần entry đầu (phủ luôn ảnh guest thật, vì `write_guest_memory` copy không có cache
  maintenance); thêm log `EL1 leaf0/insn` khi smoke fail để phân biệt "hai regime lệch" vs "phần mềm khớp,
  walker không". Payload `26ff8c02…` (shellfix) đã deploy, machinery PASS; rollback `cellos.uimg.before-shellfix-0cce2388`.
  Board 2026-10-04 (lần 6): fix input **hiệu quả** (không còn `[heartbeat] … missed liveness deadline`,
  không `[input] dropped …`) và guest lên `~ #`. Chữ nhiễu còn lại là **truy vấn con trỏ**: guest phát
  `ESC[6n`, terminal host trả `ESC[71;11R`, reply về muộn nên ash đọc thành lệnh (`/bin/sh: [71: not found`).
  Đã chặn hai đầu: `Pl011::filter_tx` bỏ request, `push_host_rx` bỏ reply (mọi thứ khác — mũi tên, text —
  đi nguyên), có đếm + log định kỳ + 4 host test; payload `42d2b22d…` đã deploy.
  Board 2026-10-04 (lần 5): guest lên `~ #` trên board nhưng **net bị heartbeat giết** khi guest chạy:
  chuỗi `task 7 Sending{6}` → `task 6 Sending{5}` — vì `input` gửi **blocking** tới cell đang giữ focus
  (guest bận nên VMM chỉ poll input ở WFI/preempt) ⇒ kẹt ngược ra dwc2 rồi net. Đã sửa: `dispatch` dùng
  `try_send` như mouse, có bộ đếm drop (`[input] dropped N keyboard event(s)…`), bỏ helper blocking;
  payload `c09e8236…` đã deploy. Gõ trong guest vẫn nhiễu/chậm: `ESC[71;11R` là **truy vấn con trỏ của
  shell trong guest** (busybox ash) bị trả về như input — mô hình một-app-một-VM (không shell tương tác)
  sẽ hết; độ chậm là chi phí trap-and-emulate (mỗi ký tự vài trap qua Stage-2), không phải lỗi.
  Board 2026-10-04 (lần 4): **`ls /bin` sạch** (fix dedup xác nhận trên board) và **`hv` gõ từ prompt dựng
  guest thật**: `[hv] guest profile: alpine (128 MiB)` → `hv: hypervisor cell started, tid=13` → `[hv] VM
  created vm_id=1` → `kernel=34603008 B initrd=8743468 B (streamed)` → `[hv] vCPU ready — entering run
  loop` → Linux 6.12.13 trên Cortex-A53. LED vẫn theo phím; `LS` chữ hoa bị `DENY launch edge` (đúng
  thiết kế). Còn lại: thấy `~ #` của guest rồi gõ phím **trong guest**.
  Board 2026-10-04 (lần 3): **bàn phím + LED lock + HDMI console đều chạy thật** — receiver `2a7a:8a53`
  ở cổng 4 enumerate (2 interface, `driving 2 HID interface(s)`), gõ `ls`/`ls /bin` ra kết quả, LED đổi
  theo phím (`iface 0 LEDs 0x02/0x00/0x01`); đường hiển thị chạy trọn: `[bcm-display] validated framebuffer
  registered … 1280x720` → `[fb-console] … first log batch received` → `[compositor] first scanout flush
  submitted` → `[bcm-display] first scanout flush completed`. Tier 3 vẫn idle, `hv` để dựng guest.
  Ghi nhận (không chặn): interface 1 của receiver không khai LED output report và STALL `SET_IDLE` (driver
  log một lần rồi bỏ qua). `ls` in trùng vài mục `/bin` (FAT + CellosFS overlay) — **đã sửa**: một helper
  `push_unique` dùng cho mọi nhánh listing (case-insensitive, có host test), payload `dbdea09f…` đã deploy. Còn lại của HDMI: đưa **surface virtio-gpu của guest** vào compositor (đường host đã
  chạy), thuộc mốc một-app-một-VM.
  Ảnh full-option (2026-10-04): `--ui --ai --supervisor` nay **đóng gói đủ cell** (compositor, bcm-display,
  fb-console, config, ai, supervisor) — trước đó `--ui` chỉ bật feature ở init nên image spawn cell
  không có trong ảnh; cell option phải build ở invocation **có default features** (driver-bcm-display giữ
  `ostd`/HAL sau default feature). Host gate PASS, log có `[bcm-display] validated framebuffer registered`
  + `[compositor] hardware cursor active` + `[fb-console] compositor found`; payload `bf80f5f3…` đã deploy.
  USB (2026-10-04): guard "hub stopped answering" của tôi hỏi **sai câu** — dùng `get_port_status(0)`
  (hub-class, wIndex là port) nên hub STALL và guard dừng liệt kê ngay ở port 2, bàn phím port 4 chưa
  từng được thử. Nay `Hub::is_responsive()` dùng **standard device GET_STATUS** (`0x80`, 2 byte), và
  `attach_port` trả `Attached/Empty/Failed` để chỉ probe sau lỗi thật. Payload `1caa26ee…` đã deploy,
  host gate PASS; đường USB chỉ kiểm chứng được trên board (QEMU không có thiết bị downstream).
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
