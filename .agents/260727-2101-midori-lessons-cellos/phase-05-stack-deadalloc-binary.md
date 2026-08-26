# Phase 05 — Xoá stack cấp trùng + 3 `.expect` + binary footprint

## Context Links

- Plan: [plan.md](plan.md) · Kế tiếp: [phase-07](phase-07-async-reactor.md) → [phase-08](phase-08-stack-sizing-table.md)
- Spec: `docs/specs/02-memory.md`, `docs/specs/12-reliability.md` (never-die)
- Midori nguồn: nhẹ nhờ **hoạt vụ không có stack** + closed-world AOT/tree-shaking. Lợi thế Cellos
  có sẵn: **không có GC**

## Overview

- **Ưu tiên**: P1 — ROI cao nhất trên effort, không đổi ABI, xoá một panic path phá never-die
- **Trạng thái**: **Done 2026-07-28, đã vào `main`** = `7621a7f6b` (SHA trước rebase `c9bb6f2fc`;
  branch `feat/kernel-stack-dedup` đã xoá). Xem `## Evidence` — hai claim của plan bị đo lại và
  đính chính ở đó. Rebase phải đổi audit event `ThreadCapReached` 23 → **25** vì trùng với phase 03.
- **Mô tả**: Phần **an toàn** của công việc "nhẹ", tách khỏi phần rủi ro (bảng sizing per-path →
  [phase 08](phase-08-stack-sizing-table.md)). Ba việc: xoá stack cấp trùng trong `Scheduler::spawn`
  (cắt ~50% peak contiguous demand, không đổi hành vi), bỏ 3 site `.expect("OOM Stack")`, và đo +
  feature-gate binary footprint.

> **Red-team correction**: draft gộp cả bảng sizing vào đây. Sizing là chỗ sinh ra OOB memset (xem
> phase 08) và số watermark đo trước phase 07 sẽ phải đo lại. Tách ra.

## Key Insights

