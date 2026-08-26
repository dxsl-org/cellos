# Cellos — Phân tích hệ thống toàn diện & Root Cause (tuần 2026-07-06)

> Mục đích: tuần phân tích, không code. Quét toàn bộ docs + 127 plan `.agents/` + nợ kỹ thuật code + 390 commit lịch sử git. Output = root cause, blind spot, danh mục ưu tiên để tuần sau lập plan chi tiết G1–G3.
>
> Nguồn: 4 báo cáo agent (docs-audit, plan-inventory, tech-debt-register, git-history) — tổng hợp bên dưới, trích dẫn file:line giữ nguyên.

---

## 0. Bức tranh hiện trạng (TL;DR)

- **G1 gần graduation**: 6/8 tiêu chí ✅; còn #4 (peripheral trên board thật) + #6 (chạy RV64/ARM64 SBC thật) — code bring-up VF2/Pioneer/RPi3 xong, **chưa từng chạy trên phần cứng thật**.
- **Hệ thống đang bất ổn ở đúng chỗ vừa refactor**: cuộc di cư driver ra Driver Cells (Kernel Boundary Law) làm gãy input + net; net DHCP chết 1 tuần; working tree 16 file sửa dở là chiến dịch debug virtio-net, kèm 4 debug hack trong kernel, 20 commit chưa push.
- **Nợ lớn nhất không phải code rot mà là "việc dở dang có hệ thống"**: boundary cleanup Phase 05/07/08 đã được unblock nhưng không ai thực thi; Hypha dừng ở P3; docs lệch thực tế hàng loạt.

---

## 1. ROOT CAUSES (5 nguyên nhân gốc, xuyên suốt)

### RC-1. Hợp đồng IPC không được đặc tả — nguồn của ~nửa số bug lặp lại
**Bằng chứng hội tụ từ 3 nguồn độc lập:**
- Git: các file fix nhiều nhất đều là IPC-contract — `kernel/src/task/syscall.rs` 11 fix, `vfs/main.rs` 6 fix (buffer-size mismatch), net TLS path 5 fix riêng lẻ mới chạy được, `input/dispatcher.rs` 3 fix trong 1 tuần, `hypha/core/main.rs` 3 lần viết lại recv-loop.
- Working tree hiện tại: virtio-net Tx phải chế thêm header `[op, len_lo, len_hi]` vì **ranh giới frame không xác định được trong buffer IPC 4KiB có padding**; RX phải chuyển non-blocking để tránh deadlock; hypha đổi `sys_recv(gw)` → `sys_recv(0)` vì mask G18 gây deadlock.
- Lịch sử: buf[0] dispatch đụng postcard discriminant (input); `VfsResponse::Data` >480B trả rỗng **không báo lỗi**; 512B vs 400B chunking; sender-id vs byte-count nhầm lẫn.

**Root cause:** mỗi service tự phát minh framing/discriminant/blocking-semantics riêng trên `[u8; 512..4096]`. Không có spec wire-protocol, không có thư viện framing chung, không có contract test. Bộ ba ABI (`kernel/task/syscall.rs` + `ostd/syscall.rs` + `api/syscall.rs`) churn lock-step 66/38/37 commit — điểm ghép nóng nhất repo.

**Mỉa mai chiến lược:** giá trị cốt lõi Cellos là "typed zero-copy IPC", nhưng tầng message thực tế là các quy ước bất thành văn. Đây là chỗ SAS/LBI đáng lẽ tỏa sáng nhất.

### RC-2. "Done" được tuyên bố ở mức code-complete, không phải verified-on-boot
- Bài học audit 2026-05-30 (23/23 "complete" → thực tế 12 done + 6 partial) **vẫn đang tái diễn**: hàng loạt plan ghi "cargo check clean, QEMU verify deferred".
- `tests/integration/tests/boot.rs` bị fix 9 lần, **2 lần vì âm thầm pass khi boot hỏng** → CI xanh ở repo này là bằng chứng yếu.
- RT "ALL BENCHMARKS PASS" đo trên QEMU TCG — TCG không cycle-accurate, WCET/EDF **về nguyên tắc không thể verify** trên đó (specs/12:229).
- Coverage: roadmap chỗ ghi 96%+, chỗ ghi 75% đang dở (Milestone 1.5), Phase 1 tự tuyên "100% COMPLETE" trong khi 2 milestone con chưa xong.
- Pattern lỗi "im lặng": silent deny của syscall filter, reply rỗng không error của VFS, GetRandom âm thầm fallback xorshift32.

