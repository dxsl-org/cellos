# Phase 10 — W^X post-reloc: hạ quyền trang cell về đúng p_flags của ELF

## Context Links

- Plan: [plan.md](plan.md) · Độc lập với 02/04/06/07/08 (chỉ đụng loader + paging)
- Spec: `docs/specs/19-hardware-isolation-layers.md` §2 Layer A (ADR đã accept),
  `docs/specs/02-memory.md`
- Nguồn: gap-analysis 2026-07-30 (gap 1) — [reports/gap-analysis-midori-lessons-260730-0849.md](../reports/gap-analysis-midori-lessons-260730-0849.md)

## Overview

- **Ưu tiên**: P2 · **Trạng thái**: Completed — runtime verified 2026-07-31; cross-hart
  TLB shootdown remains a known SMP limitation · **ABI gate**: Không
- **Mô tả**: Loader hiện map **mọi** trang cell với `WRITE` để áp reloc PIE, và không bao
  giờ hạ xuống — chú thích tại chỗ thừa nhận điều đó và hứa W^X cho G2
  ([elf.rs:~103-110](../../kernel/src/loader/elf.rs)). Hệ quả: bất kỳ cell nào (một khối
  `unsafe` là đủ) sửa được `.text`/`.rodata` của **mọi** cell khác — code injection xuyên
  cell không fault, không log. Phase này áp lại đúng `p_flags` của ELF **sau khi** reloc
  xong: không cần hardware mới, chạy trên cả 3 arch.

## Key Insights

- Chỗ duy nhất cần WRITE ngoài p_flags là giai đoạn áp `.rela.dyn` — kết thúc trước khi
  cell chạy lệnh đầu tiên. Sau thời điểm đó WRITE trên `.text`/`.rodata` là quyền thừa
  thuần tuý.
- Trang chia sẻ giữa hai PT_LOAD kề nhau đã có cơ chế merge flags (`already_ours`,
  [elf.rs:~130-155](../../kernel/src/loader/elf.rs)) — flags cuối của trang biên = OR của
  hai segment. W^X phải áp **sau** merge, trên flags cuối, không phải per-segment.
- Kernel về sau còn ghi vào trang cell (vd. loader ghi arg page, hotswap snapshot) —
  kernel đi qua HHDM alias (S-mode mapping riêng), không phụ thuộc bit W của mapping USER.
  Cần audit nhanh các đường kernel-ghi-vào-cell để xác nhận không đường nào dùng chính
  mapping USER.
- RELRO: PIE chuẩn có `.data.rel.ro` — reloc ghi vào nó lúc load, sau đó p_flags của
  GNU_RELRO là R. Vì ta hạ quyền *sau* reloc nên RELRO tự nhiên thành R mà không cần xử lý
  riêng — một quyền lợi miễn phí đúng nghĩa.
- Cần primitive mới trong paging: đổi flags của mapping đã tồn tại + `sfence.vma`/`tlbi`
  theo VA (3 arch có 3 đường invalidate khác nhau — kiểm tra HAL).

## Requirements

**Functional**

1. Paging: `protect_page(va, new_flags)` (hoặc `protect_range`) — đổi PTE flags của mapping
   sẵn có, kèm TLB invalidate đúng arch. Từ chối trang không map.
2. Loader: sau khi reloc + merge flags xong, mọi trang cell hạ về `p_flags` ELF (OR theo
   trang biên): segment không W ⇒ bỏ `Flags::WRITE`; không segment nào X+W (từ chối ELF
   khai W+X trên cùng segment — fail spawn, log rõ).
3. Thread spawn / hotswap restore / heap-snapshot: các đường map lại trang cell phải giữ
   nguyên flags đã hạ, không hồi sinh WRITE.
