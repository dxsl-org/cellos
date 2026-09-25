# Cellos → runtime kiểu BEAM/OTP cho backend — phân tích tính năng & roadmap

> **Status**: Phân tích kỹ thuật (evidence-based). Không phải cam kết ABI, không tạo work order,
> không đóng gate nào. Mọi khẳng định "đang có" đều dẫn `path:line` trong repo.
> **Ngày**: 2026-09-22 · **Trần evidence của mọi kết quả nêu ở đây**: `host`/`qemu`.

---

## 1. Kết luận ngắn

1. **Cellos đã có phần khó nhất của OTP ở tầng cell**: mailbox kernel-owned có selective receive
   theo sender, `NotifyOnExit` (204) một lần, exit-reason gửi tới supervisor, restart policy
   Permanent/Transient/Temporary, intensity 5 lần/10 s, service registry tự xoá provider khi chết.
   Đây là "let it crash + restart" thật, đã chạy QEMU — không phải slide.
2. **Bốn khối còn thiếu để thành backend BEAM-class**:
   (a) song song + cancellation *bên trong* một cell (reactor generic chưa land);
   (b) chi phí/scale của "process": **lớp cell siêu nhẹ đã được thiết kế** (Spec 19 §3 —
   *per-request server profile*, quyết định D5, mục tiêu 1000 cell cô lập) nhưng **chưa implement**:
   hôm nay `MAX_CELLS=64`, ~512 KiB stack/cell, spawn **9 ops/s**, và lần đo trực tiếp dừng ở **n=8–9**;
   (c) distribution — remote dispatch/relay bị gate, hôm nay chỉ có local ingress;
   (d) độ sâu network/ops — IPv4-only, socket budget nhỏ, `httpd` một kết nối một lúc.
3. **Vượt BEAM ở đúng chỗ BEAM yếu**: không GC, code native, zero-copy grant cho payload lớn,
   preemptive fixed-priority + RT hart, và cô lập nhiều lớp (W^X, capability, Tier-2 page table)
   cho code không tin cậy — thứ BEAM không có tương đương.
4. **Không nên copy BEAM**: mailbox vô hạn (Cellos bounded + backpressure là lựa chọn tốt hơn),
   per-process GC, hot-code-load theo module, và `!` không bao giờ block.
5. **Đường ngắn nhất tới backend dùng được** không phải "làm BEAM": nó là B0→B3 trong §5
   (actor/supervisor library thuần userspace → concurrency trong cell → scale per-cell →
   network depth). Distribution (B4) là optional cho single-node backend.

---

## 2. Đối chiếu BEAM/OTP ↔ Cellos (từng khối)