**Root cause:** không có định nghĩa "done" thống nhất gắn với bằng chứng runtime (boot log + integration test trong CI); và bản thân test harness là vùng fix kinh niên.

### RC-3. Tài liệu đã mất vai trò single-source-of-truth
- `security-model.md` (2026-06-21) gọi DMA isolation/CFI/audit/KASLR/SHA256 là "Critical/Absent/Planned" — **tất cả đã ship**. `system-architecture.md` đóng băng 2026-06-05. Đuôi roadmap còn nguyên "Next Steps 2026-05-28".
- Feature đã drop (WASM Tier 2, Dual-VFS, MicroPython, Slint) vẫn được mô tả là live ở 5+ tài liệu; PDR còn coi WASM absence là "risk" cần "mitigation Phase 28".
- Danh tính rối: crate/ELF section/test string = `ViCell`, docs = `Cellos`, prompt shell 3 kiểu ("Cellos >", "ViCell >", "Cellosh>"). Kernel LOC 4 con số khác nhau (5.6K/7.2K/8.7K/11.5K); syscall count 10/26/48/…233. Spec số 14 bị trùng (14-distributed vs 14-viui).
- Spec headline **mâu thuẫn với hệ thống thật**: specs/01+03 tuyên "IPC = direct call ~2-3 cycles, zero-copy"; thực tế là syscall message-passing ~100-1000 cycles (system-architecture.md:120 tự thừa nhận), vtable chỉ là fast-path cho trusted cells. specs/01 §5 (catch_unwind + hot re-link on panic) được specs/12 xác nhận "None of that is implemented".

**Root cause:** cập nhật docs kiểu append-header-only khi ship feature, không có lượt reconcile body. **Rủi ro khuếch đại đặc thù workflow này:** docs là context nạp cho AI agent mỗi session — docs sai chủ động đầu độc mọi quyết định sau này.

### RC-4. Cuộc di cư Kernel Boundary đang kẹt ở trạng thái nửa vời — tệ hơn cả 2 đầu mút
- Đã exile: NVMe, e1000, virtio GPU/net/input/sound ✅. Còn trong kernel: `virtio_blk.rs` (217) + `virtio_pci.rs` (225) + `mmc/*` (~700) + `hotswap.rs` (508) + `snapshot.rs` (395).
- Plan `260624-0630` Phase 05/07 DESCOPED, Phase 08 pending; plan `260624-1118` đã **gỡ hết blocker rồi dừng** — không có plan kế nhiệm. Đây là thread bỏ ngỏ hệ quả lớn nhất.
- Chi phí trạng thái nửa vời đang trả **hàng ngày**: (a) `sstatus.SUM=1` cả đời kernel chỉ để virtio_blk trong kernel chạm được trang VirtIO giờ đã map USER — gỡ bỏ hàng rào phần cứng S/U-mode; (b) code driver tồn tại 2 nơi; (c) mỗi lần exile một driver lại tái phát đúng lớp bug IPC của RC-1 (input rồi, net đang); (d) kernel driver dir vẫn là vùng churn nóng (120 commit/90 ngày) — nợ đang tính lãi.

**Root cause:** migration làm theo từng feature mà không chốt trước Driver-Cell IPC contract chuẩn (RC-1) và không có owner theo đến Phase 08.

