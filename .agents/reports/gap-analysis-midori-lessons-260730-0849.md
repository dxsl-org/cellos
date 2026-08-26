# Gap Analysis — 6 khoảng trống còn lại sau plan midori-lessons

**Ngày**: 2026-07-30 · **Nguồn**: khảo sát kiến trúc session 2026-07-30, đối chiếu
`.agents/260727-2101-midori-lessons-cellos/plan.md` (3/8 phase đã merge) với hiện trạng code.
**Mục đích**: đầu vào cho plan kế tiếp; chỉ rõ gap nào bổ sung được vào plan hiện tại.

## Tóm tắt khuyến nghị

| Gap | Bản chất | Khuyến nghị | Đặt ở đâu |
|-----|----------|-------------|-----------|
| 1 | Cell↔cell memory không có tường phần cứng | W^X software ngay (khả thi mọi arch) + **per-domain page table = cơ chế chung với gap 2** | W^X: **plan này** (phase mới, nhỏ) · domain table: ADR + plan sau |
| 2 | Ký chỉ chứng thực nguồn gốc, không chứng thực an toàn | `cellos-sign` enforce F1 cho tier 1 (first-party); dev ngoài KHÔNG cần ký — chạy **tier 2 domain cell** (MMU domain riêng, native speed); confidential build là đường lên tier 1 tự nguyện (G2+) | cellos-sign: **plan này** · domain cell: ADR + plan sau |
| 3 | IPC blocking-send, latency chưa đo | Đo baseline trước, rồi async send/reply trên CQ; vtable IPC cần ADR riêng | **Plan sau** (sau phase 07) |
| 4 | Thiếu mailbox/selective-receive/supervision tree kiểu BEAM | Mailbox là thư viện ostd trên CQ; supervision tree thuộc plan supervisory-cell | **Plan sau** (sau phase 07) |
| 5 | Trần 64 cell, không tới scale BEAM | Reframe: 2 tầng (cell = isolation, future = actor); nâng MAX_CELLS sau khi 08 đo | ADR reframe ngay · nâng trần: **plan sau** |
| 6 | `NoEntry` dev-permissive giữ cap DMA-anywhere | Fail-closed cho path có `with_path_caps` — plan gốc đã defer, giờ đủ điều kiện làm | **Plan này** (phase 09, sau 04) |

---

## Gap 1 — Cell↔cell memory isolation

### Bản chất (giải thích cho người mới đọc spec 16)

Cellos có **hai ranh giới khác nhau về bản chất**:

1. **Kernel ↔ cell**: được phần cứng bảo vệ thật (kernel S-mode, cell U-mode; trang kernel
   không có bit USER). Cell không thể ghi vào kernel — đây không phải LBI, là MMU cổ điển.
2. **Cell ↔ cell**: KHÔNG có tường phần cứng. Mọi trang của mọi cell đều map `USER + WRITE`
   trong cùng một page table (`kernel/src/loader/elf.rs:105-107` tự ghi chú). Một cell chạy
   U-mode có thể tạo con trỏ trỏ thẳng vào heap/stack/text của cell khác và ghi — không fault,
   không log. Thứ duy nhất ngăn điều đó là: *code của cell được rustc biên dịch từ Rust an
   toàn nên không thể diễn đạt con trỏ tuỳ ý* (spec 16 §2).

Nghĩa là tường cell↔cell chỉ vững khi **mọi** cell thực sự không có `unsafe` (chính sách F1).
Thực trạng: 25/71 crate cell có `#![forbid(unsafe_code)]`; shell, vfs, silo, và mọi driver
đều chứa `unsafe`. Mỗi khối `unsafe` trong một cell là một lỗ trên tường — không phải lỗ lý
thuyết: hai khối `unsafe` của VFS (`dispatch.rs:214-232`) là đúng thứ phase 07 phải audit.

### Phương án

**A. W^X bằng page-permission — làm được NGAY, mọi arch** *(khuyến nghị, chi phí thấp)*
Sau khi loader áp xong reloc PIE, flip PTE: `.text` → USER+R+X (bỏ W), `.rodata` → USER+R.
Kernel cần ghi tiếp thì đi qua HHDM alias. Hệ quả: không cell nào (kể cả cell có `unsafe`)
sửa được code/hằng của cell khác hoặc của chính nó → chặn class tấn công code-injection
xuyên cell. Giới hạn thật: heap/stack/data vẫn USER+RW xuyên cell — W^X bảo vệ *code
integrity*, không bảo vệ *data confidentiality*. Đây là bước G2 mà `elf.rs` đã hứa, nhưng
không cần chờ hardware mới — chỉ cần sửa loader + flush TLB.