| # | Khối | BEAM/OTP | Cellos hôm nay | Trạng thái |
|---|---|---|---|---|
| 1 | Đơn vị thực thi | Process, heap riêng, ~300 B–vài KiB | Cell = task trong SAS dùng chung address space, cô lập bằng LBI (W^X sau relocation: `kernel/src/loader/wx.rs`, `docs/specs/19-hardware-isolation-layers.md` §2) | Có, nhưng thô hơn |
| 2 | Heap/GC | GC per-process, pause ngắn | Không GC. Arena tĩnh 1 MiB/cell (`libs/ostd/src/heap.rs:25-42`), có `declare_custom_heap!`; quota kernel mặc định 16 MiB (`kernel/src/memory/cell_quota.rs:15-18`) | Khác mô hình (tốt hơn cho latency) |
| 3 | Mailbox | Hàng đợi vô hạn, `!` không block | **Có mailbox bounded**: `PendingMailbox` kernel-owned (`kernel/src/task/pending_mailbox.rs:128-131`), sâu 64 (`HOTSWAP_MSG_QUEUE_DEPTH`, `kernel/src/task/tcb.rs:27`), input event 512 (`tcb.rs:37`). Đầy → sender bị **Backpressure** (`kernel/src/task.rs:296-300`, `:1959-1960`) | Có (semantics khác) |
| 4 | Send | `!` luôn thành công, async, copy | `sys_send` = **handoff đồng bộ**: copy kernel-owned vào mailbox đích rồi sender park ở `TaskState::Sending{target, delivery_id}` cho tới khi *đúng message đó* được tiêu thụ (`kernel/src/task/ipc_wire.rs:19-25`, `kernel/src/task.rs:1958-1960`); mailbox đầy → `Backpressure`. `sys_try_send` = giao-không-chờ, chỉ khi đích đang `Recv{mask}` khớp, ngoài ra **drop** (trừ input service) (`kernel/src/task.rs:2275-2350`) | Có, khác mạnh (không có "fire and forget" thật) |
| 5 | Selective receive | `receive` khớp *mọi* pattern trong mailbox | Chỉ khớp **theo sender**: `mask == 0` (wildcard) hoặc `mask == sender_tid` (`kernel/src/task/syscall.rs:1515-1518`, `:1629-1633`) | Một phần |
| 6 | Call/reply | `gen_server:call` + ref monitor | `sys_reply` + `current_caller` short-circuit (`kernel/src/task.rs:2368-2383`); helper `service_call*`/`ServiceRef` (`libs/ostd/src/ipc.rs`, `libs/ostd/src/service.rs`) | Có |
| 7 | Message size | Term, không giới hạn thực tế | **4096 B/message** (`libs/api/src/ipc.rs:21`), reply phải chừa headroom envelope (Spec 17 §5); payload lớn đi đường grant | Giới hạn cứng |
| 8 | Links/monitors | 2 chiều, bất kỳ process nào, có `DOWN` | **Chỉ một chiều + một lần**: `NotifyOnExit=204` (`kernel/src/task/syscall.rs:2156-2159`), đòi `SpawnCap` (`:3829-3855`), tự gỡ sau lần đầu, đích đã chết thì có death record tổng hợp (`scheduler.rs:87-91,164-180,1144-1175`) | Một phần |
| 9 | Exit reason | `{reason, ...}` lan theo link | Có: reason non-zero gửi kèm payload cho watcher; init decode 8 byte đầu (`cells/tools/init/src/supervisor.rs:17-24`) | Có |
| 10 | Supervision tree | Lồng nhau, one_for_one/one_for_all/rest_for_one, backoff | **Một tầng, one_for_one**: init khớp `service.tid == dead` rồi respawn (`cells/tools/init/src/supervisor.rs:66-104`); policy `Permanent/Transient/Temporary` (`service_table.rs:4-9`); intensity 5/1000 tick, hết quota thì **bỏ mặc service đó** (`supervisor.rs:8-10,89-107`); không backoff, không cascade | Một phần |
| 11 | Application packaging | `application` behaviour + `.app` + release | Không có đơn vị "application". Có `declare_manifest!` + manifest ELF + `/bin` | Thiếu |
| 12 | Registry tên | `global`, `via`, atom | `service_id: u16` → tid, **bảng 32 entry** (`kernel/src/cell/service_registry.rs`), kernel-owned, tự clear khi chết; ID well-known 1–16 (`libs/api/src/abi/syscall.rs:1037-1079`) | Có, namespace hẹp |
| 13 | Concurrency trong 1 unit | N process nhẹ | 1 thread/cell mặc định; có thread trong cell (`libs/ostd/src/task.rs:24-39`) và executor `block_on` (`libs/ostd/src/executor.rs`) nhưng chỉ 5 `.await`/3 `async fn` trong ~45K LOC cells (`.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md`) | Thiếu |
| 14 | Scheduler | Preemptive, reductions, công bằng | Preemptive theo timer 10 ms, 3 lớp FIFO (Background/Normal/RealTime), `MAX_HARTS=2` (hart 1 = RT), steal việc non-RT; **không** reductions/fair-share/priority-inheritance; preempt-tức-thì chỉ RV64 (`kernel/src/task/scheduler.rs:508-555`) | Có, khác mô hình |
| 15 | Hot code load | 2 version module, state migrate | Hotswap **do supervisor điều phối**: freeze → mailbox FIFO chuyển sang bản mới → đổi registry atomically (`kernel/src/cell/hotswap.rs:299-391`); đã chứng minh bằng QEMU với `hotswap-demo` v1→v2 giữ counter (`tests/integration/tests/hotswap-smoke.rs:132-170`). Generic/state-required: scope-gated, chưa có rollback/ABI versioning | Một phần |
| 16 | Distribution | Node, `{Name,'node@host'}`, net_tick | C2C có envelope V1 (112 B header, cap 3.712 B payload), dedup 16 entry/30 s, 16 replay floor, server epoch, Noise KKpsk0 TCP — **Noise session chưa nối vào broker**, `RemoteEndpoint` trả `NotSupported`, remote dispatch tắt (`cells/services/net-broker/src/main.rs`, `libs/ostd/src/cluster_endpoint.rs`) | Thiếu (đã có hợp đồng) |
| 17 | Persistence/ETS | ETS/DETS/Mnesia | Không có bảng chia sẻ. Có `CellosFS` CoW + dual superblock + checksum, VFS `sync()` sau mỗi write (`cells/services/vfs/src/backend_cellosfs.rs:145-180`), power-cut fuzz ở tầng model host (`libs/cellos-fs/tests/power_cut_fuzz.rs`), state stash kernel 64 key × 1 MiB (`kernel/src/cell/state_stash.rs:15-22`) | Thiếu (ETS-like) |
| 18 | Observability | observer/recon, tracing, crash dump | Audit ring **volatile** với `CellSpawn/CellFault/CellExit/CellHung/RtDeadlineMiss/SyscallDenied` (`kernel/src/audit.rs:38-57`), `ps/top/free/uptime` trong shell; **không** `/proc`, không đọc được mailbox depth/CPU per-cell từ userspace (`ps` chỉ là telemetry giới hạn) | Yếu |
| 19 | Isolation | Trong VM, NIF phá cả VM | Nhiều lớp: W^X (Layer A), capability (manifest ∩ ceiling, `kernel/src/loader/governed_spawn.rs:103-130`), Tier-2 page table riêng (Spec 22, feature `native-domains`, evidence RV64/QEMU) | Mạnh hơn |
| 20 | FFI/native | NIF block scheduler → hại cả VM | Cell native là code thật, không có VM ở giữa; hệ quả: payload không serialize/deserialize | Mạnh hơn |

---

### 2.1 Ba hệ quả thiết kế khi viết actor trên Cellos (quan trọng hơn bảng trên)

1. **Không có "fire and forget" cho cell**: `sys_send` publish vào mailbox đích rồi park sender
   ở `Sending{target, delivery_id}` cho tới khi message được tiêu thụ (`kernel/src/task.rs:1954-1985`);
   chỉ khi đích **đang** park ở `Recv` khớp mask thì sender mới đi tiếp ngay. Hệ quả: gửi vòng
   A→B→A, hoặc cell tự gửi cho chính mình khi chưa `Recv`, là **deadlock** — không phải chờ trong buffer.
   (`ipc_post_nonblock` là fire-and-forget thật nhưng hiện chỉ dùng nội bộ kernel cho `GpuFlush`/`GpuCursor`,
   chưa expose cho cell.) Muốn async thật: `sys_try_send` + chấp nhận drop, hoặc một ABI post mới.
2. **Một recv buffer + selective receive theo sender**: mọi protocol chia sẻ không gian byte-0
   (Spec 17 §3), nên N actor trong 1 cell phải tự demux bằng envelope (`0xAC`, `AppContext`) —
   không thể "mỗi actor một mailbox" như BEAM nếu không qua demux tầng userspace.