### RC-5. Mọi claim định vị sản phẩm đều đang treo trên "chưa chạy board thật"
Danh sách claim **hardware-gated đang dồn toa**: hard-RT/WCET/EDF (TCG không đo được) · Instant-On <100ms (chưa đo warm-boot thật) · peripheral I/O board thật (#4) · SBC run (#6) · MTE trên RK3588 · PMP · G2 P99 latency story. QEMU-first policy đã hoàn thành vai trò lịch sử; giờ nó là nút cổ chai duy nhất chặn cả G1 graduation lẫn tính khả tín của pitch "RT-bounded + never-die". Board đã mua/chọn (RPi3 code xong, VF2/Pioneer code xong) — chỉ thiếu buổi chạy thật.

---

## 2. BLIND SPOTS (điểm mù tư duy — chưa ở đâu ghi nhận)

> **BS#1 RESOLVED 2026-07-07** — quyết định + specced ở `docs/specs/15-kernel-boundary.md §1.4`.
> G1: Tier-1b = trusted first-party (out-of-threat-model); USER-MMIO là perf choice có chủ đích;
> giả định load-bearing, phải nêu mọi nơi claim "LBI isolates cells". G2 untrusted: gate HW per-cell
> (RISC-V PMP + IOMMU/WorldGuard cho virtio-mmio DMA; x86 MPK) TRƯỚC khi chạy Tier-1b untrusted.
> Xác nhận severity: virtio-mmio KHÔNG sau IOMMU → rogue C cell DMA khắp RAM. Nội dung gốc bên dưới.

1. **USER-mapped MMIO trong SAS + Tier 1b = lỗ hổng mô hình.** Diff hiện tại map cửa sổ VirtIO `0x10001000–0x10010000` USER cho Driver Cells. Trong SAS, page table dùng chung → **mọi** U-mode cell đều chạm được MMIO đó về mặt phần cứng; hàng rào duy nhất còn lại là LBI (`forbid(unsafe_code)`). Nhưng Tier 1b C/Zig cells **không có LBI** — một cell C có con trỏ tùy ý là chạm thẳng NIC/blk device, bypass Resource Registry. Cần quyết định kiến trúc: (a) chấp nhận + ghi vào threat model "Tier 1b không được chạy cạnh USER-MMIO", (b) PMP/MPK che cửa sổ MMIO theo cell, hay (c) chỉ map USER khi cell giữ mmio cap (per-cell mapping đã bị bác vì SAS — nhưng MMIO window là ngoại lệ nhỏ đáng cân nhắc).
2. **Fail-open entropy là bom hẹn giờ tái phát.** `GetRandom(214)` fallback xorshift32 im lặng khi thiếu VirtIO-RNG (`syscall.rs:2493-2504`); quy tắc "caller MUST panic" đang enforce **theo từng caller** (TLS làm đúng, broker phải nhớ làm). Một caller mới quên = key predictable. Fix đúng chỗ: fail-closed **tại kernel** (một dòng quyết định, xóa cả lớp bug).
3. **Chuỗi trust "trên giấy" vs mã thật:** signing = dev seed, `FLEET_ROOT_PUBKEY`/`CELL_SIGNER_PUBKEY = [0u8;32]` (fail-closed, tốt — nhưng là release-gate chưa có checklist), PKU wired nhưng key all-zero → enforcement bypass, spec 01 tuyên "mọi Cell phải ký số" trong khi trust thực = path `/bin/`. Không cái nào là bug; **gộp lại là một khoảng cách posture chưa ai nhìn tổng thể**.
4. **Test harness là TCB của quy trình.** Mọi claim never-die/RT/coverage đi qua boot.rs — file bị fix 9 lần, 2 lần false-green. Đầu tư vào harness (assert chặt, fail-loud, chạy trong CI) có đòn bẩy cao hơn viết thêm test.
5. **ViUI 15 plans vẫn "Active" không tiêu chí đóng** — subsystem ngốn plan nhất repo nhưng không có definition-of-done; rủi ro trở thành hố hút effort ở G2 khi compositor/desktop mở ra.
6. **Hypha `os-gaps.md` chính là backlog G1-tail thực sự** nhưng nằm ngoài roadmap: 🔴 G17 net-cell page-fault (scause=13) giữa TLS handshake chưa điều tra; G7 name-service động (chặn tool scaling); G2 SSE streaming; G9 conversation store. Roadmap §D liệt kê gaps tổng quát, còn danh sách được **nhu cầu thật** sắp ưu tiên nằm trong plan Hypha.
7. **Quy trình plan không có nghi thức đóng:** ~12 plan frontmatter `pending` dù việc đã ship; 2 plan bỏ rơi không lời (phase28 WASM/ePMP, x86_32/AArch32); không có `.agents/archive/`. Inventory không đáng tin nếu không đọc body từng file — chi phí lặp cho mọi session AI sau.

---

## 3. DANH MỤC VẤN ĐỀ ƯU TIÊN (input cho tuần sau lập plan)

### 3.0 Trình tự thực thi ĐÃ CHỐT (quyết định 2026-07-06, cùng user)

Nguyên tắc: **làm sạch phần mềm → QEMU regression toàn diện → mới lên board thật** (board debug đắt, chỉ chạy khi tín hiệu sạch). Blind spot không phải phase riêng — 3/4 tan vào các RC.

```
Bước 0  ✅ DONE 2026-07-06 (5 commit, đã push). virtio-net E2E fixed — root cause thật:
        CellHal::share() giả định VA==PA nhưng Tx/RxBuffer nằm ở heap cell (VA loader,
        không identity) → bounce qua grant page; + RecvTimeout allowlist; + try_send
        replies; + platform allowlist regression. DHCP 0.8s, 5/7 net test xanh.
        Entropy fail-closed shipped (phát hiện thêm: virtio_rng kernel = stub → TLS
        từng chạy 100% trên xorshift; fallback giờ sau feature dev-weak-rng).
        Quick wins xong. CÒN LẠI: 2 test đỏ domain input-echo (ký tự nhân đôi khi gõ
        lệnh dài + wget crash sepc=0) → chuyển vào Bước 1; stash cũ chưa drop (chờ user).
Bước 1  RC-2: gia cố DỤNG CỤ ĐO trước — boot.rs fail-loud, integration suite vào CI,
        định nghĩa "done"=bằng chứng runtime. (BS#4 chính là bước này.)
        Lý do đứng đầu: gate "QEMU toàn diện" ở Bước 4 đo bằng harness này;
        harness từng 2 lần false-green → sửa trước khi đo bất cứ thứ gì.
        ── TIẾN ĐỘ 2026-07-06 (2 commit pushed) ──────────────────────────
        ✅ gen_disk.ps1 fail-fast (Assert-BuildOk) + cross-platform (Linux CI chạy được).
        ✅ ci_guard(): prerequisites_ok panic khi $CI set thay vì silent-skip (anti false-green).
        ✅ Definition of Done vào docs/code-standards.md (runtime evidence + ladder 📋→🔨→✅).
        ✅ Input duplication FIXED (surgical: sweep clear current_caller CHỈ nhánh timeout;
           bản vá đầu clear mọi re-park đã regress VFS → revert). Verified: lệnh echo đúng.
        ✅ CI boot-suite job — nhưng SCOPED allowlist (dhcp/tcp/curl/listen) vì:
        🔴 PHÁT HIỆN: suite boot đã ROT RỘNG (~20 test đỏ). boots_to_shell_prompt tới
           được shell nhưng fail assert marker "user_hello"/"U-mode" không còn in;
           nhiều test /tmp-RamFS-write (vwrite failed); mqtt_subscribe. Hệ THỐNG chạy
           OK — ASSERTION trôi vì suite sống ngoài CI. Đây chính là RC-2 hiện hình.
        → CÒN LẠI cho Bước 1 (task kế): DE-ROT suite (đối chiếu ~20 assert với boot
           output hiện tại, mở rộng allowlist dần); điều tra /tmp write (regression thật
           hay drift?); wget cell crash scause=0xc sepc=0 + platform ECAM crash scause=0xd
           @0x30000000 (cell-lifecycle bugs riêng).
Bước 2  RC-1: IPC wire-contract spec + crate framing chung + contract test;
        migrate net/input/vfs/driver-cells.
        PHẢI trước RC-4: mỗi lần exile driver không có contract lại tái phát lớp bug này.
Bước 3  RC-4: Boundary Phase 05 (virtio-blk cell) + Phase 08 (xóa driver kernel) + gỡ SUM=1.
        Quyết BS#1 (USER-MMIO vs Tier 1b) NGAY TRONG plan này — Phase 05 sẽ map thêm
        cửa sổ MMIO USER mới, quyết sau = làm lại.
        ── REVISED 2026-07-07 sau khi verify trạng thái thật (giả định lúc phân tích đã cũ) ──
        RC-4 phần lớn ĐÃ XONG hoặc BỊ CHẶN HỢP LỆ cho G1:
        • Phase 08 (xóa driver đã migrate): DONE — virtio_net/virtio_gpu/nic_e1000/blk_nvme
          đã là Driver Cells, mod kernel đã xóa. Còn lại nic.rs (selector mỏng → cells),
          virtio_pci::init (x86 q35 transitional).
        • Phase 05 (virtio-blk → Cell): CHẶN kiến trúc (S1) — loader gọi block::read_sector
          ở MỌI spawn, nên block device phải kernel-resident TRƯỚC khi cell đầu load được
          (chicken-and-egg). Đây thực ra là NGOẠI LỆ WHITELIST theo Boundary Law (Liedtke
          tiêu chí c: root-of-trust cần trước Cell đầu) — không phải violation. Migrate =
          G2 loader redesign (ramdisk boot + block cell). virtio_blk stack ở lại đúng.
        • mmc: descoped (QEMU không SDHCI).
        • SUM=1 whole-lifetime: CÓ SẴN TỪ TRƯỚC (main.rs:483, không chỉ do net campaign);
          kernel cần để ghi U-mode recv buffer khi giao IPC từ ISR/timer. Narrow = refactor
          cross-cutting (bọc mọi kernel→U-mode write), rủi ro cao / lợi ích G1 khiêm tốn
          (LBI đã cô lập; defense-in-depth thêm chỉ đáng cho G2 multi-tenant).
        → Bước 3 KHÔNG có deliverable G1 sạch giá trị-cao chưa làm. RC-4 = "substantially
          resolved for G1". Việc thật còn lại đều là G2 (loader redesign, SUM narrowing).
   ∥    RC-3 (docs reconciliation sprint) chạy SONG SONG bước 1-3 — delegate được, không chặn.
Bước 4  QEMU regression toàn diện 3-arch (riscv64/aarch64/x86_64) trên harness đã tin được.
Bước 5  Board thật: RPi3 trước (rẻ, code sẵn) → VF2/Pioneer.
        Đo batch: graduation #4/#6 + RT/WCET/EDF + warm-boot <100ms.
Sau đó  BS#3 (trust-chain posture: key thật, PKU tagging, K1→K3) → G2 security checklist.
```

Các bảng P0–P3 bên dưới giữ nguyên làm danh mục chi tiết; P1.3 (board thật) hiểu là Bước 5 — chạy SAU QEMU regression, không song song như bản nháp đầu.

### P0 — Dập lửa & vệ sinh (trước mọi thứ, ~2-3 ngày)
| # | Việc | Vì sao trước |
|---|------|--------------|
| P0.1 | **Chốt chiến dịch virtio-net**: hoàn tất fix E2E (DHCP → tcp_send), gỡ 4 debug hack kernel (`loader.rs`, `syscall.rs`×2, `scheduler.rs`) + println probes, tách `vicell-audio.wav`, commit theo scope, **push 20 commit**, audit 2 stash cũ | Net chết = Hypha, swarm, mọi demo mạng đều chết; tree bẩn 1 tuần là rủi ro mất công |
| P0.2 | Quick wins nợ code: untrack `silo-guest/target` + `tests/boot-unit/target` (205 file), xóa `cells/games/doom`, fix `scheduler.rs:624` `unwrap_or(0)` nuốt lỗi syscall | ~1 ngày, giảm nhiễu vĩnh viễn |
| P0.3 | **Entropy fail-closed tại kernel** (GetRandom trả lỗi/panic khi không có RNG thật, xóa xorshift fallback hoặc gate sau feature dev) | Xóa cả lớp bug bảo mật bằng 1 quyết định |

### P1 — G1 closeout (root cause, ~2-3 tuần)
| # | Việc | Root cause giải quyết |
|---|------|----------------------|
| P1.1 | **Spec + lib "Cell IPC wire contract"**: một tài liệu spec (framing length-prefix, discriminant space, blocking/timeout semantics, versioning, error thay vì silent-empty) + crate dùng chung (mở rộng `libs/agent-proto` hoặc mới) + contract test; migrate dần net/input/vfs/driver-cells | RC-1 — chặn tái phát lớp bug lớn nhất |
| P1.2 | **Hoàn tất Kernel Boundary Phase 05/08**: virtio-blk → Driver Cell, xóa driver chết trong kernel, **gỡ SUM=1**; Phase 07 (MMC) giữ G2 vì cần HW thật | RC-4 — đóng thread bỏ ngỏ lớn nhất, thu hồi security posture |
| P1.3 | **Buổi chạy board thật** (RPi3 trước — rẻ, code sẵn; rồi VF2): graduation #4/#6 + đo batch RT/WCET/warm-boot | RC-5 — một buổi hardware mở khóa cả stack claim |
| P1.4 | **Hypha tiếp tục**: điều tra 🔴 G17 (scause=13 trong TLS handshake), rồi P4 tool-peripheral (G1 showcase) | Forcing function G1; G17 có thể là bug kernel thật |
| P1.5 | Quyết định blind-spot #1: USER-MMIO vs Tier 1b — ghi threat model hoặc chọn cơ chế che | Lỗ hổng mô hình trước khi mở rộng Tier 1b |

### P2 — Docs & governance (song song P1, ~1 tuần, phần lớn delegate được)
| # | Việc |
|---|------|
| P2.1 | **Docs reconciliation sprint**: cập nhật `security-model.md` + `system-architecture.md` (2 trang gây hiểu nhầm nhất); purge WASM/Dual-VFS/MicroPython/Slint khỏi 5+ docs; chốt naming (đề xuất: giữ `ViCell` cho ABI symbols đã đóng băng theo Law 1, `Cellos` cho mọi thứ hướng người đọc — ghi thành ADR); một con số kernel-LOC + syscall-count chuẩn; sửa trùng spec-14; index README thêm specs 12–16; đồng bộ RPi3 vào graduation #4/#6 |
| P2.2 | **Trung thực hóa spec headline**: specs/01+03 — đánh dấu §IPC direct-call & §5 fault-tolerance là *target design* có link tới trạng thái thực, hoặc quyết định build vtable IPC làm mặc định ở G2. Re-score specs/12 (~25-30% là số cũ, checklist đã vượt) |
| P2.3 | **Định nghĩa "done" + đóng plan**: done = boot-log/integration-test evidence trong CI (viết vào code-standards); reconcile frontmatter 12 plan cũ; đóng chính thức 2 plan bỏ rơi; tạo `.agents/archive/` |
| P2.4 | Gia cố test harness: boot.rs fail-loud, integration suite bắt buộc trong CI (bài học "rotted 4 days") |

### P3 — Nền G2 (lập plan tuần sau, làm sau P1)
| # | Việc |
|---|------|
| P3.1 | Decompose `syscall.rs` (3333 LOC) theo domain — trả review-tax trước khi G2 phình ABI |
| P3.2 | Hotswap/snapshot → Supervisory Cell (Boundary Law, XL — cần plan riêng) |
| P3.3 | Security posture G2 checklist: provision key thật (policy/signing), PKU PTE tagging, K1→K3 identity, KMS Cell |
| P3.4 | Name service động (G7 Hypha) + async runtime cho apps — 2 gap §D chặn "real apps" còn lại |
| P3.5 | VFS: async executor (Storage 2.0 Phase 04 deferred) + mkdir/create/rename/chdir kernel path |
| P3.6 | ViUI: định nghĩa tiêu chí đóng G1, đóng plan cluster, dồn phần còn lại vào 1 plan G2 |
| P3.7 | G3: **giữ nguyên kỷ luật gated** — không spec trước hardware; điều kiện tiên quyết (sys_grant_pages) đã xong; chỉ mua RK3588 khi vào G2 để tích lũy 2 tháng RKNN |

---

## 4. Nguyên tắc rút ra (đề nghị ghi thành quy tắc làm việc)

1. **Contract-first cho mọi ranh giới IPC mới** — không cell nào tự phát minh framing nữa.
2. **"Done" = bằng chứng runtime trong CI**, không phải cargo check + checkbox.
3. **Ship feature = cập nhật docs body cùng commit** (docs là context của AI agent — sai là đầu độc).
4. **Không mở migration mới khi migration cũ chưa tới Phase-cuối** (bài học boundary cleanup).
5. **Fail-closed mặc định cho mọi đường degrade** (entropy, VFS reply, syscall filter) — silent fallback là nguồn của các phiên debug dài nhất trong log.

## 5. Phụ lục — vị trí báo cáo nguồn
- Docs audit, plan inventory, tech-debt register, git-history: kết quả 4 subagent trong session 2026-07-06 (nội dung chính đã tổng hợp ở trên).
- Nợ chi tiết: TD-001…TD-012 (tech-debt register) — TD-001/002 = boundary, TD-003 = syscall.rs god-file, TD-006/007 = quick wins, TD-008 = key placeholders.
- Bug đang mở đáng chú ý: #7 nested-trap store-fault (heisenbug, log 260610); Hypha G17 scause=13; net DHCP (đang fix trong working tree).