**B. Protection key — chỉ có trên x86** *(G2, ADR trước)*
x86 MPK/PKU: 16 key; scheme 2-key là đủ — trang của "cell đang chạy + kernel" = key 0,
trang mọi cell khác = key 1; context switch ghi PKRU (~20 cycle) để cấm key 1. Tường
cell↔cell thật trên x86, giữ nguyên SAS. Nhưng: **phần cứng thật của dự án không có** —
RK3588 (Cortex-A76/A55, ARMv8.2) KHÔNG có MTE (cần v8.5+); VF2/Pioneer (RISC-V) không có
PKU tương đương. → MPK chỉ là defense-in-depth cho lane x86, không phải câu trả lời chung.

**C. Per-domain permission table — SAS về địa chỉ, nhiều domain về quyền** *(NÂNG ƯU TIÊN
— cơ chế chung với tier 2 của gap 2)*
Truyền thống SASOS (Opal, Nemesis): giữ MỘT không gian địa chỉ (con trỏ chuyển giao được,
không reloc) nhưng mỗi protection domain có bảng trang riêng map cùng VA→PA với bit quyền
khác nhau; RISC-V và ARM đều có ASID nên switch không flush toàn bộ TLB. Đánh đổi: tốn bảng
trang per-domain + IPC vào/ra domain phải copy qua kernel (grant phải map tường minh) — về
marketing là "quay lại MMU", về kiến trúc vẫn là SAS (địa chỉ thống nhất) + hardware wall.
**Sau quyết định bỏ WASM (2026-07-30), đây là cơ chế duy nhất vừa chặn cell không tin được
vừa chạy trên mọi phần cứng thật của dự án (VF2/Pioneer/RK3588 đều có MMU+ASID, không có
MTE/PKU)** — xem gap 2, tier 2 "domain cell". Một khoản đầu tư kernel phục vụ cả
defense-in-depth (gap 1) lẫn untrusted-app tier (gap 2).

**D. Chấp nhận LBI-by-trust + thu hẹp bề mặt `unsafe`** *(đường hiện tại, cần làm tử tế)*
Audit + tối thiểu hoá `unsafe` trong cell non-driver (shell không có lý do chứa `unsafe`);
driver được ngoại lệ có hồ sơ (SAFETY comment + miri khi chạy được). Gắn với gap 2.

### Khuyến nghị

- **Bổ sung plan này**: phase mới "W^X post-reloc" (phương án A) — độc lập với 02–08, chạm
  `loader/elf.rs` + paging; success criterion: cell thử ghi vào `.text` của cell khác → fault.
- **ADR "layer-2 hardware security"** (spec 16 §8 đã đặt chỗ): chốt B cho x86, C là tuỳ chọn
  G2+ có điều kiện, ghi rõ RK3588/VF2 không có MTE/PKU nên tường data xuyên cell trên
  hardware thật là **không khả thi ở G1-G2** — hệ quả: gap 2 (F1 enforcement) là tường thật
  duy nhất cho data, phải làm nghiêm.

---

## Gap 2 — Trust-by-signing vs trust-by-verification

### Bản chất + trả lời hai phương án được đề xuất

Chữ ký hiện tại (`scripts/sign-cell.py` → `kernel/src/signing.rs`) trả lời đúng một câu:
*"binary này do người giữ khoá tạo ra và không bị sửa"*. Nó KHÔNG trả lời *"binary này an
toàn"*. Midori trả lời được câu thứ hai vì app ship dạng **MSIL — bytecode có kiểu, verify
được bằng thuật toán**; installer verify MSIL rồi mới AOT-compile bằng Bartok. Điểm mấu
chốt: verify được là nhờ **định dạng phân phối** (typed IR), không phải nhờ compiler.

**Phương án "fork rustc thành bộ build riêng cho Cellos": không nên.**
- Fork không thêm được sự *verify* nào — vẫn là *trust* compiler, chỉ khác giờ mình tự gánh
  3–5M LOC compiler + LLVM, mất luôn upstream security fix. TCB to ra chứ không nhỏ đi.