3. **`NotifyOnExit` one-shot + cần `SpawnCap`**: worker không tự monitor worker khác. Muốn "link" hai
   cell bất kỳ phải mở ABI (xem ADR #2 ở §8). Đây là lý do B0 buộc supervisor phải là cell được cấp cap.

---

### 2.2 Lớp cell siêu nhẹ — đã được thiết kế (Spec 19 §3, quyết định D5)

Đây là "2 loại cell" trong thiết kế: **không phải 2 kind/ABI khác nhau, mà 2 profile scale**
(`docs/specs/19-hardware-isolation-layers.md:87-134`):

| Profile | Định nghĩa | Trạng thái |
|---|---|---|
| **Large-app** | vài cell lớn, quota MiB, stack sâu | **đang chạy** (mặc định hôm nay, `MAX_CELLS=64`) |
| **Per-request server** | "thousands of very light cells, one per request, each with a real isolation boundary" — **D5 nhận 1000 cell đồng thời làm mục tiêu qualification**, không phải năng lực hiện tại | **đã accept làm goal, chưa implement** |

Ràng buộc ghi rõ trong spec: "this ruling authorizes **no runtime or ABI change**"
(`19-hardware-isolation-layers.md:106`) — nên trong code hôm nay **không** tồn tại một cell kind siêu nhẹ
riêng; nó là profile sẽ đạt được bằng 4 thay đổi hạ tầng.

**Bằng chứng đo (2026-07-31, `.agents/reports/d5-cell-scale-measurement-260731.md`)** — spawn cell park
cho tới khi kernel từ chối, QEMU 2 GiB, `MAX_CELLS` nâng 512, `MAX_SLOTS=512`:

- Dừng ở **n=8** (sau khi suite chạy), **n=9** khi chạy trên bộ nhớ chưa phân mảnh ⇒ không phải fragmentation.
- **Trần thật khi đó là 190 MiB RAM hardcode** (`FALLBACK_MEMORY_MAP`), không phải per-cell cost:
  "the per-request server profile **cannot be reached by raising constants**".
- Thứ tự việc đúng theo đo lường: (1) parse DTB memory node; (2) share `.text`/`.rodata` refcount;
  (3) demand-paged stack; (4) **cuối cùng** mới nâng bảng.

**Cập nhật trạng thái từng bước (kiểm tra 2026-09-22):**

| Bước | Trạng thái |
|---|---|
| (1) DTB memory node | ✅ **đã land**: `kernel/src/boot/dtb_memory.rs` + `fallback_boot_info(dtb)` dựng map từ FDT (`kernel/src/boot.rs:545-573`); `MemInfo=243` trả `ViMemInfoV1{total,used,free}` (`kernel/src/task/syscall.rs:4899-4920`), có `/bin/free` và `capacity-observability` test |
| (2) Shared immutable frames | ❌ chưa — loader vẫn copy toàn bộ ELF mỗi spawn (`kernel/src/loader/elf.rs`) |
| (3) Demand-paged stacks | ❌ chưa — vẫn cấp sẵn (Spec 19 §3, `docs/specs/12-reliability.md` §4.1) |
| (4) Nâng `MAX_CELLS`/`MAX_SLOTS` | ❌ chưa (và đo lường đã chứng minh nâng suông là vô ích) |
| Gate N=64/128/256/512 | ❌ **chưa đo lại** sau khi DTB land — `plan-portfolio.md:33-35` vẫn ghi promotion cần baseline này |

**Doc drift — đã đóng (2026-09-22)**: báo cáo đo đề nghị sửa Spec 19 §3 để đưa DTB lên đầu
("it should be amended to put the memory map first"). Amendment đã được ghi vào
`docs/specs/19-hardware-isolation-layers.md` §3 — kèm số đo n=8–9, thứ tự việc 4 bước, và 5 ràng buộc
mixed-profile — và header của spec đã cập nhật ngày amendment.

**Hệ quả cho backend**: con số "1000 cell" là *aspiration có gate*, không phải năng lực. Muốn dùng
cell như process, phải làm B2 theo đúng thứ tự trên và **đo lại** N — nút chặn giờ không còn là RAM nhìn thấy
mà là per-cell cost (ELF copy + stack cấp sẵn) và spawn rate 9 ops/s.

---

### 2.3 Chạy cả hai profile cùng lúc trên một máy — phân tích khả thi

**Kết luận: được, và một dạng mix đã đang chạy.** Hai profile không phải hai chế độ loại trừ nhau hay hai
partition phần cứng; chúng là **policy tài nguyên per-cell**. Hôm nay máy đã chạy hỗn hợp: cell có stack
64 page (`vfs`) cạnh cell 16 page (`shell`, `net`), cell `RealTime` cạnh `Background`. Cell xử lý data
không "siêu nhẹ" được — điều đó không mâu thuẫn: nó chỉ nhận **profile khác**, không phải bị loại khỏi máy.

#### A. Nền tảng đã per-cell (vì sao mix khả thi)

| Tham số | Cơ chế hiện tại | Nguồn |
|---|---|---|
| Stack pages | `stack_pages_for(name)` → 16 hoặc 64 page | `kernel/src/task.rs:583-594` (hardcode theo **tên**) |
| Heap quota | `LaunchState.quota_limit` → `QuotaReservation` | `kernel/src/task/launch.rs:32,134-138`; hiện **luôn** = `DEFAULT_QUOTA_BYTES` 16 MiB ở cả hai call site `kernel/src/loader/governed_spawn.rs:177,210` |
| Priority | `LaunchState.priority` + RT hart routing | `launch.rs:36,154`; `kernel/src/task/scheduler.rs:497-505` |
| Tier (SAS vs domain) | manifest `tier` byte + `PROTECTION_CLASS_*` | Manifest v2 |
| Trần thread/cell | `PER_CELL_THREAD_CAP` (biên fragmentation) | `scheduler.rs:9-20` |
| Watchdog | chỉ RT (5 s); heartbeat opt-in | `scheduler.rs:1653-1694` → data cell `Background` chạy dài **không** bị kill oan |

⇒ **Cơ chế "2 loại cell cùng lúc" đã tồn tại**; thiếu (1) *nguồn policy* để chọn các tham số này theo
profile, và (2) *chỗ trong bảng toàn cục* để chứa đồng thời nhiều cell nặng lẫn nhiều cell nhẹ.

#### B. Năm điểm coupling phải xử lý

1. **Bảng toàn cục cứng**: `MAX_CELLS=64` (mảng tĩnh `IN_USE`/`DMA_IN_USE`/`cell_owners`) + `MAX_SLOTS=512`.
   Heavy cell chiếm **1 slot như light cell** nhưng ăn nhiều RAM/VA hơn → D5 gate (N=64..512) phải đo
   **khi có M heavy cell thường trú**, không phải sweep đồng nhất. Sweep thuần light sẽ cho con số
   không dùng được cho mix thật.
2. **VA stride 32 MiB cố định** (`CELL_VA_STRIDE = 0x200_0000`, `kernel/src/loader/va_alloc.rs:47`):
   một data cell có code+data+heap vượt 32 MiB VA **không tồn tại được hôm nay**. Bulk buffer vẫn đi được
   qua **grant** (identity-mapped, trần 4096 page = 16 MiB/grant, `kernel/src/task/syscall.rs:150-151`,
   cấp nhiều grant liên tiếp được) hoặc qua VFS — nhưng heap/index lớn trong VA riêng thì không.
   → cần **VA budget biến đổi theo profile**; điểm này **không** có trong Spec 19 §3 lẫn thứ tự D5.
3. **Supervisor model**: `init` là bảng **tĩnh** (`service_table::configured()`), intensity tính theo từng
   service (5/10 s), và `NotifyOnExit` đòi `SpawnCap` (`kernel/src/task/syscall.rs:3829-3855`).
   Nhóm cell churn cao (per-request) **không được** nhét vào bảng đó → phải có **supervisor cell
   userspace** với rate-limit riêng (B0), `init` chỉ giữ service thường trú.
4. **Isolation**: cell nhẹ Tier 1 SAS được bảo vệ bởi LBI (safe Rust + signing), **không** bởi MMU.
   Nếu lớp nhẹ xử lý input **không tin cậy** thì **bắt buộc** Tier 2 domain (Spec 22) — và admission
   Tier 2 hiện chưa có route công khai (`docs/app-development-guide.md`) ⇒ mixed deployment cho code
   untrusted phụ thuộc B7. Với handler first-party trusted, Tier 1 nhẹ là hợp lệ và đủ.
   Hệ quả trực tiếp cho ví dụ của bạn: *data cell* (trusted, first-party) chạy Tier 1 heavy **ổn**;
   *cell xử lý request từ mạng* thì không được coi là "nhẹ cho nhanh".
5. **Fragmentation + spawn rate**: mỗi thread cần một run frame **liền kề** `STACK_PAGES+1`
   (`scheduler.rs:9-13`) → trộn heavy (giữ vùng lớn, sống lâu) với light (churn) dễ làm cạn run liền kề.
   Spawn 9 ops/s **chỉ chặn lớp nhẹ** — heavy cell sống lâu không bị. Nên mix hôm nay chạy được;
   chỉ "per-request" mới cần B2.

#### C. Đề xuất triển khai (không cần đổi ABI)

- Mở rộng **bảng policy kernel-side đã có**: `LaunchProfile` hiện chỉ mang authority
  (`child_ceiling`, `denial_label`, `requires_lifecycle_authority` — `kernel/src/loader/launch_profile/mod.rs:30-38`),
  keyed theo `(caller, route, target)`. Thêm `{stack_pages, quota_bytes, va_budget, default_priority}`
  vào chính struct đó (hoặc một bảng song song cùng khoá) — cách này đối xứng với `boot_ceiling` /
  `reviewed_user_target_ceiling` đang làm cho capability.
- Plumbing **đã có sẵn**: `LaunchState` đã mang `quota_limit`, `priority`, `cluster_mode`, `pku_*`
  (`launch.rs:32-39`) — chỉ cần nguồn giá trị thay vì hằng số.
- **Không chạm `libs/api`** ⇒ không cần Law 1. Nếu sau này muốn app tự khai profile qua manifest thì
  đó là **Manifest v3** (đang `PHASE07_BLOCKED`) — đừng làm sớm.
- Bổ sung policy phân bổ: ví dụ trần `H` slot heavy + `N` slot light, hoặc quota pool theo class,
  để một workload không ăn hết bảng slot của workload kia.

#### D. Acceptance cho mixed deployment (test mới `tests/integration/tests/mixed-profile.rs`)

1. Cùng một run: 1 heavy cell (heap lớn + grant 16 MiB) + 100 light cell churn → đo p99 của heavy cell
   **và** spawn rate của light, chứng minh hai bên không phá nhau.
2. Kill heavy → light không hồi tố; kill light → heavy không restart (supervision độc lập).
3. Churn ≥ 30 phút: theo dõi **run frame liền kề lớn nhất** còn cấp được — chứng minh fragmentation
   không tích luỹ (đây là chỉ số mà `MemInfo` chưa cung cấp, cần thêm).
4. Trần evidence: `qemu`; không suy diễn sang `physical`.

#### E. Tóm lại

| Câu hỏi | Trả lời |
|---|---|
| 2 loại cell chạy cùng lúc được không? | **Được** — cơ chế per-cell đã có; hôm nay đã chạy hỗn hợp ở dạng thô |
| "Tuỳ theo nhu cầu" (cấp theo profile) cần gì? | Nguồn policy per-spawn (§C) + bảng động/phân bổ slot (§B1) + VA budget biến đổi (§B2) |
| Cell xử lý data có phải "siêu nhẹ" không? | Không — nó là **heavy profile**; cái phải siêu nhẹ là cell *per-request*, và hai cái này sống chung được |
| Chặn lớn nhất | Không phải kiến trúc, mà là: bảng tĩnh + VA stride 32 MiB + spawn rate 9 ops/s (cho lớp nhẹ) |
| Rủi ro phải đo trước khi hứa | Fragmentation do trộn stack liền kề; và nếu lớp nhẹ nhận input untrusted thì phải Tier 2, không được để Tier 1 SAS |

---

## 3. Vì sao thiết kế này *không* bị hạn chế như BEAM

| Hạn chế của BEAM | Cellos |
|---|---|
| Copy mọi message giữa process heap | Kernel copy ≤4096 B; payload lớn đi **grant pages** (zero-copy). Ring SPSC 16×64 B đã có trong `libs/api/src/services/ring_channel.rs`, **nhưng cầu nối kernel cho cell liên kết riêng chưa tồn tại** — cell hiện luôn đi đường syscall (`kernel/src/fast_ipc.rs` tự ghi là scaffolding; `docs/performance-report.md` §"Spec vs. Implementation Gap") |
| GC per-process, pause ảnh hưởng tail latency | Không GC; allocator free-list trong arena tĩnh/custom (`libs/ostd/src/heap.rs`); heap **leak là OOM → cell chết → supervisor restart** thay vì pause toàn hệ |
| NIF/port block scheduler hoặc phải copy | Code native chạy trực tiếp trong cell |
| Không có real-time | 3 lớp ưu tiên + RT hart + watchdog RT (`scheduler.rs:65-83,1653-1694`) — có, dù chưa hard-RT (WCET chưa đo, `docs/specs/12-reliability.md` §3: RT ~45 %) |
| Cô lập chỉ trong VM | W^X + capability + Tier-2 domain cho code không tin cậy (Spec 18/19) |
| Hot upgrade giữ state ở mức module | Hotswap cấp cell với state handoff + FIFO mailbox (đã QEMU-verified ở mức demo) |

Đánh đổi phải chấp nhận: **không unwinding** — build là `panic = "abort"` (`Cargo.toml:195-201`),
`ostd` panic log rồi `sys_exit(1)` (`libs/ostd/src/startup.rs:124-136`).
Nghĩa là "let it crash" ở Cellos = *mất sạch heap của cell*, restart từ state bền vững.
BEAM cũng mất state process, nhưng Cellos **không** có `Drop` glue đảm bảo trên đường panic
(Law 8 trong `README.md` có giới hạn này) — mọi thứ cần sống sót phải nằm ngoài heap của cell.

---

## 4. Nút thắt cứng hôm nay (số liệu)

| Giới hạn | Giá trị | Nguồn |
|---|---|---|
| Cell đồng thời (quota table) | **64** | `kernel/src/memory/cell_quota.rs:15` |
| VA slot cho cell PIE | **512** × 32 MiB stride | `kernel/src/loader/va_alloc.rs:48` |
| Heap/cell mặc định | 1 MiB arena (`ostd`), quota kernel 16 MiB | `libs/ostd/src/heap.rs:25`, `cell_quota.rs:17-18` |
| Stack/cell | 2 guard page + 64 page usable (~256 KiB/stack, 2 stack) | `kernel/src/task/stack.rs:62`, `docs/specs/19` §3 |
| Message IPC | **4096 B**; TCP inline ≤3840 B | `libs/api/src/ipc.rs:21-23` |
| Mailbox sâu | 64 (thường) / 512 (input event) | `kernel/src/task/tcb.rs:27,37` |
| Completion queue | **32 slot/cell** | `kernel/src/task/completion.rs` |
| Service registry | **32 entry** | `kernel/src/cell/service_registry.rs` |
| Harts | **2** (`MAX_HARTS`), RT ở hart 1 | `kernel/src/task/smp.rs:11-19` |
| Quantum | 10 ms | `kernel/src/task.rs:712-725` |
| **Spawn rate (đo được)** | **9 ops/s (~111 ms/spawn) — FAIL mục tiêu** | `docs/performance-report.md:87-89` |
| **Cell đồng thời (đo 2026-07-31)** | **n=8–9** trên guest 2 GiB (trần khi đó: RAM 190 MiB hardcode; nay đã sửa bằng DTB, **chưa đo lại**) | `.agents/reports/d5-cell-scale-measurement-260731.md` |
| IPC round-trip (QEMU TCG) | p50 41.8 µs / p99 91.7 µs | `docs/performance-report.md:45-55` |
| Context switch (proxy) | p50 11.9 µs / p99 25.6 µs | `docs/performance-report.md:80-85` |
| Allocator commitment | 76.08 MiB (mục tiêu < 10 MiB) | `docs/performance-report.md:84-85,124-135` |
| RT watchdog | 500 tick ≈ 5 s (chỉ RT) | `kernel/src/task/scheduler.rs:65-83` |
| Socket budget | 18 (16 user + 1 DHCP + 1 ARP), POSIX shim 8; Noise pool K ≤ 4 | `cells/services/net/src/socket_table.rs:14-15`, `libs/api/src/posix/net.rs:19` |
| UDP recv | ≤512 B/lần, kèm 6 B header nguồn | `.agents/260624-cell-to-cell-anywhere/plan.md` (Known constraints) |
| Địa chỉ mạng | IPv4-only (`TcpConnect { addr: [u8; 4] }`), **không DNS resolver** (stub) | `libs/api/src/services/ipc.rs:315-318`; C2C plan |
| HTTP server | **một kết nối một lúc** | `cells/services/httpd/src/main.rs:4` |
| Remote call | blocking, **1 in-flight/peer** | `.agents/260624-cell-to-cell-anywhere/phase-00-remote-call-api-contract.md` |

---

## 5. Roadmap B0 → B7

Mỗi phase ghi rõ: mục tiêu · việc · acceptance · phụ thuộc lane đang có · rủi ro.
Thứ tự **không** bắt buộc tuần tự ngoài phụ thuộc đã nêu; B0–B2 là đường găng cho single-node backend.

### B0 — Actor & Supervisor library (userspace, **không đổi ABI**)

- **Mục tiêu**: một app viết được "supervisor tree" khai báo, không phải sửa `init`.
- **Việc**: crate `libs/ostd/src/actor/` (mailbox loop trên `sys_recv(mask)`, dispatch theo postcard
  discriminant, `call/reply` qua `service_call_typed`) + `supervisor` library (child spec:
  id, path, policy, intensity, backoff) dùng `sys_spawn_from_path` + `NotifyOnExit`; thêm
  `invalidate()`-on-restart tự động trong `ServiceRef` (hiện phải gọi tay, `libs/ostd/src/service.rs`).
  Khung đã có sẵn để dùng thay vì viết mới: `ostd::dispatch::MessageHandler` + `run_service`
  (`libs/ostd/src/dispatch.rs:17-102` — **hiện không service nào dùng**), `ostd::service_entry!`
  + `CellRuntime::run` (`libs/ostd/src/runtime.rs:386-449`), `AppContext::run_with_lifecycle`
  (`libs/ostd/src/app.rs:129-185`), `declare_manifest!`/`declare_syscalls!`.
  Lưu ý: **không có IDL/codegen** — mọi message là enum Rust viết tay + `postcard`
  (`libs/ai-proto/Cargo.toml` chỉ phụ thuộc serde/postcard), nên actor library phải chấp nhận
  kiểu viết tay đó hoặc tự sinh macro, đừng giả định có schema compiler.
  **Bảng child phải động** (không phải mảng tĩnh như `service_table::configured()`), có rate-limit và
  backoff riêng: lớp per-request churn cao không thể nằm trong bảng restart-intensity của `init`
  (xem §2.3.B3), và `NotifyOnExit` đòi `SpawnCap` nên supervisor phải là cell được cấp cap.
- **Acceptance**: demo cell "backend" 1 supervisor + 3 worker; `kill` một worker → restart < 1 s,
  log exit reason; crash-storm 6 lần/10 s → supervisor bỏ mặc worker đó và **cell khác vẫn sống**
  (đúng semantics hiện tại của `init`, `supervisor.rs:89-107`); test QEMU mới
  `tests/integration/tests/actor-supervisor.rs`.
- **Phụ thuộc**: không. **Rủi ro**: `NotifyOnExit` đòi `SpawnCap` → supervisor phải là cell được cấp cap;
  nếu muốn worker monitor lẫn nhau thì phải mở ABI (đưa vào B1).

### B1 — Concurrency trong cell + cancellation (mở đường "N request / 1 cell")

- **Mục tiêu**: một cell phục vụ N nguồn đồng thời trên 1 thread, có hủy an toàn.
- **Việc**: hoàn tất các mục còn lại của reactor `phase-07` (`.agents/260727-2101-midori-lessons-cellos/
  phase-07-async-reactor.md`): `WaitCompletion` đa nguồn, đăng ký waiter theo tid + synthetic
  completion khi peer chết, waker thật trong `ostd`, CQ overflow = **backpressure** không drop;
  ADR chốt ngữ nghĩa cancellation (chờ-hoàn-tất-rồi-bỏ kết quả *hoặc* driver-ack) trước khi mở async cho DMA;
  audit mọi `unsafe` có lý lẽ "caller blocks" (VFS: `cells/services/vfs/src/dispatch.rs:214-232`).
- **Acceptance**: 1 `httpd` cell giữ ≥2 kết nối đồng thời; kill VFS giữa lời gọi → client nhận lỗi,
  **không treo**; burst bàn phím 0 drop trên 3 arch (đúng acceptance đã ghi trong phase-07).
- **Phụ thuộc**: ADR mới (4 quyết định CQ/cancellation/lock-order). **Rủi ro**: cao nhất trong toàn roadmap
  (SAS không có MMU chặn → cancel + DMA in-flight = ghi vào cell khác; pinning registry là điều kiện tiên quyết).

### B2 — Chi phí & scale của một cell (profile "per-request server", D5)

- **Mục tiêu**: cell đủ rẻ để dùng như "process" cho request ngắn, không chỉ là service thường trú
  (đây là profile *per-request server* của Spec 19 §3 — xem §2.2).
- **Số đo hôm nay**: **9 spawn/s ≈ 111 ms/spawn** (`docs/performance-report.md:87-89`, FAIL mục tiêu ≥10/s);
  lần đo trực tiếp gần nhất dừng ở **n=8–9** (`.agents/reports/d5-cell-scale-measurement-260731.md`)
  → "1 cell / 1 request" hiện **bất khả thi**.
- **Việc — theo đúng thứ tự mà đo lường đã chứng minh** (đừng đảo): (1) **parse DTB memory node** ✅ đã land
  (`kernel/src/boot/dtb_memory.rs`); (2) share `.text`/`.rodata` giữa nhiều instance cùng image bằng refcount
  frame — khả thi vì Layer A đã làm các segment đó read-only, chạm `kernel/src/loader/elf.rs`;
  (3) stack demand-paged thay vì cấp sẵn 512 KiB; (4) **cuối cùng** mới nâng `MAX_CELLS`/`MAX_SLOTS`
  — nâng suông đã được đo là vô ích (n=8 với `MAX_CELLS=512` + 512 VA slot).
- **Gate đã định**: baseline memory/spawn latency/isolation ở **N = 64/128/256/512** trước khi đổi;
  qualification N = 1000 thêm chứng minh frame refcount sống qua spawn/reap và shared page vẫn RO.
  Instrument đã có: `MemInfo=243` + `cells/tests/bench/src/capacity-probe.rs` (chỉ bật với
  `CELLOS_INCLUDE_CAPACITY_PROBE=1`) + `tests/integration/tests/capacity-observability.rs`.
- **Bắt buộc bổ sung cho mix (§2.3)**: sweep phải chạy **khi có M heavy cell thường trú** (M=1/2/4),
  không phải N cell đồng nhất — con số N đồng nhất không dùng được cho deployment thật. Kèm theo:
  **VA budget biến đổi** (stride 32 MiB cố định hiện chặn data cell) và bằng chứng fragmentation
  không tích luỹ (run frame liền kề lớn nhất còn cấp được sau churn dài).
- **Acceptance**: đo lại N=64/128/256/512 **sau** khi DTB land (baseline này đang thiếu) và đặt ngưỡng
  spawn đo được (đề xuất ≥ 500 spawn/s ≈ 2 ms để 1 cell/request khả thi); giữ nguyên trần evidence `qemu`.
- **Phụ thuộc**: `plan-portfolio.md` đang xếp D5 "WIP-limited behind Midori" → phải mở lane riêng theo đúng promotion rule.
- **Rủi ro**: `MAX_SLOTS=512` là trần thứ hai (512 × 32 MiB VA) — nâng `MAX_CELLS` mà không xét stride là vô nghĩa;
  loader hiện copy toàn bộ ELF mỗi spawn (`kernel/src/loader/elf.rs`).

### B3 — Độ sâu network cho backend (single-node, không cần distribution)

- **Mục tiêu**: một cell backend chịu được tải thật trong LAN/loopback.
- **Việc**: DNS resolver thật (hiện stub), nâng socket budget + connection pool, IPv6 (tuỳ chọn),
  `httpd` nhiều kết nối (sau B1), backpressure ở tầng `NetRequest`, tách kênh lớn (file/body) qua grant
  thay vì 4096 B; thêm keep-alive.
- **Acceptance**: `tests/integration/tests/http-smoke.rs` mở rộng: ≥64 kết nối đồng thời, benchmark
  RPS/p99 qua `scripts/run-*-qemu.sh` hostfwd; 0 reply rơi khi bão hoà (phải là backpressure, không silent drop — Spec 17 §7).
- **Rủi ro**: `MAX_SOCKETS=18` là trần kiến trúc hiện tại; sửa nó chạm net cell + smoltcp + POSIX shim.

### B4 — Distribution ("cell-to-cell anywhere") — *chỉ khi cần multi-node*

- **Mục tiêu**: `call_remote` thật, có failure semantics rõ ràng.
- **Việc**: nối Noise session đã implement nhưng **chưa wire** (`cells/services/net-broker/src/transport/noise_session.rs`)
  vào dispatch; hoàn tất remote forwarding + remote lookup (`dispatch`/`lookup` đang thiếu);
  oracle 2 node chứng minh call tới peer rồi quay về **không fallback local**; sau đó mới tới relay
  (ADR-0008 AC-012 + protected relay identity) và `seq_no` cho N in-flight.
- **Trạng thái nguồn**: `plan-portfolio.md` ghi "partial; foundation complete, integration blocked";
  `docs/roadmap/current-focus.md` ghi remote dispatch stays disabled.
- **Acceptance**: hai QEMU guest gọi chéo service; kill node B → node A nhận `Indeterminate`/timeout
  **có phân loại** (đã có `retry_class` + deadline semantics: `Idempotent/Conditional/Never`);
  sau đó mới bàn remote supervision/failover.
- **Rủi ro**: gate không nằm ở code mà ở identity/protected persistence — đây là lane bị external-gate,
  không phải việc code thuần.

### B5 — Ops & observability (điều kiện để vận hành thật)

- **Mục tiêu**: nhìn thấy được hệ đang làm gì, giống `observer`/`recon`.
- **Việc**: expose per-cell metrics (CPU tick, mailbox depth, restart count, fault reason, generation)
  qua một service/`/proc`-like surface; crash report có N message cuối + exit reason; audit ring **ghi bền**
  (hiện volatile, `kernel/src/audit.rs`); tracing IPC theo `delivery_id`.
- **Acceptance**: test `capacity-observability` mở rộng: sau khi kill 1 cell, metric restart count +1 và
  mailbox depth của watcher quan sát được từ userspace; crash report đọc được qua VFS.
- **Rủi ro**: per-cell CPU accounting chưa tồn tại ở tầng scheduler (đây là việc kernel, không phải cell).

### B6 — State bền vững & hot upgrade (zero-downtime)

- **Mục tiêu**: nâng cấp service đang chạy mà không mất state/không rơi request.
- **Việc**: generic hoá hotswap đang chỉ chứng minh ở `hotswap-demo` (`cells/services/supervisor/src/hotswap.rs`);
  state stash hiện 64 key × 1 MiB, không persist qua reboot (`kernel/src/cell/state_stash.rs:15-22`) →
  cần API "checkpoint actor state" lên CellosFS (đã có CoW + dual superblock + `sync()` sau mỗi write);
  thêm rollback khi bản mới không ack; version negotiation cho caller (hiện chỉ là re-lookup theo `service_id`).
- **Acceptance**: `native-workload.rs` đã có mẫu (hotswap ở op 300 + VFS kill/restart ở op 600) — mở rộng
  cho service thật (`httpd`/`ai`) với 0 request rơi trong cửa sổ swap.
- **Rủi ro**: Manifest v3 + atomic publication đang blocked (`PHASE07_BLOCKED`), nên đừng hứa zero-downtime
  trước khi lane đó mở.

### B7 — Trust cho fleet (chỉ cần khi chạy code không tin cậy)

- **Mục tiêu**: chạy code bên thứ ba an toàn.
- **Việc**: `signing-required` mặc định ON + public key thật (hiện **non-default** và key non-dev còn là
  placeholder — `docs/project-roadmap.md` "Immediate Open Gates"); hoàn tất admission Tier 2 (Spec 22)
  để code unsigned/FFI bị nhốt trong domain page table riêng, thay vì bị từ chối như hôm nay
  (`docs/app-development-guide.md`: chưa có route admission công khai cho Tier 2).
- **Acceptance**: `tier2_fault_isolation.rs` (đã PASS) + negative admission tests + một lane hardware-qualified.
- **Rủi ro**: đây là lane production-gated; không nên để nó chặn B0–B3.

---

## 6. Định nghĩa "backend dùng được" (checklist nghiệm thu)

Một backend trên Cellos chỉ nên gọi là *nhẹ, bền, không giới hạn sức mạnh* khi có đủ:

1. Supervisor tree khai báo được, có backoff + strategy ngoài one_for_one (B0).
2. Một cell phục vụ N request đồng thời, hủy an toàn (B1).
3. Spawn/teardown một đơn vị request ở mức đo được và N ≥ 512 cell cô lập (B2).
4. HTTP server multi-connection + giới hạn socket/pool đủ dùng, không silent drop (B3).
5. State sống sót qua restart cell *và* reboot, có checkpoint rõ ràng (B6).
6. Quan sát được: restart count, mailbox depth, CPU/cell, crash reason (B5).
7. Nâng cấp không downtime cho service thật (B6).
8. Payload lớn đi đường grant, không bị chặn ở 4096 B (B3).
9. Failure semantics rõ: timeout vs indeterminate vs backpressure (đã có hợp đồng C2C, cần áp cho nội bộ).
10. Mọi kết quả trên chỉ được tuyên bố ở đúng trần evidence (`qemu` ≠ `physical` ≠ `production`).

Điểm 1–4 là điều kiện *tối thiểu* để bắt đầu viết backend thật; 5–8 là điều kiện *vận hành*;
9–10 là điều kiện *nói thật về hệ thống*.

---

## 7. Việc KHÔNG nên làm

- **Mailbox vô hạn như BEAM** — bounded + backpressure hiện tại ngăn một consumer chậm làm nổ RAM toàn hệ.
- **Bắt chước `!` không-block** — sẽ phá `TryAgain`/`Backpressure` và làm mất tín hiệu quá tải.
- **Hot-code-load theo module** — Rust không có cơ chế đó; hotswap cấp cell + state handoff là đường đúng.
- **Kéo POSIX/Linux vào Tier 1** — ADR-0015 routing: C/FFI/unsigned → Tier 2; Linux app → Tier 3.
- **Nâng `MAX_CELLS` mà không làm B2** — sẽ đổi trần này lấy trần khác (`MAX_SLOTS`, stack 512 KiB, ELF copy/spawn).
- **Hứa zero-downtime trước khi Manifest v3/atomic publication mở**.

---

## 8. ADR cần chốt trước khi code (theo Law 1 nếu chạm `libs/api/`)

1. **Cancellation semantics** (chờ-rồi-bỏ *hoặc* driver-ack) — điều kiện tiên quyết của B1, cùng 3 quyết định
   CQ còn lại trong phase-07 (source_id, CQ overflow, lock order khi append).
2. **Monitor mở cho mọi cell** hay chỉ `SpawnCap` holder (mở rộng `NotifyOnExit` thành monitor set) — nếu mở, là Law 1.
3. **Namespace tên**: giữ `u16` service id + mở rộng sang dynamic name registry, hay dùng VFS path (`/svc/...`) như gợi ý trong phase-00 C2C.
4. **Đơn vị application/package** (nếu muốn DX kiểu OTP release).
5. **Ngưỡng spawn latency + memory/cell** cho profile per-request (B2) — phải là số đo, không phải cảm giác.

---

## 9. Nguồn chính đã dùng

| Chủ đề | File |
|---|---|
| Hợp đồng IPC hiện hành | `docs/specs/17-ipc-wire-contract.md` |
| Never-die, 6 trục, đối chiếu Erlang/OTP | `docs/specs/12-reliability.md` |
| Isolation layers + profile per-request (D5) | `docs/specs/19-hardware-isolation-layers.md:87-134` |
| Lớp cell siêu nhẹ: đo năng lực + thứ tự việc | `.agents/reports/d5-cell-scale-measurement-260731.md`, `.agents/reports/d5-cell-scale-profile-ruling-analysis-260801.md`, `.agents/260801-d1b-d3-d5-closure/phase-04-cell-scale-profile.md` |
| Kiến trúc 3 tier | `docs/decisions/0015-dual-mode-hybrid-architecture.md` |
| Reactor/async còn thiếu gì | `.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md` |
| C2C anywhere (envelope, gate, trạng thái) | `.agents/260624-cell-to-cell-anywhere/plan.md` |
| Lane & promotion rule | `.agents/plan-portfolio.md`, `docs/roadmap/current-focus.md` |
| Số đo hiệu năng + trần evidence | `docs/performance-report.md` |
| Supervisor thật | `cells/tools/init/src/supervisor.rs`, `cells/tools/init/src/service_table.rs` |
| Hotswap/state | `kernel/src/cell/hotswap.rs`, `cells/services/supervisor/src/hotswap.rs`, `tests/integration/tests/hotswap-smoke.rs` |
