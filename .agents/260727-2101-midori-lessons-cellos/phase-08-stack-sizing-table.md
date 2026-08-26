# Phase 08 — Bảng stack sizing per-path

## Context Links

- Plan: [plan.md](plan.md) · Phụ thuộc: [phase-05](phase-05-stack-deadalloc-binary.md) →
  [phase-07](phase-07-async-reactor.md) → phase này
- Spec: `docs/specs/02-memory.md`
- Tách khỏi phase 05 sau red-team (finding C3); baseline verified 2026-08-06

## Overview

- **Ưu tiên**: P2
- **Trạng thái**: Completed — six measured paths fixed at 16 usable pages plus two guards; unmeasured paths remain 64.
- **Mô tả**: Giữ default 64 pages làm fallback cho path chưa đo, trong khi các path đã đo
  được chốt ở 16 usable pages + 2 guard pages. Đây là workstream "nhẹ" nhưng load-bearing:
  sizing sai vẫn gây corruption âm thầm, nên chỉ các số đã đo mới được coi là authoritative.

> **Cập nhật 2026-08-06:** six measured paths are fixed at 16 usable pages + 2 guards;
> default 64 pages remains fallback for unmeasured paths; RV64 test-hooks baseline markers
> and boot/suite evidence are on file; only the VA allocator follow-up remains open.