- Spec 16 đã chọn phiên bản đúng của ý tưởng này: **pin nightly (F5) + Ferrocene** (§5.2) —
  một rustc được *chứng nhận* ISO 26262/IEC 61508, drop-in, không phải fork. ARM64 đã
  qualified; RISC-V chờ 12–24 tháng.

**Phương án "chương trình cài đặt + kiểm tra an toàn như Midori": đúng hướng nhưng phải
tách làm hai, vì ELF native không verify được.**
Không tồn tại thuật toán kiểm tra memory-safety của một ELF đã biên dịch (bài toán
undecidable; Midori né nó bằng cách không bao giờ phân phối native). Vậy "installer +
verifier" cho Cellos nghĩa là:

1. **Với cell native (first-party)**: verify tại **build-time**, không phải install-time —
   *trust-by-construction*. Chữ ký chỉ được cấp khi pipeline build đã kiểm:
   - `#![forbid(unsafe_code)]` có mặt ở crate + toàn bộ dependency tree (trừ allowlist
     ngoại lệ có hồ sơ: driver, ostd) — dùng script kiểm attribute + `cargo geiger`/lint gate;
   - toolchain đúng pin `rust-toolchain.toml` (F5), build reproducible;
   - allowlist ngoại lệ nằm trong file ký được review (mỗi entry kèm lý do + SAFETY audit).
   Khi đó chữ ký nâng cấp ngữ nghĩa từ "của chúng tôi" thành **"được build bởi pipeline đã
   enforce F1"** — đúng nghĩa Midori's guarantee, đạt bằng đường rẻ hơn. Hiện trạng 25/71
   crate có forbid cho thấy đây là việc thật, không phải hình thức.

2. **Với cell third-party / không tin được**: ~~WASM~~ — **đã loại** (quyết định user
   2026-07-30: tốc độ interpreter quá chậm + giới hạn; README/docs còn quảng cáo WASM tier-2,
   cần ADR ghi lại việc bỏ + dọn `cells/drivers/wasm` khỏi tài liệu). Thay bằng
   **tier 2 "domain cell"** — xem mục "Giới hạn cứng + mô hình 3 tier" dưới đây.

### Giới hạn cứng: cellos-sign trên máy dev không thể chống giả mạo bằng software

Đặt câu hỏi "giao cellos-sign cho dev ngoài chạy mà họ không làm giả được?" — trả lời:
**không tồn tại phương án software thuần**. Tool chạy trên máy do đối thủ kiểm soát luôn
giả được (patch tool, patch rustc, ký ELF khác). Ngoại lệ duy nhất là TEE:

- **Confidential build** (SEV-SNP/TDX/ARM CCA): cellos-sign + toolchain pin đóng thành
  image chạy trong confidential VM; source dev vào enclave mã hoá, build + kiểm + ký bên
  trong; key-server Cellos chỉ cấp khoá cho enclave attest đúng image. **Chính Cellos cũng
  không đọc được source** → giải đúng nỗi sợ lộ IP. Giá: vận hành key-server + attestation
  → G2+, là đường *tự nguyện* lên tier 1, không phải điều kiện gia nhập.

### Mô hình 3 tier (sau khi bỏ WASM)

| Tier | Ai | Isolation | Tốc độ | IPC |
|---|---|---|---|---|
| 1 — SAS cell | First-party, qua cellos-sign (khoá platform trong CI/KMS) | LBI (rustc + F1 enforced) | Native, zero-cost boundary | Zero-copy grant |
| 2 — Domain cell | Dev ngoài, KHÔNG ký | Per-domain page table (gap 1C): cùng VA layout, không map trang cell khác; MMU chặn mọi `unsafe` | Native trong domain; đổi satp+ASID ở ranh giới | Copy qua kernel; grant map tường minh |
| 3 — Silo VM | Legacy stack nguyên khối (Linux) | Stage-2 paging (h-ext đã có) | Native trong guest | virtio/proxy |

Động cơ đúng: ký = *mua hiệu năng* (zero-copy, boundary rẻ), không phải giấy phép tồn tại.
Dev ngoài chạy tier 2 với đúng ABI cell thường — không cần lộ source, không cần tin ai,
không cần VM. Cái giá họ trả là chi phí ranh giới MMU, và đó là giá công bằng cho việc
không chứng minh an toàn.

### Khuyến nghị