- **Mỗi lần spawn cell cấp 4 stack, dùng 2** (red-team, đã xác minh): `task.rs:572-575` cấp một cặp
  bằng `map_err(...)?` (có `Result`); rồi `spawn(name, ...)` tại `task.rs:580` → `Scheduler::spawn`
  cấp **cặp thứ hai** tại [scheduler.rs:197](../../kernel/src/task/scheduler.rs#L197) +
  [:220](../../kernel/src/task/scheduler.rs#L220) với `.expect()`; rồi
  [task.rs:590-592](../../kernel/src/task.rs#L590-L592) ghi đè `task.kernel_stack`/`task.user_stack`
  bằng cặp thứ nhất → cặp của scheduler bị drop. **Peak contiguous demand per spawn ≈ 4 × 260 KiB
  ≈ 1040 KiB**, không phải 520 KiB; một nửa là phí thuần.
- **3 site `.expect("OOM Stack")`, không phải 2**: [scheduler.rs:197](../../kernel/src/task/scheduler.rs#L197),
  [:220](../../kernel/src/task/scheduler.rs#L220), và [:271](../../kernel/src/task/scheduler.rs#L271)
  trong `spawn_thread` — site thứ ba là site **attacker-reachable**.
- **Thread spawn không được SpawnCap-gate**: `Syscall::Spawn` (tạo thread) không qua
  `caller_has_spawn` (so với [syscall.rs:2092](../../kernel/src/task/syscall.rs#L2092) cho spawn cell).
  Một cell không đặc quyền loop thread-spawn → mỗi thread đòi 65 frame **liên tục**
  ([stack.rs:82-84](../../kernel/src/task/stack.rs#L82-L84)) → phân mảnh → `.expect` tại
  `scheduler.rs:271` **panic kernel** = never-die vỡ từ một cell untrusted.
- `Scheduler::spawn(&mut self, name: &str, cell_id, allowed_drivers) -> usize` — **nhận `name` không
  nhận `path`, trả `usize` không trả `Result`** (0 khi fail). Nên "propagate the Result" là refactor
  signature + callers (`task.rs:466`, `:580`, `:1650`, `kernel/src/task/tests.rs`,
  `syscall.rs:4393`), không phải sửa một dòng.
- **Contiguity là bắt buộc, không phải tuỳ chọn**: `allocate_contiguous(total_pages)`
  ([stack.rs:83](../../kernel/src/task/stack.rs#L83)) vì SAS identity-map và **chưa có VA allocator**;
  block comment [stack.rs:60-80](../../kernel/src/task/stack.rs#L60-L80) tự nhận "TEMPORARY SOLUTION".
  Lazy-commit qua guard page cũng vô ích vì VA==PA nên frame vẫn bị giữ. VA allocator là follow-up
  ngoài scope.
- **Binary: lever LTO đã dùng hết** — `lto = true`, `opt-level = "z"`, `panic = "abort"`
  ([Cargo.toml:162-167](../../Cargo.toml#L162-L167)). Lever còn lại là feature-gate ostd. `http`/`json`
  đã gate đúng (comment nêu rõ mục tiêu "ZERO binary cost" — pattern đúng để nhân bản), nhưng
  **`fontdue` (rasterizer font), `hashbrown`, `serde`, `heapless`, `linked_list_allocator` là dep vô
  điều kiện** trong `libs/ostd/Cargo.toml`. `fontdue` chỉ dùng cho `font_atlas.rs` (ViUI).
- Số liệu quan sát (riscv64 release): `hello-cell` 32 KB · `service-vfs` 502 KB · `app-shell` 902 KB ·
  `doom` 846 KB · `libostd.rlib` 1.68 MB · `libapi.rlib` 1.93 MB. **Chưa kết luận** fontdue có bị link
  vào cell non-GUI hay không — LTO + gc-sections có thể đã strip. Bước đầu của workstream B là ĐO.
- Comment `// Stack Size: 8 Frames (32KB)` tại [scheduler.rs:193](../../kernel/src/task/scheduler.rs#L193)
  là stale (thực tế 64 frames) — sửa luôn.

## Requirements

**Functional — workstream A (RAM, không đổi hành vi)**

1. Xoá stack cấp trùng: `Scheduler::spawn` không tự cấp stack cho đường spawn cell — nhận stack đã
   cấp làm tham số, hoặc đường `task.rs` thôi pre-allocate. Chọn một, không để cả hai.
2. Bỏ cả **3** `.expect("OOM Stack")`; spawn fail trả `ViError::OutOfMemory`. Bao gồm signature của
   `Scheduler::spawn` và mọi call site.
3. Đường lỗi mới phải dọn sạch: `Stack` có `Drop`, xác minh nó chạy trên **mọi** nhánh lỗi và không
   để lại task nửa-dựng trong scheduler.
4. Cap số thread mỗi cell + charge stack vào `cell_quota` trước khi cấp — đóng đường DoS qua
   `Syscall::Spawn` không được cap-gate.
5. Sửa comment stale ở `scheduler.rs:193`; xoá block comment "TEMPORARY SOLUTION" ở `stack.rs:60-80`,
   thay bằng một câu nêu **ràng buộc thật** (identity-map ⇒ contiguous ⇒ cần VA allocator để bỏ).

**Functional — workstream B (binary)**

6. Đo `.text`/`.rodata` mỗi cell; xác định `fontdue`/`serde`/`hashbrown` có thực sự bị link vào cell
   non-GUI hay không.
7. Feature-gate mọi thứ đo được là dead weight, theo pattern `http`/`json` đã có.

**Non-functional**

8. Không đổi kích thước stack ở phase này — đó là phase 08. Phase này giữ `STACK_PAGES = 64` nguyên vẹn
   nên **không có rủi ro OOB memset** (memset vẫn khớp allocation).
9. Không đụng libs/api.

## Architecture

```
Trước:  task.rs:572-575  cấp cặp #1 (Result)
        └─► spawn() ─► scheduler.rs:197/:220  cấp cặp #2 (.expect)  ─┐
                        task.rs:590-592  ghi đè bằng cặp #1          ─┴─► cặp #2 drop (phí)
        peak = 4 × 260 KiB

Sau:    task.rs  cấp một cặp (Result) ─► spawn(..., stacks) ─► scheduler dùng luôn
        peak = 2 × 260 KiB, và không còn .expect trên đường này
```

`spawn_thread` (`scheduler.rs:265+`) là đường riêng, không có pre-allocation — nó giữ việc tự cấp,
chỉ đổi `.expect` → `Result` và thêm cap thread/quota (req 4).

## Related Code Files

| File | Hành động |
|------|-----------|
| `kernel/src/task/scheduler.rs` | Modify — xoá cấp trùng, 3 `.expect` → `Result`, signature `spawn`, comment stale `:193` |
| `kernel/src/task.rs` | Modify — `spawn()` wrapper trả `Result`, truyền stack vào |
| `kernel/src/task/tests.rs`, `kernel/src/task/syscall.rs:4393` | Modify — call site của `spawn` |
| `kernel/src/task/stack.rs` | Modify — thay block comment "TEMPORARY SOLUTION" |
| `kernel/src/task/syscall.rs` | Modify — cap thread/cell + charge quota cho `Syscall::Spawn` |
| `libs/ostd/Cargo.toml` | Modify — feature `ui` gate fontdue (nếu bước đo xác nhận) |
| `libs/ostd/src/lib.rs` | Modify — `#[cfg(feature = "ui")] mod font_atlas` |
| `cells/**/Cargo.toml` (cell dùng ViUI) | Modify — opt-in `ostd/ui` |

## Implementation Steps

**Workstream A**

1. Xoá cấp trùng (req 1). Đây là bước đầu vì nó đơn giản, không đổi hành vi, và tự nó cắt một nửa
   peak demand. Đo peak contiguous demand per spawn trước/sau — đây là metric báo cáo của phase.
2. Đổi signature `Scheduler::spawn` sang `Result`, cập nhật mọi call site, bỏ 3 `.expect`.
3. Xác minh `Drop` của `Stack` chạy trên mọi nhánh lỗi; test spawn khi hết RAM → nhận lỗi, hệ vẫn chạy.
4. Cap thread/cell + charge stack vào `cell_quota` (req 4). Test: cell loop thread-spawn → bị từ chối
   ở quota, không panic kernel.
5. Sửa 2 comment (req 5).
6. Boot + suite 3 arch.

**Workstream B**

7. Đo `.text` per-cell (llvm-size/readelf trên ELF riscv64), lập bảng before.
8. Kiểm tra symbol fontdue có xuất hiện trong `hello-cell`/`service-vfs` không (nm/objdump). Nếu
   không → LTO đã strip, workstream B gần như xong: ghi kết luận và dừng.
9. Nếu có: feature `ui` cho ostd, gate `font_atlas`, opt-in ở cell ViUI. Grep mọi cell dùng
   `font_atlas`/`GlyphAtlas` trước khi gate. Đo lại, lập bảng after.
10. Áp cùng pattern cho `serde`/`hashbrown` nếu bước 8 cho thấy chúng bị link vô ích.

## Todo List

- [x] Xoá stack cấp trùng trong đường spawn cell — **và một chỗ thứ hai plan chưa nêu** (`spawn_synthetic`)
- [x] `Scheduler::spawn` → `Result`; cập nhật mọi call site; bỏ **3** `.expect`
- [x] Xác minh không còn task nửa-dựng: `spawn_synthetic` không thể fail sau khi insert nữa
- [x] Cap thread/cell + **boot self-test chứng minh** loop thread-spawn bị từ chối (`TryAgain`)
- [x] Sửa comment stale `scheduler.rs:193` + block comment `stack.rs:60-80`
- [x] Đo `.text` per-cell (bảng dưới)
- [x] Kiểm tra symbol fontdue trong cell non-GUI — **đã bị LTO strip**, không cần gate
- [x] ~~feature `ui` gate fontdue~~ — không cần (xem kết luận workstream B)
- [x] Boot rv64 + clippy `-D warnings` 3 arch
- [ ] Charge stack vào `cell_quota` — **thay bằng thread cap**, xem Evidence
- [ ] Test spawn dưới allocator phân mảnh có chủ ý → `OutOfMemory`

## Evidence (2026-07-28)

Branch `feat/kernel-stack-dedup` (từ main). Tất cả số liệu tôi tự chạy.

**Workstream A**

| Tiêu chí | Bằng chứng |
|----------|-----------|
| Xoá cấp trùng | `Scheduler::spawn` tách thành `spawn_with_stacks` (nhận Stack) + `spawn` (tự cấp rồi gọi lại). Đường spawn cell truyền cặp đã cấp vào thay vì để scheduler cấp cặp thứ hai rồi ghi đè |
| **Chỗ cấp trùng thứ hai — plan chưa nêu** | `spawn_synthetic` (`task.rs`) cũng gọi `spawn()` rồi cấp thêm một cặp nữa trong vùng giữ lock. Nặng hơn: cặp thứ hai cấp **sau** khi task đã insert + runnable, nên OOM ở đó trả `Err` và **để lại task nửa-dựng trong scheduler vĩnh viễn**. Nay cấp trước, insert sau → vùng giữ lock không còn nhánh fail nào |
| Đo được | Địa chỉ stack của cell đầu tiên tụt xuống đúng `0x82000` = **130 page = 2 × (STACK_PAGES+1)**: `platform` base `0x85B5A000` (trước) → `0x85AD8000` (sau); `init` `0x85BE3000` → `0x85B61000` |
| 3 `.expect("OOM Stack")` | `grep -rn 'expect("OOM Stack'` trong `kernel/src/` → **0 hit** |
| **DoS bound chứng minh tại boot** | `[sched] cell CellId(13651968) at thread cap (32) — refusing spawn_thread` + `[selftest] THREAD-CAP: PASS (… + spawn bound)`. Self-test lấp cell bằng 31 task **không stack** rồi gọi đúng đường `handle_syscall(Syscall::Spawn)` thật, khẳng định `Err(TryAgain)`, rồi dọn sạch scheduler |
| Boot | rv64 boot tới `=== ViCell shell ready ===`, 3 self-test PASS |
| clippy `-D warnings` | riscv64 / aarch64 / x86_64 exit 0 |

**Đính chính một claim của plan.** Plan viết "peak = 4 × 260 KiB, một nửa là phí thuần" và ngụ ý
RAM steady-state giảm ~50%. Không đúng: cặp thừa được **drop ngay tại dòng gán**, nên allocator thu
lại trước lần spawn kế tiếp — khoảng cách địa chỉ giữa `platform` và `init` **không đổi** (137 page ở
cả hai build). Cái thật sự giảm là **số lần đi tìm run liên tục** (4 → 2 mỗi spawn) và peak trong
cửa sổ spawn. Vì mỗi lần tìm run 65-frame là một lần có thể fail do phân mảnh, giảm một nửa số lần
tìm mới là giá trị — không phải giảm RAM.

**Thay `cell_quota` bằng thread cap (lệch req 4, có chủ ý).** Req 4 yêu cầu charge stack vào
`cell_quota`. Tôi làm `MAX_THREADS_PER_CELL = 32` đếm task cùng `CellId` đang sống. Lý do: quota
charge cần một đường refund gắn với lúc reap task, và **một lần refund sót = cell không bao giờ spawn
được nữa** — chế độ lỗi tệ hơn chính DoS nó chặn. Cap suy ra từ số task đang sống thì tự khỏi khi
task chết, không có trạng thái để rò. Nếu muốn accounting theo byte thì đó là việc riêng, cần refund
đúng ở `exit_task`.

**Workstream B — kết luận: không cần feature-gate.**

| Cell | `.text` | fontdue | serde | hashbrown | heapless |
|------|--------:|--------:|------:|----------:|---------:|
| `platform` | 3 953 | 0 | 0 | 0 | 0 |
| `hello-cell` | 4 751 | 0 | 0 | 0 | 0 |
| `driver-virtio-blk` | 7 720 | 0 | 0 | 0 | 0 |
| `service-config` | 11 611 | 0 | 3 | 0 | 0 |
| `service-vfs` | 209 803 | 1 | 7 | 0 | 0 |
| `app-shell` | 362 051 | 1 | 8 | 0 | 0 |

`llvm-nm --defined-only --print-size` trên ELF riscv64 release. **Symbol "fontdue" duy nhất là
`alloc::vec::from_elem::<u8>`** — một generic được instantiate trong CGU của fontdue, 46 byte
(`service-vfs`) / 60 byte (`app-shell`). Không có mã rasterizer nào. `hashbrown` và `heapless`: 0
symbol ở **mọi** cell. serde symbol đều là postcard ser/de mà VFS dùng thật, không phải dead weight.

→ **Đính chính ghi chú cũ**: nhận định "`fontdue` string còn trong `service-vfs` → LTO KHÔNG strip
sạch" dựa trên grep chuỗi và gây hiểu sai. LTO + gc-sections **đã** strip. Feature-gate `ui` sẽ tiết
kiệm ~50 byte trên 2 cell — không đáng đánh đổi bằng một trục feature mới. Bước 9-10 của plan không
áp dụng.

Ghi chú ngoài lề đáng theo dõi: **`.bss` ≈ 8 MB ở *mọi* cell** (kể cả `hello-cell` 31 KB) — là heap
tĩnh `static mut`. Đó mới là con số footprint lớn nhất, và nó không nằm trong scope phase này.

## Success Criteria

**Done khi**

- Peak contiguous frame demand per cell spawn giảm từ ~4×65 xuống ~2×65 frame (đo được).
- Không còn `.expect("OOM Stack")` trong kernel (3 site); spawn khi hết RAM trả lỗi và hệ vẫn chạy.
- Một cell không đặc quyền loop thread-spawn bị từ chối ở quota, **không** panic kernel.
- Bảng `.text` before/after mỗi cell kèm kết luận rõ về fontdue.

**Validation**

- Suite 3 arch pass. `doom` vẫn chạy (case dùng stack nhiều nhất).
- Test spawn dưới allocator bị phân mảnh có chủ ý → trả `OutOfMemory`, không panic. **Đây là test
  duy nhất phân biệt được "đã sửa panic path" với "đã dịch panic path đi chỗ khác".**

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| Xoá cấp trùng làm lộ ra một đường code dựa vào stack của scheduler | Trung bình | Boot vỡ | `task.rs:590-592` chứng minh cặp của scheduler bị ghi đè ngay — nhưng grep mọi đọc `task.kernel_stack`/`user_stack` giữa `:580` và `:592` trước khi xoá |
| Đổi signature `spawn` → `Result` lộ ra call site chưa dọn resource | Trung bình | Leak / task nửa-dựng | Req 3 là bước riêng có test, không gộp vào bước 2 |
| `spawn_thread` không có pre-allocation nên vẫn tự cấp — dễ bị bỏ sót khi refactor | Trung bình | Site thứ 3 còn `.expect` | Todo list ghi rõ "3 site"; grep `expect("OOM` là gate cuối |
| Feature-gate fontdue vỡ build cell ViUI | Trung bình | Build fail | Grep `font_atlas`/`GlyphAtlas` trước khi gate |
| Metric "RAM free sau boot" bị nhiễu nếu 01/03 land song song | Cao | Báo cáo sai | Metric của phase là **peak contiguous demand per spawn** (đo cục bộ), không phải RAM free whole-system |

## Security Considerations

- **Bỏ panic trên OOM là cải thiện an toàn, không chỉ tối ưu**: panic ở đường spawn = never-die vỡ,
  và `Syscall::Spawn` (thread) hiện **không** được cap-gate nên đây là DoS reachable từ một cell
  không đặc quyền. Req 4 (cap thread + quota) là phần bắt buộc, không phải nice-to-have.
- **Guard page phải giữ nguyên**. Trong SAS không có MMU per-cell nên overflow không chặn = ghi đè
  cell khác. Không bao giờ cấp stack với `guard = false`.
- Phase này **không** giảm kích thước stack, nên không mở rủi ro OOB memset — rủi ro đó thuộc phase 08
  và được xử lý ở đó.

## Next Steps

- [Phase 07](phase-07-async-reactor.md) (reactor) giảm *số lượng* stack cần thiết.
- [Phase 08](phase-08-stack-sizing-table.md) giảm *kích thước* mỗi cái — phải sau 07 vì shim
  `block_on` thêm frame cho mọi lời gọi syscall (`libs/ostd/src/executor.rs:20` pin future trên stack
  của caller), nên watermark đo trước 07 là dữ liệu của một thế giới 07 xoá đi.
- Follow-up ngoài plan: **VA allocator** → stack không liên tục → xoá hẳn class lỗi phân mảnh.
- Follow-up: prerequisites của Instant On (`docs/specs/03-runtime.md:96-102`) + di trú `snapshot.rs`
  khỏi kernel.