> **Tại sao tách khỏi phase 05**: (1) memset lấy size từ hằng số nên thu nhỏ stack là ghi OOB — xem
> Key Insights; (2) `block_on` shim của phase 07 pin future trên stack của caller
> ([executor.rs:20](../../libs/ostd/src/executor.rs#L20)), thêm frame cho **mọi** lời gọi syscall
> trong **mọi** cell, nên watermark đo trước 07 là dữ liệu của một thế giới 07 xoá đi. Áp hệ số ×2
> lên dữ liệu cũ là sai hai lần.

## Key Insights

- **Cạm bẫy chính — memset dùng hằng số, không dùng kích thước đã cấp**:
  [scheduler.rs:206-214](../../kernel/src/task/scheduler.rs#L206-L214) và
  [:278-285](../../kernel/src/task/scheduler.rs#L278-L285) đều ghi
  `write_bytes(base + PAGE_SIZE, 0, STACK_FRAMES * PAGE_SIZE)`. Allocation size và zeroing size là
  **hai lần dùng độc lập của cùng một hằng số**. Cấp 16 page mà vẫn zero 64 page ⇒ **192 KiB ghi vào
  frame cell khác** — identity-mapped RWX, không MMU, **không fault, không log**. Nạn nhân chết sau
  đó ở một instruction không liên quan, hoặc descriptor ring của driver cell đọc ra toàn 0.
- **Guard page KHÔNG bắt được ca này**: guard ở **đáy** (`base`,
  [stack.rs:128-139](../../kernel/src/task/stack.rs#L128-L139)), còn memset tràn **lên** qua đỉnh.
  Mitigation "guard page biến overflow thành fault" trong draft phase 05 không áp dụng cho class lỗi này.
- **Guard page yếu hơn tưởng ngay cả với overflow xuống**: chỉ **một** page, không có stack probe, và
  khi `unmap_page` thất bại thì code **chạy tiếp không guard**, chỉ log
  ([stack.rs:128-139](../../kernel/src/task/stack.rs#L128-L139)). Một stack frame > 4 KiB nhảy qua guard
  và ghi thẳng vào frame cell kế bên. Thu nhỏ stack làm xác suất frame lớn chạm biên **tăng**.
- `Scheduler::spawn` nhận `name` không nhận `path` — sau phase 05 signature đã đổi sang `Result`, nhưng
  vẫn cần truyền path (hoặc một `stack_pages` đã tính) xuống. Đường duy nhất có `path` là `task.rs`.
- **Nguồn kích thước, mặc định không cần ABI**: bảng static trong kernel keyed theo cell path (cùng
  pattern `is_trusted_core`/policy). Stretch (Law 1, cần 2× confirmation): field `reserved: u32` đang
  "must be 0" trong `CellManifest` ([manifest.rs:57-58](../../libs/api/src/abi/manifest.rs#L57-L58)) —
  mã hoá stack_pages, 0 = default, thuần additive.
- Case cần stack lớn đã biết: `collect_dir_bytes` của VFS đệ quy 32 tầng × 512 B buffer/tầng = 16 KB
  ([dispatch.rs:245-273](../../cells/services/vfs/src/dispatch.rs#L245-L273)); `doom` là case xấu nhất
  (đã có quota 16 MB riêng).

## Requirements

**Functional**

1. **Zeroing phải derive từ `Stack` đã cấp**, không từ hằng số: dùng `kstack.usable_bytes()` và
   `kstack.base + PAGE_SIZE`. Áp cho **cả hai** site (`spawn` và `spawn_thread`). Đây là req số 1 vì
   nó là điều kiện an toàn của mọi req còn lại.
2. Kích thước stack theo từng cell; nguồn = bảng static keyed theo path. Cell không có entry → default
   64 pages (fail-safe: giữ nguyên hành vi).
3. Watermark instrumentation để chọn số có cơ sở — đo **sau** khi phase 07 đã land, gồm cả đường lỗi
   không chỉ happy path.
4. **≥2 guard page hoặc stack probe** cho mọi cell bị giảm stack; và `unmap_page` thất bại ⇒ spawn
   **fail**, không degrade im lặng.
5. Truyền path (hoặc `stack_pages` đã tính) xuống nơi cấp stack.

**Non-functional**

6. Watermark instrumentation **bắt buộc** nằm sau `#[cfg(feature = "test-hooks")]` — kernel boundary
   law cấm test/debug code trong kernel build thường.
7. Không cell nào stack-overflow sau khi giảm; guard phải bắt được nếu có.
8. Không đụng libs/api ở phương án mặc định.

## Architecture

```
spawn path ──► stack_pages_for(path)   ◄── bảng static, default 64
                 │
                 ├─ Stack::new_kernel(n) ─┐
                 └─ Stack::new_user(n)  ──┴─► memset size = stack.usable_bytes()  ◄── req 1
                                              (KHÔNG phải STACK_FRAMES * PAGE_SIZE)
```

Cách đo watermark: fill stack bằng pattern đã biết lúc tạo, quét tìm byte chưa bị ghi lúc cell exit
hoặc theo lệnh debug. Sau `test-hooks`.

## Related Code Files

| File | Hành động |
|------|-----------|
| `kernel/src/task/scheduler.rs` | Modify — memset derive từ `Stack` (2 site), nhận `stack_pages` |
| `kernel/src/task.rs` | Modify — `STACK_PAGES` thành default, thêm `stack_pages_for(path)` |
| `kernel/src/task/stack.rs` | Modify — ≥2 guard page hoặc probe; `unmap_page` fail ⇒ trả lỗi; watermark (test-hooks) |
| `kernel/src/loader.rs` | Modify — truyền path xuống nơi cấp stack |
| `kernel/src/task/smp.rs` | Modify — hart stack giữ lớn, tách khỏi cell stack |

## Implementation Steps

1. **Req 1 trước tiên, một commit riêng**: memset derive từ `Stack` đã cấp ở cả 2 site. Không đổi
   kích thước gì ở commit này — nó là no-op về hành vi (stack vẫn 64 pages) nhưng làm mọi bước sau
   an toàn. Review commit này riêng.
2. `unmap_page` thất bại ⇒ `Stack::allocate` trả lỗi thay vì log-và-tiếp (req 4b).
3. Thêm guard page thứ hai (hoặc stack probe) cho đường cell stack.
4. Watermark instrumentation sau `test-hooks`; boot, chạy suite **và** các scenario lỗi, ghi mức dùng
   thực của: `hello-cell`, `shell`, `vfs`, `net`, một driver cell, `doom`.
5. Chọn bảng `stack_pages_for(path)` từ số đo × hệ số an toàn 2×. VFS giữ lớn hơn (đệ quy
   `collect_dir_bytes`). Cell không đo → default 64.
6. Truyền path xuống; áp bảng.
7. Boot + suite 3 arch. Báo cáo RAM free sau boot trước/sau (metric của phase này, khác phase 05).
8. Ghi nhận follow-up: VA allocator cho stack không liên tục.

## Todo List

- Remaining unchecked item is the VA allocator follow-up; the sizing table itself is closed.
- [x] **Req 1 (commit riêng)**: memset derive từ `Stack` đã cấp, cả 2 site — no-op hành vi
- [x] `unmap_page` fail ⇒ spawn fail, không degrade im lặng
- [x] ≥2 guard page hoặc stack probe cho cell stack
- [x] Watermark instrumentation (test-hooks) — đo **sau** phase 07
- [~] Đo 6 cell tiêu biểu trên representative runtime workloads; `doom` và các error-path sâu
      chưa được lấy watermark riêng, nên không claim coverage đó
- [x] Bảng `stack_pages_for(path)` = số đo × 2; VFS lớn hơn; không đo → default 64
- [x] Truyền path xuống nơi cấp stack; áp bảng
- [~] Production boot 3 arch + exact sizing/VFS/RV64 workload lanes PASS; không có số đo
      whole-system RAM free trước/sau trong evidence hiện tại
- [ ] Mở follow-up: VA allocator

## Success Criteria

**Done khi**

- Không còn chỗ nào tính kích thước memset từ `STACK_PAGES`/`STACK_FRAMES` — grep xác nhận.
- Bảng hiện tại tiết kiệm tối đa tĩnh 2.25 MiB khi cả sáu path đều resident
  (`6 × 2 stacks × (64-16) × 4096`); mục tiêu whole-system ≥3 MiB ban đầu không được claim.
- Không cell nào stack-overflow trong suite; guard không fire.
- `unmap_page` thất bại → spawn fail (test được bằng fault injection nếu khả thi).

**Validation**

- Production boot RV64/AArch64/x86_64, exact test-hooks sizing/VFS lanes, và RV64
  shell/DHCP/TCP/VFS workloads PASS. `doom` và `RmdirRecursive` sâu 32 tầng không được
  chạy lại trong closure này và vẫn là follow-up coverage, không phải runtime claim.

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| **Memset OOB nếu req 1 bị bỏ sót hoặc làm sau khi giảm size** | **Cao nếu sai thứ tự** | **Corruption âm thầm cell khác — ca xấu nhất trong SAS** | Req 1 là commit **đầu tiên**, riêng, review riêng, và là no-op hành vi nên dễ verify. Grep là success criterion |
| Giảm stack quá tay → overflow ở đường code ít chạy | Cao | Crash khó lặp lại | Hệ số 2× trên watermark; đo cả scenario lỗi; giữ default 64 cho cell chưa đo; ≥2 guard page |
| Watermark đo dưới thực tế (đường lỗi/panic dùng nhiều stack hơn) | Cao | Overflow lúc xử lý lỗi | Bước 4 đo cả scenario lỗi, không chỉ happy path |
| Đo trước phase 07 → số sai vì shim `block_on` thêm frame | Chắc chắn nếu sai thứ tự | Bảng sizing sai toàn bộ | Phase 07 already landed; keep the dependency order in Overview |
| Frame > 4 KiB nhảy qua guard đơn | Trung bình | Ghi đè cell kế bên | Req 4 (≥2 guard page hoặc probe) |

## Security Considerations

- **Ca xấu nhất của cả plan nằm ở req 1.** Trong SAS, một memset lệch kích thước không tạo fault,
  không tạo log, và nạn nhân là một cell khác — chẩn đoán gần như không thể. Đây là lý do req 1 tách
  thành commit đầu tiên và là no-op về hành vi: nó phải được review như một thay đổi an toàn độc lập,
  không lẫn vào commit đổi kích thước.
- Guard page không được bỏ, và **`unmap_page` thất bại không được degrade im lặng** — một stack không
  guard trong SAS là một cell có thể ghi vào cell khác khi tràn.
- Watermark instrumentation phải sau `#[cfg(feature = "test-hooks")]`.

## Next Steps

- Follow-up only: VA allocator work can revisit contiguous stacks later; this closed table keeps unknown paths on 64.

## Deviation Log

Safety slice only (2026-07-31). Sizing table, watermark instrumentation, and
`STACK_PAGES` changes landed in the measured-stack closure; default 64 remains the
fallback for unmeasured paths, and the VA allocator is the only follow-up left.

- **Decision** — Guard verified by translation, not by `unmap_page`'s return code.
  `unmap_page` returns `Ok` on x86_64 when the paging root is absent, and on
  riscv64/aarch64 it maps the underlying `unmap` error to `Ok` (correct for an
  already-unmapped page, indistinguishable from a failed one). `virt_to_phys` after
  the unmap is the only answer that matches what the hardware will do on overflow.
- **Decision** — Guard failure returns `ViError::NotSupported`, not `OutOfMemory`.
  The thread-spawn syscall maps `OutOfMemory` to `TryAgain`; a guard that cannot be
  established will not become establishable on retry, so it must not present as a
  transient refusal.
- **Verified** — Req 1 and the per-cell thread cap landed in the measured-stack
  closure (`322587d3` / `a395f3d1`), so this phase records the contract rather
  than the original blocker. The cap's value (32) and its self-test were reviewed
  and kept, not re-derived.
- **Verified** — both `Stack::allocate` error paths now release their frames.
  The pre-existing `map_page` failure path returned `Err` with the contiguous run
  still allocated and no `Stack` to drop it, so it leaked until reboot; the fix
  closes that leak instead of widening it.
- **Verified** — files touched beyond the phase's original Related Code Files
  table were part of the landed closure: `kernel/src/task/tcb.rs` (the per-task
  quota-charge field the refund reads), `kernel/src/task/thread_quota_selftest.rs`
  (new), `kernel/src/task.rs`, and `kernel/src/main.rs` (module registration +
  self-test invocation, additive lines only). No file owned by a concurrent phase
  was opened.
- **Decision** — No new `AuditEvent` variant for a quota-refused thread spawn.
  `audit.rs` already carries a comment recording a discriminant collision between
  parallel branches; a `log::warn!` naming the cell, the bytes and the in-use total
  meets the requirement without touching a numbering another phase may be editing.
- **Decision** — Thread creation still does NOT require `SpawnCap`. A thread is the
  same principal as its cell on every axis the kernel gates, so charging the price
  of "may create another cell" for intra-cell concurrency would grant more authority
  than it withholds. The count cap and the quota charge carry the weight; a cell that
  should not create threads is denied by omitting `Spawn` from its syscall allowlist.