4. Kiểm chứng runtime (test-hooks): một cell thử ghi vào `.text` của chính nó ⇒ fault, bị
   terminate đúng đường fault-report hiện có (không panic kernel — đúng bài học PR #15).

**Non-functional**

5. Không đo được hồi quy thời gian spawn > 5% (chỉ thêm một lượt duyệt `mapped` + N lần
   ghi PTE + invalidate).
6. Không ABI change; cell không cần rebuild.

## Related Code Files

| File | Hành động |
|------|-----------|
| `kernel/src/memory/paging.rs` | Modify — `protect_page`/`protect_range` + invalidate per-arch |
| `hal/arch/*` | Modify nếu invalidate theo VA chưa expose đủ 3 arch |
| `kernel/src/loader/elf.rs` | Modify — pass hạ quyền sau reloc; từ chối W+X; cập nhật chú thích "G2 item" thành mô tả hiện thực |
| `kernel/src/loader.rs` | Read/Modify — điểm gọi sau reloc; đường arg page |
| `kernel/src/task.rs` / hotswap | Audit — mọi đường re-map trang cell giữ flags |
| `tests/` | Add — test ghi vào `.text` ⇒ fault (test-hooks) |

## Implementation Steps

1. Audit các đường kernel-ghi-vào-trang-cell (grep quanh loader, hotswap, snapshot) — danh
   sách đường nào dùng USER mapping trực tiếp. Rẻ, quyết định bước 3 cần gì.
2. `protect_page` + invalidate trên riscv64; nối HAL cho aarch64/x86_64.
3. Pass hạ quyền trong `spawn_from_mem` sau reloc; từ chối W+X.
4. Sửa các đường re-map (nếu bước 1 tìm thấy) sang HHDM alias.
5. Test fault-on-text-write (test-hooks); boot + suite 3 arch; đo spawn time trước/sau.
6. Cập nhật chú thích elf.rs + spec 19 §2 Layer A từ "planned" sang mô tả hiện thực.

## Todo List

- [x] Audit đường kernel-ghi-vào-cell (bước 1) — không đường nào cần đổi; xem Deviation Log D1
- [x] `protect_page`/`protect_range` + TLB invalidate 3 arch
- [x] Pass hạ quyền sau reloc; reject W+X segment
- [x] Re-map path giữ flags (thread/hotswap/snapshot) — không đường nào hồi sinh WRITE
- [x] Test: ghi `.text` ⇒ fault + terminate sạch — PASS via `tests/integration/tests/wx-text-write.rs` 2/2
- [x] Boot + suite 3 arch; spawn time không hồi quy >5% — PASS via `qemu-build-unblock-260731` / boot 54/54
- [x] Cập nhật chú thích + spec 19

## Success Criteria

- Grep không còn chú thích "All cell pages are mapped WRITE" ở elf.rs — thay bằng mô tả
  hạ quyền.
- Trong suite: `.text` của mọi cell là USER+R+X, `.rodata` USER+R (kiểm bằng test-hook dump
  PTE), heap/stack/data giữ USER+RW.
- Cell ghi vào `.text` ⇒ fault → terminate cell, hệ tiếp tục chạy, 3 arch.
- Suite + 3 peripheral demo + doom xanh trên 3 arch.

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| Một đường kernel hợp lệ vẫn ghi qua USER mapping (hotswap/snapshot) → fault kernel | Trung bình | Boot/hotswap vỡ | Bước 1 audit trước; các đường đó chuyển HHDM alias |
| Cell nào đó tự sửa code (trampoline, JIT nội bộ) | Thấp (workspace không có JIT) | Cell đó vỡ | Suite 3 arch là lưới; nếu lộ ra, cell đó khai segment W+X tường minh → bị reject → sửa cell, không nới kernel |
| Invalidate thiếu trên SMP (hart khác còn TLB entry W) | Trung bình | Cửa sổ ghi được sau hạ quyền | Dùng IPI/sfence broadcast sẵn có của SMP phase 32; test trên cấu hình 2-hart |
| p_flags của linker script cell sai (thiếu W trên trang cần ghi) | Thấp | Cell fault ngay lần chạy đầu | Lỗi hiện hình, dễ sửa linker script — ngược với hiện trạng lỗi vô hình |

## Security Considerations

- Đây là Layer A của Spec 19: sau phase này, **code integrity xuyên cell được phần cứng
  bảo đảm ở mọi tier** — kể cả khi F1 bị vi phạm. Data (heap/stack) vẫn hở xuyên cell trong
  SAS; điều đó thuộc Layer B (Tier 2, plan sau), không claim quá.
- Từ chối W+X là chốt chống cell "tự khai để được ghi code": muốn W+X phải sửa manifest/
  linker công khai, reviewer nhìn thấy.

## Deviation Log

**D1 — Audit (bước 1): không đường kernel nào cần chuyển sang HHDM alias.** Kết quả
grep toàn kernel cho các đường ghi vào trang cell:

| Đường | Cách ghi | Kết luận |
|-------|----------|----------|
| `loader/elf.rs:~200-250` nạp segment | `frame::phys_to_virt(frame)` | HHDM/identity alias — an toàn |
| `loader/reloc.rs:112` (aarch64) | `phys_to_virt(virt_to_phys(va))` | alias — an toàn |
| `loader/reloc.rs:117` (riscv64/x86_64) | **USER VA trực tiếp** | chạy **trước** khi hạ quyền — an toàn theo thứ tự |
| `snapshot.rs:276-291` warm restore | `pa as *mut u8` (identity) | alias — an toàn; khôi phục cả PTE đã hạ quyền |
| `task.rs:1138`, `task.rs:1202` IPC | USER VA vào buffer nhận | nằm ở heap/stack/`.data` — giữ `PF_W`, không bị hạ |
| `task/syscall.rs:404/1089/1177/3118/3276` | USER VA vào buffer cell cấp | như trên |
| `task/syscall.rs:153` grant zeroing | `paddr as *mut u8` | trang grant riêng, không thuộc segment |
| `task/stack.rs:97` stack cell | map VA==PA, luôn có WRITE | không thuộc segment |
| `task/user_hello.rs` | test-hooks, ghi frame trước khi map | không phải cell ELF |

Không đường nào phụ thuộc bit W của mapping USER trên trang **không-W**. Không phải sửa gì
ở bước 4. Thread spawn chỉ cấp kernel stack, không map lại trang segment; hotswap đi qua
`spawn_from_path` → `spawn_from_mem` nên nhận flags đã hạ như mọi spawn khác.

**D2 — AArch64 chưa từng mã hoá bit read-only (phát hiện lúc implement).** `hal/arch/arm/
src/aarch64/paging.rs` chỉ set `AP[1]` (bit 6, cho phép EL0); `AP[2]` (bit 7, read-only)
không bao giờ được set, nên `PageFlags::WRITE` bị **bỏ qua hoàn toàn** trên arch này. Nếu
không sửa, W^X sẽ là no-op im lặng trên aarch64 trong khi riscv64/x86_64 thực thi thật.
Đã thêm `PTE_AP_RO` + set khi thiếu `WRITE`. Rủi ro hồi quy thấp: chỉ đúng một call site
hiện có bỏ `WRITE` (`task/user_hello.rs:79`, test-hooks, vốn *muốn* read-only).

**D3 — x86_64 `#PF` handler panic thay vì kill cell.** `memory/paging.rs::vi_handle_page_
fault` panic ở nhánh "no VMA covers this address" cho mọi user fault. Sau W^X, cell ghi
`.text` trên x86_64 sẽ **panic kernel**, vi phạm requirement 4 (và bài học PR #15).
riscv64 (`rv64/trap.rs:139-148`) và aarch64 (`aarch64/trap.rs:118-131`) đã route đúng về
`vi_terminate_on_fault`. Đã thêm `fault_kill_cell` để x86_64 đồng bộ, **và** chặn
demand-paging khi `error_code & 1 != 0` (protection violation) — nếu không, handler sẽ
re-map trang bằng flags của VMA và hồi sinh WRITE đúng thứ mà pass này vừa gỡ.

**D4 — Vị trí file: `protect_page` đặt ở `memory/page_protect.rs`, không phải `paging.rs`.**
Plan ghi `paging.rs`; file đó đã 940+ dòng. Đặt ở module riêng rồi `pub use` lại trong
`paging.rs` nên tên gọi `paging::protect_page` mà plan giả định vẫn đúng nguyên.

**D5 — Sửa kèm rò rỉ frame trong `load_segments`.** Chỉ nhánh VA-collision unwind các
trang đã map; ba nhánh lỗi header khác `return Err` thẳng, để lại frame đã cấp + mapping
sống (đầu độc luôn overwrite-guard của lần spawn sau). Đã trích `ElfLoader::unwind` và
gọi ở mọi nhánh — cùng 4 dòng vốn đã có, hoá ra dùng chung.

**D6 — Trang biên W+X sau merge: cảnh báo, không từ chối.** Plan chỉ yêu cầu từ chối
segment **tự khai** W+X (đã làm, `wx::reject_wx_segment`). Một trang bị hai PT_LOAD R-X và
R-W dùng chung sẽ OR thành W+X dù không segment nào khai. Bỏ bit nào cũng làm vỡ cell, và
không boot được để kiểm chứng linker layout thật, nên chọn log `warn` nêu đúng số trang
thay vì fail spawn. Đây là lỗ hổng còn lại, đã ghi vào Spec 19 §2 Layer A.
**→ Đã bị D9 thay thế: nay là `Err`, không phải `warn`.**

**D7 — Test cell đặt ở `cells/tests/wx-test/`, ngoài danh sách File Ownership.** Ownership
chỉ liệt kê `tests/`. Requirement 4 cần một **cell U-mode** ghi vào `.text` của chính nó —
không thể viết bằng file dưới `tests/` (đó là host binary chạy QEMU). Theo tiền lệ
`cells/demos/cfi-test` (cell cố tình vi phạm CFI, harness grep log fault). Kéo theo sửa
`Cargo.toml` (members) + `gen_disk.ps1` (build/sign/table) — cả hai không đụng phase khác.

**D8 — Xung đột chéo phase (CẦN THEO DÕI).** Một agent khác đang sửa cùng working tree
(`libs/ostd/src/entry.rs`, `ostd::cell_main!`, `#![forbid(unsafe_code)]` cho cells,
`scripts/unsafe-allowlist.toml`, F1 admission gate trong CI). Không trùng file với phase
này. NHƯNG `cells/tests/wx-test` dùng `unsafe` (bắt buộc — đó là bài test) nên sẽ bị F1
admission gate chặn cho tới khi có entry `[[file]]` + `[[crate]]` trong
`scripts/unsafe-allowlist.toml`, y hệt `cfi-test` (dòng 251 và 505). **Không tự sửa file
đó** vì nó thuộc phase kia.

**D9 — Trang biên W+X sau merge: chuyển từ `warn` sang `Err` (thay thế D6).** Review
`wave1-review-260730.md` C1. Câu hỏi mà D6 không trả lời được vì không boot được — "linker
thật có sinh trang biên W+X không?" — đã được giải bằng phân tích tĩnh: parse program
header của 505 ELF dựng sẵn dưới `target/`, **không có** cái nào. Từ chối không tốn gì hôm
nay. Tách thành `wx::reject_wx_page` (hàm thuần, kiểm được bằng self-test) và cho `enforce`
chạy **một lượt validate toàn bộ trước** khi hạ quyền trang nào, để ELF bị từ chối không để
lại nửa số trang đã hạ. Lỗi trả về `PermissionDenied` (như `reject_wx_segment`, phân biệt
với `InvalidInput` vốn dành cho bug loader).

**D10 — `CellId` được dẫn xuất trong `spawn_cell_task`, không phải vá sau spawn.** Review
C2: `Syscall::SpawnFromMem` truyền `CellId(0)` rồi không gán lại, nên cell `exec` được sẽ
panic cả kernel ở fault handler (cả ba arch đều đòi `cell_id != 0`). Thay vì chép đoạn vá
của `loader.rs` sang syscall (đường spawn thứ tư sẽ lại quên), đưa hẳn phép dẫn xuất vào
`task::spawn_cell_task`: `CellId(0)` = "hãy tự cấp id", gán `CellId(tid)` **trong cùng
critical section** đăng ký task nên không có cửa sổ nào task quan sát được với id
placeholder. Xoá đoạn vá trùng ở `loader.rs` và `main.rs` (cả hai gán đúng giá trị đó).
Hệ quả có chủ ý: cell spawn bằng `exec` nay bị tính quota 16 MiB như mọi cell khác, thay vì
dùng slot không giới hạn của kernel.

**D11 — Hạ quyền W^X trước khi đăng ký task, không thêm state mới (review C3).** Kiểm tra
theo yêu cầu: `apply_relocations` chỉ cần `load_base` + section bytes, không phụ thuộc
task; đường ghi duy nhất vào trang cell sau bước 6 cũ chính là nó. Nên chuyển **cả**
relocation lẫn `wx::enforce` lên trước `spawn_with_stacks` là đủ và ít xâm lấn hơn hẳn so
với thêm `TaskState` không-chạy-được. Đóng luôn miễn phí cửa sổ "cell chạy khi chưa
relocate". Đường lỗi: task chưa tồn tại nên không cần `exit_task` — `CellSegments` drop trả
frame + VA slot, đúng như `exit_task` vẫn làm. Đã sửa doc `wx.rs` để nêu đúng bảo đảm thật
(bước 4 của ordering contract) và nêu rõ điều **không** bảo đảm (entry TLB cũ của hart khác
từ cell trước ở cùng VA — `protect_page` chỉ invalidate local hart).

**D12 — Cửa sổ còn lại, KHÔNG thuộc phạm vi ba lỗi trên.** Giữa `spawn_cell_task` (đăng ký
+ `push_ready`) và bước 9 (dựng trap frame/context), task đã Ready nhưng `context.ra` vẫn
trỏ `task_entry_point` — vòng lặp kernel, không phải entry của cell. Hart khác steal trúng
cửa sổ này sẽ chạy nhầm entry. Cell id lúc đó đã đúng nên **không** sinh user fault với
`cell_id 0`; W^X cũng đã áp xong. Đây là lỗi có sẵn từ trước phase này (thứ tự bước 6/7 cũ
y hệt); sửa đúng cách là dựng context xong mới `push_ready`, cần đổi
`Scheduler::spawn_with_stacks` nên để phase riêng.

## Next Steps

- Deferred follow-up: TLB shootdown xuyên hart cho `protect_page` still belongs to the SMP phase 32 work.
- Deferred follow-up: `wx-test` can stay on the unsafe allowlist path when the F1 admission gate phase lands.
- Deferred follow-up: Layer B (per-domain page table — Tier 2, Spec 18/19) remains a later plan and can reuse `protect_range`.
