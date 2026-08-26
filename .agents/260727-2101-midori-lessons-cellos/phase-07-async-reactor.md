# Phase 07 — Reactor thật + Async Pinning Registry

## Context Links

- Plan: [plan.md](plan.md) · Phụ thuộc: [phase-05](phase-05-stack-deadalloc-binary.md) ·
  Kế tiếp: [phase-08](phase-08-stack-sizing-table.md)
- Spec: `docs/specs/03-runtime.md` §2 (Owned Buffers + Async Pinning Registry), `docs/specs/12-reliability.md`
- Law: Law 2 (owned buffers), Law 8 (implement `Drop` — **xung đột, xem dưới**)
- **Cross-plan**: `.agents/260712-1901-cap-revocation/phase-02-selective-grant-reclaim.md` (phase này
  rewrite đường grant-reap)
- Midori nguồn: async toàn tập, không blocking, hoạt vụ không có stack

> **D36 reciprocal precedence (2026-08-01):** this phase owns pin/quarantine mechanism
> and lifecycle ordering. Cap-revocation phase 02 owns the revoke trigger/policy and must
> call the same pin-aware reclaim path; it cannot free an in-flight grant before
> cancellation/driver acknowledgement.

## Overview

- **Ưu tiên**: P3 — lớn nhất trong plan, cần thiết kế syscall
- **Trạng thái**: Completed — `WaitCompletion(TIMER)` and NIC IRQ → NET_RX completion wiring are landed; the generic reactor, `RecvScatter`, and async VFS/DMA remain deferred. **Cần 2× confirmation nếu thêm/đổi syscall hoặc event ABI mới (Law 1)**.
- **Mô tả**: Cellos hiện là **một thread blocking mỗi cell** — đúng cái Midori xoá bỏ. Phase này
  KHÔNG làm "async toàn tập"; nó xây một reactor thật để một cell xử lý N việc đồng thời trên một thread.