- **Bổ sung plan này**: phase mới "F1 enforcement + signing semantics" — (a) script
  `check-forbid-unsafe` chạy trong CI và trong `lib-sign-cells.sh` *trước khi ký*; (b) đưa
  71 crate về tuân thủ (thêm forbid, hoặc vào allowlist ngoại lệ kèm lý do — dự kiến:
  drivers + ostd vào allowlist, shell/tools/apps phải sạch); (c) từ chối ký crate ngoài
  allowlist mà có `unsafe`. Không ABI, không Law 1, toàn tooling.
- **ADR "tiered trust"**: theo bảng 3 tier ở trên (SAS cell / domain cell / silo VM). Ghi
  kèm: quyết định bỏ WASM + dọn tài liệu; confidential build là đường lên tier 1 ở G2+.
  Điểm bất biến: **không có tier "native không kiểm chạy trong SAS"** — native không ký
  thì chạy trong MMU domain, không bao giờ chạy chung address-space view với tier 1.
- **Ferrocene**: giữ nguyên lịch spec 16 (trước G2 production trên ARM64). Không fork.

---

## Gap 3 — IPC blocking-send + latency chưa đo

### Bản chất

Sau phase 07, `sys_recv` hết busy-poll nhưng `sys_send`/`Reply` vẫn chặn thread gửi
(phase 07 ghi rõ là follow-up ngoài scope). Số hiệu năng: target <50 µs round-trip,
spec 03 hứa 2–3 cycle qua vtable, thực tế ước 200–500 µs QEMU — **chưa từng đo**
(`docs/performance-report.md`: UNMEASURED), bench có sẵn ở `cells/tests/bench` nhưng
không chạy trong CI.

### Phương án

- **A. Đo trước, tối ưu sau** *(bắt buộc làm đầu)*: kích hoạt bench trong CI 3 arch, thêm
  một lần đo trên board thật (VF2/RK3588 — QEMU TCG không nói gì về latency thật). Không có
  baseline thì mọi claim "không hồi quy" của phase 07 là vô nghĩa thao tác.
- **B. Async send/reply trên CQ**: mở rộng tự nhiên của phase 07 (CQ đã có, waiter
  registration đã có); Law 1 vì thêm syscall submit-send. Làm sau khi 07 ổn định 1–2 tuần.
- **C. Direct-call vtable IPC** (2–3 cycle): thay đổi mô hình sâu — bỏ ranh giới syscall cho
  service cùng ring. Phase 07 đã ghi "cần ADR riêng, đụng toàn bộ spec 17". Chỉ cân nhắc khi
  A cho thấy IPC là bottleneck thật; tham chiếu Singularity exchange-heap ~1.200 cycle.

### Khuyến nghị

**Plan sau**, thứ tự A → B → (ADR cho C, chưa chắc làm). Không nhét vào plan này — phase 07
đã là phase lớn nhất và red-team đã cảnh báo scope creep.

---

## Gap 4 — Semantics BEAM: mailbox, selective receive, supervision tree

### Bản chất

CQ của phase 07 là completion queue cho *syscall*, không phải mailbox *message*. Rendezvous
`Recv` 1-buffer 4 KiB được giữ có chủ đích (req 6a). Thiếu so với BEAM: hàng đợi message
per-cell có backpressure, selective receive theo pattern (hiện chỉ lọc theo 1 sender-tid),
supervision phân tầng (restart nằm phẳng trong init, `NSVC=9`).

### Phương án

- **A. Mailbox là thư viện ostd trên CQ** *(khuyến nghị)*: sau 07, ostd dựng
  `Mailbox<T>` — bounded queue trong cell + waker từ CQ; selective receive theo pattern làm
  **trong cell** (rẻ, không cần kernel biết type). Kernel chỉ cần một việc: tổng quát hoá
  `pending_msgs` (đã có cho hotswap/input, depth 64/512) thành hàng đợi per-cell có
  backpressure — Law 1 gated, một syscall semantics change duy nhất.
- **B. Mailbox trong kernel**: kernel giữ queue + match pattern — sai boundary law (spec 15),
  kernel không nên biết type của message. Loại.
- **C. Supervision tree**: đã có plan riêng `.agents/260712-0800-supervisory-cell-migration`
  (supervisor cell tách khỏi init). Cây phân tầng + intensity per-subtree thuộc plan đó;
  việc của plan sau chỉ là nối `NotifyOnExit` chaining (supervisor giám sát supervisor).

### Khuyến nghị