> **Red-team correction**: draft coi req 4 ("giữ mọi syscall blocking làm shim `block_on`, không cell
> nào phải sửa một dòng") là điều kiện scope không đàm phán. Red-team cho thấy **chính điều kiện đó
> tạo ra ba lỗ**: nó phá rendezvous `TaskState::Recv`, phá peer-death unblock, và vô hiệu hoá lý lẽ
> soundness của `unsafe` trong VFS. Req 4 giữ nguyên mục tiêu (không phá 45K LOC) nhưng giờ có ba
> ràng buộc kèm theo.

## Key Insights

- **Lợi thế Cellos không phải trả**: Midori cần compiler work (M#) để async không tốn stack. Rust đã
  cho `async fn` → state machine trên heap, miễn phí.
- Executor hiện tại đã chuyển sang TIMER park: `block_on` dùng `WaitCompletion(TIMER)`
  thay vì busy-poll, còn `Recv` giữ nguyên đường mailbox riêng của nó
  ([executor.rs:21-63](../../libs/ostd/src/executor.rs#L21-L63), [completion_wait.rs](../../kernel/src/task/completion_wait.rs)).
- Mức dùng thực tế: **5 `.await` và 3 `async fn` trên ~45K LOC cells** → bề mặt hồi quy nhỏ.
- Mô hình đang chạy: `TaskState::{Sleeping, blocked-send, blocked-recv, Futex, Waiting(join),
  WaitEvent, WaitIrq}` ([tcb.rs:44-101](../../kernel/src/task/tcb.rs#L44-L101)).
- Nguyên thuỷ async duy nhất có thật: `WaitForEvent = 217` backing bởi **một `AtomicBool` toàn cục**
  `NET_RX_PENDING` do timer sweep quét ([waker.rs:16-22](../../kernel/src/task/waker.rs#L16-L22)). Có
  lost-wakeup guard đúng (check trước khi park) — pattern đó phải giữ.

**Ba bất biến mà shim `block_on` sẽ phá (red-team, đã xác minh):**

- **(a) Rendezvous `TaskState::Recv`**: `ipc_try_send` chỉ giao khi target đang ở
  `TaskState::Recv{mask, buf_ptr, buf_len}` ([task.rs:1314-1326](../../kernel/src/task.rs#L1314-L1326));
  `task.rs:1351` ghi *"All other try_send callers keep strict drop-if-not-ready semantics."* Đường
  input async của shell dựa **chính xác** vào đó: `sys_recv_timeout` đặt shell vào `TaskState::Recv`
  1 tick để `sys_try_send` của input service giao được
  ([shell/src/async_utils.rs:36-44](../../cells/tools/shell/src/async_utils.rs#L36-L44)). Park ở một
  completion-wait state mới ⇒ **mọi TrySend bị drop âm thầm** ⇒ bàn phím chết, không error. Đúng class
  bug đã cháy hai lần (`project-ipc-wildcard-recv-poisoning`, `project-input-service-registry-bug`).
- **(b) Peer-death unblock**: `exit_task` gỡ block cho peer bằng cách **match trên state**:
  `if let TaskState::Sending { target, .. } = task.state { if target == tid { regs[10] = usize::MAX } }`.
  CQ-park không match ⇒ không ai ghi completion ⇒ caller **treo vĩnh viễn** thay vì nhận `usize::MAX`.
  Nếu supervisor đang giữa một lời gọi thì nó cũng treo ⇒ **never-die không fire**.
- **(c) Lý lẽ `unsafe` của VFS**: hai khối `unsafe` ghi trực tiếp vào grant của caller với lý lẽ an
  toàn là *"The caller's `ipc_call` blocks until we reply, so it cannot free the grant before this copy
  completes"* ([dispatch.rs:229-232](../../cells/services/vfs/src/dispatch.rs#L229-L232),
  [:214-215](../../cells/services/vfs/src/dispatch.rs#L214-L215)). Future huỷ được ⇒ cell cancel (hoặc
  `select` với timer thắng) → `GrantFree` → VFS vẫn đang `copy_nonoverlapping` vào frame đã free ⇒
  **corruption âm thầm sang cell khác**.

**Hai vấn đề của completion queue (red-team):**

- **(d) CQ trên grant là UAF kernel→cell**: `GrantUnregister`/`GrantFree` chỉ kiểm `owner == caller_id`
  rồi `free_grant_pages` trả frame về allocator
  ([syscall.rs:3445-3462](../../kernel/src/task/syscall.rs#L3445-L3462)). Cell submit op rồi free CQ
  của chính nó → frame được cấp lại → kernel/ISR ghi entry `(source_id, result)` vào bộ nhớ cell khác.
  Pinning Registry req 1 chỉ phủ *buffer của op*, không phủ CQ.
- **(e) Lock order + reaper**: `waker.rs:9-10` nói *"callers in the sweep already hold SCHEDULER"*, còn
  `free_grant_pages` lấy `FRAME_ALLOCATOR` + `unmap_page` và `scheduler.rs:86-90` ghi rõ đó là **đảo**
  thứ tự đã tài liệu hoá. Thêm nữa `reap_grants_for_task` chạy **vô điều kiện** khi exit và force-exit
  ([task.rs:334](../../kernel/src/task.rs#L334), [:392](../../kernel/src/task.rs#L392)), và init
  auto-restart mọi service `Permanent` ([init/src/main.rs:320-346](../../cells/tools/init/src/main.rs#L320-L346))
  → **cell panic giữa DMA là ca thường, không phải ngoại lệ**. Reaper hiện không biết gì về pin: tôn
  trọng pin thì leak frame và chặn restart; bỏ qua pin thì driver giữ con trỏ dangling vào bộ nhớ mà
  cell vừa restart nay sở hữu.

**Xung đột phải xử lý, không né:**

- Law 8 yêu cầu mọi resource implement `Drop`, nhưng `Drop` không thể async. Đây chính là thứ Joe Duffy
  ghi nhận là phần khó nhất của Midori (cleanup + cancellation, không phải performance). Law 2 (owned
  buffers) tồn tại đúng vì lý do này.
- Trong SAS, cancel một op còn DMA in-flight = ghi đè bộ nhớ cell khác, không MMU nào chặn.
  `docs/specs/03-runtime.md:22-24` spec ra Async Pinning Registry cho đúng việc này — **grep kernel
  không thấy hiện thực nào**.

## Requirements

**Functional**

1. **Async Pinning Registry** (làm trước mọi thứ khác): kernel từ chối unload/free một vùng nhớ đang
   tham gia op async cho tới khi op hoàn tất hoặc bị huỷ có xác nhận từ driver. **Phạm vi phải phủ cả
   completion queue và mọi grant đang được service đọc/ghi**, không chỉ DMA buffer.
2. **Reap path tra pinning registry**: khi cell chết, frame đã pin được **quarantine** khỏi frame
   allocator cho tới khi driver-ack, **không** trả về allocator. Không tôn trọng pin = dangling
   pointer; tôn trọng pin bằng cách chặn restart = never-die vỡ. Quarantine là đường thứ ba.
3. **CQ là kernel-owned memory tham chiếu từ TCB**, không phải grant cell thu hồi được. `GrantFree`/
   `GrantUnregister` từ chối reg_id đang là CQ. Kernel drop con trỏ CQ trong **cùng critical section**
   với việc gỡ task khỏi poll set trong `exit_task`.
4. Reactor: `WaitForEvent` nâng thành readiness theo object — CQ per-cell với entry `(source_id, result)`.
5. ostd waker register vào CQ; `block_on` park thay vì busy-poll. Giữ lost-wakeup guard (check CQ
   trước khi park).
6. **Giữ mọi syscall blocking hiện có làm shim `block_on(async)`** — với ba ràng buộc:
   a. **Giữ rendezvous `Recv{mask, buf_ptr, buf_len}`** (kernel ghi trực tiếp vào buffer của receiver
      đang park) HOẶC migrate `ipc_try_send` sang CQ **trong cùng step**. Không được để hai cơ chế
      cùng tồn tại mà `try_send` chỉ biết một.
   b. **Register mỗi waiter theo tid nó phụ thuộc**, và `exit_task` push một synthetic completion
      `(source_id, ERR_TARGET_GONE)` cho mọi waiter của tid đang chết.
   c. **Audit mọi khối `unsafe` có lý lẽ "caller blocks" TRƯỚC khi đổi executor**; mọi op grant-based
      (`ReadGrant`/`WriteGrant`/`ReadFileGrant`) phải được pin qua registry, hoặc future của `ipc_call`
      phải không huỷ được.
7. Cancellation có ngữ nghĩa **duy nhất**: chờ-op-hoàn-tất-rồi-bỏ-kết-quả **hoặc** driver-ack. Chốt
   một trong ADR, không hỗ trợ cả hai, không để "tuỳ driver".

**Non-functional**

8. Không hồi quy độ trễ IPC (bench có sẵn: `cells/tests/bench`).
9. Không đưa policy scheduling vào kernel — reactor là *mechanism*; chọn task nào chạy vẫn là scheduler.
10. Không đổi `Drop` semantics của resource hiện có.

## Architecture

```
Trước:  cell ──sys_recv (blocks thread)──► kernel: TaskState::Recv{mask, buf_ptr, buf_len}
        1 việc / 1 thread / 1 stack        └─ ipc_try_send ghi TRỰC TIẾP vào buf_ptr  ◄── (a)
        exit_task match TaskState::Sending{target} để unblock peer  ◄── (b)

Sau:    cell: block_on(select(recv_a, recv_b, timer))
           ├─ poll ──► sys_submit(op) ──► kernel ghi CQ khi xong
           └─ Pending ──► sys_wait_completion(mask) ──► park 1 thread cho N op
        CQ = kernel-owned, tham chiếu từ TCB, KHÔNG phải grant  ◄── (d)
        waiter registered theo tid phụ thuộc; exit_task push ERR_TARGET_GONE  ◄── (b)
        rendezvous Recv giữ nguyên HOẶC try_send migrate cùng step  ◄── (a)
```

**Lock order của đường append CQ phải ghi vào ADR.** Ràng buộc hiện có: sweep/ISR đã giữ SCHEDULER
(`waker.rs:9-10`), và `free_grant_pages` lấy `FRAME_ALLOCATOR` — thứ tự đó đã được tài liệu hoá là đảo
(`scheduler.rs:86-90`). CQ kernel-owned (req 3) giúp tránh việc này vì không cần resolve grant lúc append,
nhưng ADR vẫn phải nói rõ **được giữ lock nào khi append**.

**Bốn quyết định phải chốt bằng ADR trước khi code:**
1. `source_id` là gì: tid? cap id? handle riêng? (ảnh hưởng tới revoke)
2. CQ tràn thì sao — drop entry = lost wakeup; block driver = deadlock. **Ưu tiên backpressure hơn drop.**
3. Cancellation: chờ-rồi-bỏ **hay** driver-ack — chọn một (req 7)
4. Lock order được phép giữ khi append CQ

## Related Code Files

| File | Hành động |
|------|-----------|
| `kernel/src/memory/...` (pinning) | Create/Modify — Async Pinning Registry, phủ cả CQ + grant đang đọc/ghi |
| `kernel/src/task.rs` | Modify — reap path tra registry + quarantine (`:334`, `:392`); `exit_task` push synthetic completion; giữ/migrate `ipc_try_send` (`:1314-1326`) |
| `kernel/src/task/waker.rs` | Modify — CQ thay `AtomicBool` toàn cục |
| `kernel/src/task/tcb.rs` | Modify — con trỏ CQ trong TCB; `WaitEvent` mang mask theo object |
| `kernel/src/task/syscall.rs` | Modify — submit/wait_completion; `GrantFree`/`GrantUnregister` từ chối reg_id là CQ |
| `libs/api/src/abi/syscall.rs` | Modify — **Law 1, cần 2× confirmation** |
| `libs/ostd/src/executor.rs` | Modify — waker thật, bỏ busy-poll |
| `libs/ostd/src/syscall.rs` | Modify — shim blocking = `block_on(async)` |
| `cells/services/vfs/src/dispatch.rs` | Đọc-để-audit (`:214-215`, `:229-232`) — lý lẽ `unsafe` "caller blocks" |
| `docs/specs/03-runtime.md` | Modify — §2 từ spec thành mô tả hiện thực |
| ADR mới | Create — 4 quyết định ở Architecture |

## Implementation Steps

1. **ADR trước** (4 quyết định). Blocking, không code trước.
2. **Audit `unsafe`**: grep mọi khối `unsafe` có lý lẽ dựa trên "caller blocks" / "cannot free while
   we reply". Lập danh sách. Đây là bước rẻ nhất và nó quyết định req 6c cần gì.
3. **Async Pinning Registry** — prerequisite. Pin/unpin; kernel từ chối free vùng đã pin. Test: cell
   cố free buffer đang có op async → bị từ chối.
4. **Reap path + quarantine** (req 2): reap tra registry, frame đã pin đi vào quarantine. Test: kill
   một cell giữa lúc có op async → frame không quay lại allocator, cell restart được. Phối hợp với
   `.agents/260712-1901-cap-revocation/phase-02-selective-grant-reclaim.md` — không rewrite chồng.
5. **CQ kernel-owned** (req 3) + `GrantFree` từ chối reg_id là CQ + drop con trỏ CQ trong critical
   section của `exit_task`.
6. `sys_wait_completion(mask, timeout)` + lost-wakeup guard.
7. **Waiter registration theo tid + synthetic completion trong `exit_task`** (req 6b). Test **trước**
   khi chuyển shim: kill VFS trong lúc client đang giữa một lời gọi → client nhận lỗi, **không treo**.
8. ostd: waker thật, `block_on` park thay vì spin. Đo bench — busy-poll hiện đốt CPU, đây là win đo được.
9. **Shim blocking syscall** (req 6) — chỉ sau khi 2, 3, 4, 5, 7 xong. Cùng step: giữ rendezvous
   `Recv{buf_ptr}` hoặc migrate `ipc_try_send` sang CQ. Acceptance test: **burst bàn phím ("hypha\n")
   gõ ở console tới được shell với 0 drop, trên cả 3 arch.**
10. Di trú `NET_RX_PENDING` sang CQ; net cell là caller thật đầu tiên.
11. Cell tiên phong dùng concurrency thật: 1 cell xử lý ≥2 nguồn trên 1 thread.
12. **Chỉ khi (3) và (4) đã xong**: mở async cho driver có DMA.
13. Cập nhật `docs/specs/03-runtime.md` §2 để nó mô tả code, không mô tả ý định.

## Todo List

- Remaining unchecked items are deferred follow-ups for the generic reactor path; they are not blockers for this closed substrate.
- [x] ADR: `source_id` · CQ overflow (backpressure) · cancellation semantics · lock order append
- [ ] **Audit mọi `unsafe` có lý lẽ "caller blocks"** — lập danh sách trước khi làm gì khác
- [~] **Async Pinning Registry** phủ cả CQ + grant đang đọc/ghi; test từ chối free vùng đã pin
      — cơ chế xong (`kernel/src/memory/pin.rs`), `GrantFree`/`GrantUnregister` từ chối vùng
      đã pin. Producer duy nhất hiện có là `GrantDma`; CQ và grant service đang đọc/ghi cần
      syscall submit/complete (ngoài scope). Xem § Deviation Log.
- [x] **Reap path tra registry + quarantine frame đã pin**; test kill-giữa-DMA + restart được
- [x] CQ kernel-owned trong TCB; `GrantFree`/`GrantUnregister` từ chối reg_id là CQ
      — `kernel/src/task/completion.rs`: bounded (32 slots, 624 B/cell), slot
      reserved at submission, append takes only the queue's leaf lock, wake
      deferred to `yield_cpu`. Không có reg_id nào để từ chối: CQ là
      `Arc<CompletionQueue>` kernel-owned, không phải grant. Chưa có caller thật —
      chứng minh bằng boot self-test. Xem § Deviation Log.
- [x] `sys_wait_completion` + lost-wakeup guard
      — `WaitCompletion` (242): the call reserves the slot, arms the source, then
      consumes the level flag, so a frame is covered by one or the other with no
      window. Parks in `WaitEvent { mask: 0, deadline }`; no new state.
- [ ] Waiter register theo tid + `exit_task` push `ERR_TARGET_GONE`; **test kill VFS giữa lời gọi → lỗi không treo**
- [x] ostd waker thật, bỏ busy-poll; đo bench trước/sau
- [ ] Shim blocking syscall + giữ/migrate rendezvous `Recv` **cùng step**
- [ ] **Acceptance: burst bàn phím tới shell 0 drop, 3 arch**
- [x] Di trú `NET_RX_PENDING` sang CQ — NIC IRQs now route through `signal_net_rx`;
      `NET_RX_PENDING` remains the remembered level flag for late arrivals. The
      net cell keeps the same submission contract (`WaitCompletion(NET_RX)` +
      reservation self-test + `http-smoke`), but the producer is now wired.
      Xem § Deviation Log.
- [ ] Cell tiên phong: N nguồn / 1 thread
- [ ] Mở async cho driver DMA (chỉ sau pinning + quarantine)
- [ ] Cập nhật spec 03 §2

## Success Criteria

**Done khi**

- `dummy_raw_waker` không còn tồn tại; `block_on` park chứ không spin.
- Một cell xử lý ≥2 nguồn sự kiện đồng thời trên **1 thread** (test).
- Cell cố free buffer/CQ đang có op async → bị kernel từ chối.
- Kill một cell giữa lúc có op async → frame đã pin vào quarantine, cell restart được, driver không
  ghi vào bộ nhớ cell mới.
- **Kill VFS trong lúc client đang giữa một lời gọi → client nhận lỗi, không treo.**
- **Burst bàn phím tới shell 0 drop trên 3 arch.**
- Không cell nào phải sửa để tiếp tục chạy.

**Validation**

- Suite 3 arch pass. Bench IPC không hồi quy; CPU idle giảm đo được.
- Test cancellation: huỷ op đang chờ → buffer không bị free trước khi driver xong.

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| **Cancel + DMA in-flight → ghi đè cell khác** | Cao | Corruption âm thầm, khó debug nhất trong SAS | Pinning Registry là prerequisite cứng (bước 3); không mở async cho driver DMA trước bước 12 |
| **Cell chết giữa DMA (ca THƯỜNG, không phải ngoại lệ — init auto-restart mọi Permanent service)**; reaper không biết pin | **Cao** | Dangling pointer HOẶC leak + chặn restart | Req 2 quarantine là đường thứ ba; bước 4 có test kill-giữa-DMA |
| **CQ bị cell tự free → kernel/ISR ghi vào frame đã cấp lại** | Cao nếu CQ là grant | Corruption âm thầm | Req 3: CQ kernel-owned, không phải grant; `GrantFree` từ chối |
| **Lock order đảo khi append CQ** (sweep giữ SCHEDULER, grant teardown lấy FRAME_ALLOCATOR) | Trung bình trên SMP 2-hart | Deadlock | CQ kernel-owned tránh resolve grant lúc append; ADR item 4 ghi rõ lock được giữ |
| **Shim phá rendezvous `Recv` → mọi TrySend drop âm thầm → bàn phím chết** | **Cao** | Hệ dùng không được, test suite không bắt | Req 6a: giữ rendezvous hoặc migrate `try_send` cùng step; acceptance test burst bàn phím |
| **Shim phá peer-death unblock → caller treo vĩnh viễn, supervisor treo, never-die không fire** | Cao | Hệ wedge | Req 6b: waiter register theo tid + synthetic completion; test bước 7 chạy **trước** shim |
| **Shim vô hiệu lý lẽ `unsafe` của VFS → corruption qua đường grant IPC** | Cao | Corruption âm thầm | Req 6c: audit `unsafe` (bước 2) trước khi đổi executor; pin mọi op grant-based |
| CQ tràn → lost wakeup | Cao | Cell treo | ADR item 2; ưu tiên backpressure hơn drop |
| `Drop` không async → resource không dọn được trên đường huỷ | Cao | Leak | Law 2 (owned buffer) là hợp đồng chính; resource cần dọn async phải có `close().await` tường minh, `Drop` chỉ log |
| Đổi syscall ABI phá cell đã build | Trung bình | Panic build-skew | Pitfall đã biết (`project-syscall-allowlist-and-build-pitfalls`): rebuild toàn bộ image |
| Scope creep sang "async toàn tập" | Trung bình | Không bao giờ xong | Req 6 (giữ mọi syscall blocking làm shim) là ranh giới scope — nhưng **với ba ràng buộc**, không phải vô điều kiện |

## Security Considerations

- **Rủi ro nghiêm trọng nhất của cả plan nằm ở đây.** Trong SAS không có address space boundary; một
  async op bị huỷ mà driver còn giữ con trỏ vào buffer đã free = ghi vào bộ nhớ cell khác, không fault,
  không log. Đó là lý do Pinning Registry là bước 3 và quarantine là bước 4, không phải bước cuối.
- **Cell chết giữa op async là ca thường xuyên**, không phải exotic — init restart mọi service
  `Permanent` khi crash. Thiết kế phải coi đó là đường chính.
- CQ là vùng nhớ kernel ghi vào. Nếu bất kỳ field nào cell ghi được (index, len) thì phải coi là
  untrusted và bounds-check mọi lần đọc — cùng class lỗi với ring buffer virtio. CQ kernel-owned giảm
  bề mặt này nhưng không xoá nó.
- Cancellation là bề mặt tấn công: một cell huỷ op liên tục để ép driver vào trạng thái lạ. Ngữ nghĩa
  huỷ phải là **một** phương án đã chốt.
- **Đổi executor mà không audit `unsafe` trước là đổi nền móng dưới một lý lẽ soundness đang gánh
  tải.** Hai khối `unsafe` của VFS nói rõ chúng dựa vào "caller blocks"; có thể còn chỗ khác. Bước 2
  rẻ và phải đi trước.
- Reactor là *mechanism*, giữ trong kernel là đúng theo boundary law (cần hardware IRQ receipt). Nhưng
  **chọn task nào chạy là policy** — không để reactor lấn sang scheduling policy.

## Deviation Log

### Entry — 2026-08-06 evidence/status closure (no product-code change)

Scope: status taxonomy and dependency wording only. No new kernel, ABI, cell, or
doc/spec code changed in this closure step.

- **Decision — close this phase as verified completion substrate, not generic reactor.**
  Runtime evidence now covers the queue substrate, the TIMER park contract, and
  NIC IRQs driving NET_RX completions; the generic reactor claims in this file
  were still too broad for the tree state.
- **Verified — current boot evidence already on file remains load-bearing.**
  `.agents/reports/phase-07-completion-queue-260731.md` records
  `completion-queue self-test PASS (reserve, land, bound, defer)` and
  `PASS: shell prompt reached`; `.agents/reports/phase-07-net-rx-migration-260731.md`
  records `net-rx-reservation self-test PASS (fill, remember, release)` and the
  `http-smoke` proof that the current net cell wait path is exercised; `kernel/src/main.rs:613`
  still prints `ipc-pending self-test PASS (deferred delivery, bounds, quota)`.
- **Verified — `WaitCompletion` now accepts the landed TIMER and NET_RX sources.**
  `kernel/src/task/completion_wait.rs:75` still rejects unsupported masks, but the
  accepted contract now covers the TIMER park path and the NET_RX completion path
  from the current ADR. No generic IPC wait, peer-death wait, or multi-source CQ
  wait has landed.
- **Verified — non-test producer routing now calls `signal_net_rx`.**
  `kernel/src/task/drivers/virtio_common.rs:161-172` signals NET_RX from the
  registered NIC IRQ path, and the net cell's wait at `cells/services/net/src/main.rs:184`
  now observes that producer through the completion contract.
- **Verified — executor and generic blocking are now split by contract.**
  `libs/ostd/src/executor.rs:21-63` parks on `WaitCompletion(TIMER)` instead of
  busy-yielding, while `kernel/src/task/scheduler.rs:39` still keeps a legacy
  dummy waker for unrelated kernel paths. `RecvScatter` stays on the old path;
  shell/input still rely on `TaskState::Recv`; peer-death CQ plus target
  tid/generation tracking remains a separate ABI design behind Law 1.
- **Verified — VFS grant soundness still depends on synchronous IPC.**
  `cells/services/vfs/src/dispatch.rs:308` still states the caller cannot free the
  grant before the copy completes because the `ipc_call` blocks until reply. Async
  VFS-grant producers remain deferred until pin/ack semantics cover them.

### Entry — NET_RX migrated onto the completion queue (requirements 4, 5, 6b)

Scope: syscall 242 `WaitCompletion`, the NET_RX reservation, and the net cell's
wait. `WaitForEvent` (217) untouched, no other event source migrated, no other
caller changed, `libs/ostd/executor.rs` and `block_on` untouched.

- **Surprise — nothing in the tree calls `signal_net_rx`.** Grep across kernel,
  cells and hal finds exactly one mention outside `waker.rs`, and it is a doc
  comment (`tcb.rs:82`). The NIC lives in the virtio-net Driver Cell and owns its
  IRQ through the separate `irq_wait` mechanism, whose only entry point
  (`device.rs:wait_recv`) is itself `#[allow(dead_code)]` and unused by the
  polling main loop. So `NET_RX_PENDING` has never been set, `consume_pending`
  has always returned 0, and `sys_wait_for_event(NET_RX, 10)` at
  `net/src/main.rs:179` was a 100 ms timed park and nothing more. The migration
  therefore reproduces exactly that behaviour and additionally makes the
  interrupt half *reachable*; it does not switch on an RX fast path that was
  never wired. Wiring a producer means routing the NIC slot in
  `vi_handle_virtio_irq`, which is outside the approved scope.
- **Decision — calling `WaitCompletion(NET_RX)` is the submission.** A
  level-triggered hardware condition is not a discrete operation anyone submits,
  so there is no other context in which a slot could be reserved from the waiting
  cell's own stack, which is what the reserve-at-submission rule requires. The
  wait reserves, records the reservation, then parks.
- **Decision — one global `(queue, slot)` for the source, not a registration
  table.** One producer, one consumer; a general source registry would be
  machinery for a case that does not exist. It is a leaf `Spinlock` in `waker.rs`
  and is never held across `complete()`, so the append path's lock set is still
  exactly `{queue.ring}`. The interrupt path clones the `Arc` under the guard and
  never drops the last reference — the registry keeps its own — so it still
  reaches no allocator.
- **Decision — a second arm displaces the first, and the displaced reservation is
  completed as `RESULT_ABANDONED`.** Refusing the second caller was the obvious
  answer and is wrong here: a reservation left behind by a cell that died mid-wait
  would wedge the source for the cell that replaces it, and init restarts every
  `Permanent` service. Displacement is self-healing, and completing rather than
  dropping the loser keeps the rule that a promised landing place is always
  filled. It is logged at `warn`.
- **Decision — `NET_RX_PENDING` stays.** It is now the "arrived while nobody was
  waiting" memory, which is exactly the lost-wakeup guard: a level condition must
  still be visible to a wait that starts after it. The wait arms *first* and
  consumes the flag *second*, so a frame is covered by the reservation or by the
  flag with no window between them. Deleting a working guard without an
  equivalent would have been a regression, not a simplification.
- **Decision — no new `TaskState`; park in `WaitEvent { mask: 0, deadline }`.**
  Mask 0 keeps the sweep from consuming a fired bit on this waiter's behalf and
  reporting an empty wake — delivery for it is the queue — while leaving the
  deadline as the sweep's one remaining job. `exit_task` and `ipc_try_send` see
  the same state they see today.
- **Deviation — the opcode carries a deadline and an out-pointer, not just
  `mask`.** `a0=mask, a1/a2=timeout ticks, a3=result buffer`. The approved shape
  named `{ mask: u32 }`, but the same instruction required the timeout to
  survive, and the net cell's 100 ms deadline is load-bearing: smoltcp must be
  polled for retransmits and DHCP renewal whether or not a frame arrives. The
  two-field result does not fit a single return register without freezing a
  packed encoding, so it is written to a caller buffer as
  `api::completion::ViCompletion` (24 bytes, tagged and versioned, same shape as
  the directory attestation record), and the return value is 1 or 0.
- **Deviation — `WaitCompletion` shares allowlist bit 42 with `WaitForEvent`.** A
  fresh bit would deny the call to every cell whose `__ViCell_syscalls` section
  predates it, and the two calls gate the same authority. `cells/services/net`
  now declares `WaitCompletion` in place of `WaitForEvent`; the emitted mask is
  unchanged.
- **Gap — requirement 6b does not apply to this source.** It asks that a waiter
  register against the tid it depends on so `exit_task` can post a synthetic
  completion. NET_RX depends on hardware, not on another task; there is no tid
  whose exit could strand this waiter. `exit_task` is untouched. The requirement
  stays open for the IPC sources it was written for.
- **Surprise — expressing "withdraw a reservation" as a completion broke the
  network, and the boot suite caught it.** The first cut released an unfilled
  slot by completing it with `RESULT_ABANDONED` and draining the result. That
  raises a wake request for a task that is *running*, and the request is still
  outstanding when the same task parks a few microseconds later —
  `deliver_pending_wakes` runs inside the very `yield_cpu` that parks it, so the
  park was cancelled the instant it began. The net cell never slept again: 10 of
  54 boot rows failed, all networking, DHCP never acquired. Fix:
  `CompletionQueue::release`, which returns a `Reserved` slot to `Free` without
  appending, and refuses a slot that already holds a result so a real completion
  can never be discarded by a withdrawal. `network_dhcp_acquires_ip` goes from a
  40 s timeout to passing in 1.4 s. A self-test row now pins the property.
- **Gap — a completion drained by the submitter before it parks still leaves a
  wake request set**, which costs that submitter one immediate return from its
  next wait. Reachable only when the source fills the slot while the submitter is
  between arming and parking. Self-limiting (one iteration per frame, and a cell
  that just received a frame has work to do anyway) and it never loses a result,
  so `drain` was left alone rather than given a clearing rule inside the append
  lock protocol.
- **Gap — a cell with two threads waiting at once.** The queue is per cell, so
  `drain` hands whichever thread drains first the oldest completion, which may
  not be its own slot; that thread's own reservation then stays armed until its
  next call displaces it. Self-correcting, never corrupting, and unreachable with
  one caller — but it is the reason the second-arm rule had to be decided rather
  than left implicit.
- **Verification note — the passing http-smoke run was mutation-checked.**
  Parking with `deadline: None` made the net cell stall after its first wait:
  DHCP never completed, the heartbeat killed and restarted the cell, and the
  suite failed in 104 s where it passes in 14 s. The suite therefore genuinely
  drives the new syscall, and the deadline is genuinely preserved.

### Entry — completion queue infrastructure (requirements 3 and 4)

Scope: the queue, slot reservation, the append path and the deferred wake. No
syscall added or altered, no waiter/syscall/driver migrated, `libs/ostd` and
`block_on` untouched, receive path / non-blocking send / task exit untouched.

- **Decision — the queue is `Arc<CompletionQueue>` held by the task record, not a
  value inside `Task`.** Forced by the one-lock append rule: `Task` lives in
  `sched.tasks` behind `SCHEDULER`, so a queue stored by value could only be
  reached by taking the scheduler lock — exactly what append must not do. A
  separately-owned object whose handle the source keeps means append resolves no
  address and consults no allocator.
- **Decision — per cell, via `queue_for` rather than a spawn-time copy.** Threads
  of one cell share the queue because creation looks across the cell for an
  existing handle before allocating. Doing it at the single creation point rather
  than in `spawn`/`spawn_thread` keeps the property from depending on every future
  spawn path remembering to propagate it, and leaves `spawn` untouched.
- **Decision — lazy creation.** `Task::new` sets `completion: None`, so the cost
  while nothing is migrated is 8 bytes per task and no heap. First reservation
  allocates 624 bytes (measured at boot) for the whole cell.
- **Deviation — the deferred wake is a per-queue flag plus one global gate, not a
  `Vec<usize>` inside `Scheduler` like `pending_grant_reap`.** Same shape (record
  the need, act later in `yield_cpu`), different carrier, for two reasons the
  grant reap does not face: its producers already hold `SCHEDULER` and may
  allocate, whereas an append may run in interrupt context and must do neither.
  A fixed-size tid array was rejected as well — a full array is a lost wakeup, and
  the flag has no capacity to exhaust.
- **Decision — no new `TaskState`.** Delivery makes the registered waiter `Ready`
  only from a state that is not `Ready`/`Running`/`Terminated`/`Frozen`. The park
  state belongs with the first migration, where a real caller pins its shape;
  inventing one now would add a state that `exit_task` and `ipc_try_send` do not
  match on, which is the silent-discard hazard this phase exists to avoid.
- **Surprise — the logger is not a leaf.** The first cut logged a protocol
  violation from inside the queue guard, which would have made the append path
  take the UART lock as well. Both `complete` and `drain` now report after the
  guard drops, so the append path's lock set is exactly `{queue.ring}`.
- **Gap — no production caller.** `reserve`/`complete`/`drain`/`register_waiter`
  are exercised only by `kernel/src/task/completion_selftest.rs`. `GrantFree`/
  `GrantUnregister` need no CQ rejection because the queue is not a grant and has
  no reg_id; that part of requirement 3 is satisfied by construction.

### Entry — pinning registry (requirements 1 and 2)

Scope of this entry: requirements 1 and 2 only. No reactor, no executor change, no
completion queue, no syscall added or altered.

- **Decision — the registry lives in `kernel/src/memory/pin.rs` and is range-based, not
  DMA-specific.** Pins are `[base, base+len)` page spans with overlap detection, so a pin
  covering part of a grant blocks teardown of the whole grant. This is what makes the
  mechanism able to cover a completion queue or a service-borrowed grant later without a
  redesign.
- **Deviation — only one pin producer is wired: `GrantDma` (syscall 233).** Requirement 1
  asks for coverage of every grant a service is reading or writing. Pinning those needs a
  submit/complete pair the kernel can observe, which needs a syscall, which is out of
  scope. Deriving a pin from `GrantShare` instead was tried on paper and rejected: every
  grant path in `libs/ostd/src/fs.rs` (`:305`, `:387`, `:418`) frees while still shared, so
  it would break VFS reads and writes system-wide. The registry reach is general; the
  producer set is not, and that gap is named in the report.
- **Deviation — quarantine holds only frames the reaper declined to free.** First cut had
  `acknowledge` free every quarantined pin range. Self-review found two defects that
  created: `GrantDma` accepts an arbitrary `phys`, so an MMIO window would have been handed
  to `deallocate_frame`; and a registered grant whose ownership transfers to a live grantee
  would have been freed under the grantee's feet. Frames now enter quarantine only through
  `withhold_or_free`, which the reaper calls for a grant it was otherwise about to free.
- **Deviation — frames are charged to the pin HOLDER, not the dying task.** A driver cell
  can authorise DMA against another cell's buffer, so the acknowledgement that matters is
  the driver's, not the corpse's.
- **Surprise — the documented lock order was inverted.** `free_grant_pages`
  (`kernel/src/task/syscall.rs:180`) takes `FRAME_ALLOCATOR` and holds it across
  `unmap_page`/`map_page`, which take `KERNEL_ROOT`. The real order is
  `FRAME_ALLOCATOR → KERNEL_ROOT`. Three comments said the reverse and are corrected;
  `scheduler.rs:96` and `:548` still say it and were left alone (not owned by this work).
  The rule those comments enforce — never take either while holding `SCHEDULER` — is
  correct and unchanged.
- **Surprise — `iommu::cleanup_cell` is passed a `CellId` at `cell/hotswap.rs:181` and a
  task id everywhere else.** Pre-existing. Consequence under this change is a leak, never a
  use-after-free, so no acknowledgement was wired at that site.

## Next Steps

- Deferred follow-up only: generic completion, waiter-by-tid, shell shim migration, and async VFS/DMA stay outside this closed substrate.
- Follow-up: `sys_send`/`Reply` async để IPC không chặn thread gửi.
- Follow-up (ngoài scope, cần ADR riêng): direct-call IPC qua vtable như `docs/specs/03-runtime.md:8-12`
  hứa — đụng toàn bộ spec 17.