**Plan sau**, phụ thuộc 07. Deliverable: `ostd::mailbox` + generalized pending queue (Law 1)
+ demo 1 cell chạy N actor-future với mailbox riêng — đây cũng chính là câu trả lời gap 5.

---

## Gap 5 — Scale: trần 64 cell vs "hàng triệu process" của BEAM

### Bản chất

`MAX_CELLS = 64` (`kernel/src/memory/cell_quota.rs:15`), `MAX_THREADS_PER_CELL = 32`,
stack 512 KiB/cell (2×256 KiB, phase 08 sẽ giảm theo bảng), quota 4 MiB. Kể cả sau 08,
"một triệu cell" là bất khả thi và **không nên** là mục tiêu: cell của Cellos tương đương
*process* của Midori, không phải *process* của BEAM.

### Phương án

- **A. Reframe hai tầng** *(khuyến nghị — đây là chính mô hình Midori)*: tầng isolation =
  cell (hàng chục, mỗi cái có quota/cap/manifest); tầng concurrency = async task trong cell
  (hàng nghìn future trên 1 thread sau phase 07 — Midori gọi là *activities*). "Nhẹ như
  BEAM" định nghĩa lại thành mục tiêu đo được: ví dụ **10.000 actor-future đồng thời trên
  64 cell, RAM < X MB** — thay vì đuổi theo con số cell vô nghĩa.
- **B. Nâng trần cơ học**: sau khi 08 đo watermark, nâng `MAX_CELLS` 64 → 256 (mảng static
  → sized const, đo RAM). Rẻ, làm cùng đợt với 08 hoặc plan sau.
- **C. Cell paging/swap để chứa nghìn cell**: quá sớm, không có use case ở G1/G2. Loại.

### Khuyến nghị

ADR ngắn chốt reframe A (định nghĩa lại tiêu chí "nhẹ" thành số đo actor-density) — viết
được ngay không cần code. B đi cùng plan sau, sau số đo của 08.

---

## Gap 6 — Escape hatch `NoEntry` dev-permissive

### Bản chất

Sau phase 03, POLICY.BIN đã bake nhưng path **thiếu entry** (typo, bake sót) rơi vào nhánh
`NoEntry` → dev-permissive → cell giữ nguyên cap `with_path_caps` mint, kể cả 3 cap P-TRUST
(DMA-anywhere). Plan gốc defer vì "đổi fleet posture" và vì lúc đó chưa đủ nền: cần
enumeration (03 đã giao) + per-path ceiling của init (04 sẽ giao).

### Phương án

- **A. Fail-closed cho path có `with_path_caps` khác rỗng** *(đúng như finding deferred đã
  chỉ)*: `NoEntry` + path nằm trong bảng `with_path_caps` ⇒ deny 3 cap P-TRUST (hoặc
  `CapSet::EMPTY` tuỳ mức), log audit. Path không có trong bảng → giữ dev-permissive (không
  phá dev loop).
- **B. Fail-closed toàn bộ khi build có flag `policy-required`**: mạnh hơn, nhưng đã tồn tại
  flag này và nó tắt mặc định — A là bước giữa đúng.

### Khuyến nghị

**Bổ sung plan này** thành phase 09, xếp sau 04 (cần ceiling per-path của 04 để tránh lặp
lại C1). Nhỏ: một nhánh trong `loader.rs`/`cap.rs` + test bake-sót-entry + chạy 3 peripheral
demo xác nhận không vỡ dev build. Sau phase này plan mới được claim "bít escape hatch" —
đúng ràng buộc mà mục Deferred đã đặt.

---

## Đề xuất cập nhật plan midori-lessons

Thêm 3 phase không ABI-gate (làm song song được với 02/04):

- **Phase 09 — NoEntry fail-closed** (gap 6): sau 04. Nhỏ.
- **Phase 10 — W^X post-reloc** (gap 1A): độc lập. Trung bình. `loader/elf.rs` + paging + TLB.
- **Phase 11 — F1 enforcement trong sign pipeline** (gap 2): độc lập, toàn tooling/CI. Trung bình.

Hai ADR viết ngay (không chờ code): **tiered-trust** (gap 2) và **layer-2 hardware
security + reframe actor-density** (gap 1B/C + gap 5A).

Plan sau (sau khi 07 land): đo IPC baseline → async send/reply (gap 3) → `ostd::mailbox` +
generalized pending queue (gap 4) → nâng `MAX_CELLS` theo số đo 08 (gap 5B).
