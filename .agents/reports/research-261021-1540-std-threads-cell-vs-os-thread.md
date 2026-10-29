# Research: Triển khai `std::thread`/`std::sync` trên Cellos (SAS/LBI) — Cell có thay được OS thread không?

**Ngày:** 2026-10-21 · **Loại:** arch · **Độ sâu:** standard (5 web + 1 scout nội bộ)
**Câu hỏi:** Các hàm `std` về thread/đồng bộ (ngầm định mô hình Linux) nên được triển khai thế nào trên Cellos? Cell + C2C có thể thay OS thread không?

## Verdict

**Không — Cell không thay được OS thread; nhưng Cellos đã có đủ substrate kernel để triển khai
`std::thread` + futex-based `std::sync` theo đúng mẫu Hermit, chỉ còn thiếu phần PAL và TLS runtime.**

- Cell là đơn vị *authority / recovery / CSpace / quota* (Spec 25 `docs/specs/25-cell-kernel-authority-contract.md:14-16,47-50,77-80`). `std::thread::spawn` đòi hỏi **chia sẻ heap & stack-borrowed data** (`thread::scope`, `Arc<Mutex<T>>`); hai Cell không chia sẻ heap, nên mọi `std::sync` primitive dựa trên atomic chung sẽ vô nghĩa giữa hai Cell. Thay Cell cho thread là đổi ngữ nghĩa ngôn ngữ, không phải triển khai std.
- Kernel **đã có** intra-Cell thread: `Spawn=5` tạo task cùng CellId, kế thừa CapSet/allowlist/domain (`kernel/src/task/scheduler.rs:929-968`); `FutexWait=17`/`FutexWake=18` với key `(address-space, generation, addr)` và compare-and-park dưới `SCHEDULER` (`kernel/src/task/futex.rs:1-30`); `Wait=8` join; `SetTlsBase=9`. `libs/ostd/src/task.rs:3-7` nói thẳng: *"A 'thread' in Cellos is another task of the same Cell"*. `libs/port-platform/cellos_pthread.c` đã dùng đúng bộ này cho pthread C (mutex = CAS + futex, condvar = seq word).
- Đây chính là mô hình Hermit: `sys/thread/hermit.rs` = `hermit_abi::spawn2/join/yield_now`; `sys/sync/futex/hermit.rs` = `hermit_abi::futex_wait/futex_wake`; `sys/sync/mutex/mod.rs` chọn `futex::Mutex` cho `target_os = "hermit"`. Motor OS (Tier‑3 target `x86_64-unknown-motor`, 2025) làm y hệt qua `moto_rt::thread::spawn/join` + `moto_rt::futex`, "no ELF TLS; TLS provided by the runtime" (motor-os.org/rust.html).
- Phần đúng của ý tưởng "Cell thay thread" là **mô hình Midori**: process siêu nhẹ, single-threaded event loop, không blocking, message passing — đó là ADR‑0021 (actor/supervisor userspace, "no intra-Cell concurrency in B0") và C2C. Nó là **tầng trên** `std` (như tokio/actix trên Linux), không phải cách triển khai `std::thread`.

## Hiện trạng Cellos (đã xác minh)

| Thành phần | Trạng thái | Cite |
|---|---|---|
| Kernel thread cùng Cell (`Spawn=5`) | Có; cap 32 task/Cell (fragmentation bound, `STACK_PAGES+1` contiguous) | `scheduler.rs:9-22,929-968` |
| Futex wait/wake | Có; timeout tick; không REQUEUE/PI | `kernel/src/task/futex.rs` |
| Join (`Wait=8`) | Có | `syscall.rs:5411-5455` |
| TLS register (`SetTlsBase=9`; FS_BASE/TPIDR_EL0/tp) | Có, kernel reinstall khi resume | `kernel/src/task/tls.rs` |
| ELF `PT_TLS` trong loader | **Không** | `docs/guides/tier1b-c-zig.md:332-335` |
| `patches/rust-std-cellos.patch` thread | `Thread` = `unsupported`, `available_parallelism = 1`, `thread_local` = `no_threads`, không có futex/mutex module | patch `:443-493, 747-787` |
| `ostd::task::spawn` | Có, nhưng "nothing calls it" | `libs/ostd/src/task.rs:33-48`, `scheduler.rs:13` |
| pthread C subset | Có, dùng Spawn+futex+Wait thật | `libs/port-platform/cellos_pthread.c` |
| mlibc vicell futex | yield-spin, wake no-op (không dùng được) | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:58-86` |
| `WaitCompletion=242` | chỉ NET_RX/TIMER, không phải park/unpark | `kernel/src/task/completion_wait.rs:78-105` |
| ADR‑0021 actor | userspace mailbox, B0 không có intra-Cell concurrency | `docs/decisions/0021-*:50-60` |

## So sánh phương án

| Tiêu chí | A. Thread = kernel task cùng Cell (Hermit/Motor) | B. Thread = Cell riêng + C2C | C. Green/M:N trong user (fiber) |
|---|---|---|---|
| Ngữ nghĩa `std` (shared heap, `thread::scope`, `Arc<Mutex>`) | ✅ đúng | ❌ không có heap chung; `Send + 'static` closure không serialize được | ✅ (nhưng blocking syscall chặn cả executor) |
| Primitive kernel cần thêm | 0 (Spawn/Futex/Wait/SetTlsBase có sẵn) | Cần serialize closure + grant heap → thực chất là RPC | 0, nhưng cần `WaitCompletion` tổng quát để không block |
| Chi phí spawn | 2 stack + clone metadata (`scheduler.rs:1025-1116`) | ELF load, VA slot 32 MiB, CSpace, quota 16 MiB (ADR‑0018:85-95) | ~µs |
| Cô lập lỗi | panic = chết cả Cell (đúng với Rust: abort) | ✅ Cell recovery | ❌ |
| Song song thật trên nhiều hart | ✅ per-hart ready queue | ✅ | ❌ (trừ khi M:N trên nhiều task) |
| Rủi ro | 32 task/Cell; contiguous stack frames; cần PT_TLS | ABI/ngữ nghĩa khác hẳn std | Stack switching unsafe, không có `WaitCompletion` tổng quát |

**Khuyến nghị: A cho `std`; B giữ nguyên vai trò là mô hình ứng dụng (actor/C2C) tầng trên.**

## Kế hoạch triển khai (khi được mở — hiện bị chặn sau cellbox-cli, xem `.agents/TODO.md`)

1. **PAL `sys/thread/cellos.rs`** theo mẫu `sys/thread/hermit.rs`: `Thread::new` = Box `ThreadInit` → `sys_spawn(thread_start, ptr)`; `thread_start` chạy `init.init()()`, `thread_local::destructors::run()`, `rt::thread_cleanup()`, `sys_exit(0)`. `join` = `Wait=8`. `available_parallelism` = số hart (cần syscall/`MemInfo`-style query; hiện trả 1). `sleep` = `WaitCompletion(TIMER)` hoặc `RecvTimeout`.
2. **`sys/sync/futex/cellos.rs`**: `futex_wait(&AtomicU32, expected, timeout)` → `FutexWait=17` (đổi `Duration`→tick, 0 = vô hạn; map `ValueMismatch`→`true`, `TimedOut`→`false`); `futex_wake` → `FutexWake=18(count=1)`, `wake_all` → `count=0`. Thêm `target_os = "cellos"` vào `sys/sync/mutex|condvar|rwlock|once/mod.rs` nhánh futex. **Không viết Mutex riêng** — std đã có futex-based Mutex/Condvar/RwLock/Once/thread_parking.
3. **TLS**: loader expose `PT_TLS` (size, align, init image, tbss) qua launch profile; userspace runtime cấp block per-thread (TLS block + TCB self-pointer cho x86_64 variant II) và gọi `SetTlsBase` làm việc đầu tiên trong `thread_start`; set `target_thread_local = true` trong `targets/*.json` → std dùng `thread_local/native`. Tránh con đường `os` key-based (cần `pthread_key` tương đương) — Hermit/Motor đều native/runtime.
4. **Kernel**: nâng/giải thích `MAX_THREADS_PER_CELL=32` (tokio default worker = hart count, nên 32 đủ cho giai đoạn đầu); stack guard đã có; cân nhắc demand-paged stack để bỏ ràng buộc contiguous (đã ghi ở risk register D5).
5. **Bằng chứng**: `tests/rust-std-promotion` thêm witness: `thread::scope` + `Arc<Mutex<Vec>>` + `Condvar` + `thread_local!` destructor + 32 create/join vòng; so sánh với `cells/tests/c-pthread/src/witness.c`.
6. **Hợp đồng tài liệu**: ADR mới "std threads = intra-Cell kernel tasks; Cells are not threads"; cập nhật `docs/roadmap/runtime-and-platform-tracks.md:131-134` (bỏ invariant `available_parallelism = 1`).

## Khi nào dùng Cell thay vì thread (đúng Midori)

- Workload cần **fault isolation / restart độc lập / quota riêng / authority khác** → Cell riêng, giao tiếp C2C (`async_ipc`, 64 op/owner, lifecycle Queued→Dispatched→Terminal, `docs/specs/20-*:149-163`).
- Workload là **compute song song trên dữ liệu chung** (rayon, tokio worker, parser) → thread cùng Cell.
- Midori chỉ đạt "process = unit of concurrency" nhờ linked stacks + compiler-known async + không blocking (Duffy, *Asynchronous Everything*). Cellos không có compiler hỗ trợ đó; áp mô hình này lên `std` sẽ phá `std::sync` và mọi crate dùng nó.

## Pitfalls

- `FutexWait` key gồm generation: wait/wake qua grant chung giữa 2 Cell Tier‑2 **không** hoạt động (by design, `futex.rs:10-12`) — `std::sync` chỉ intra-Cell.
- `panic = abort` trong target JSON: `JoinHandle::join()` trả `Err` không bao giờ xảy ra; panic một thread giết cả Cell. Chấp nhận (Motor cũng vậy) nhưng cần ghi rõ.
- `thread::sleep` hiện không có syscall sleep thuần; dùng TIMER completion, tránh busy-yield.
- Không dùng mlibc `vicell` futex (yield-spin) cho Rust; chỉ dùng cho C legacy.

## Nguồn

- Hermit PAL: `library/std/src/sys/thread/hermit.rs`, `sys/sync/futex/hermit.rs`, `sys/sync/mutex/mod.rs` (rust-lang/rust main, đọc 2026-10-21)
- Motor OS: https://motor-os.org/rust.html ; `library/std/src/sys/thread/motor.rs`
- Midori: Joe Duffy, *Asynchronous Everything* (2015) https://joeduffyblog.com/2015/11/19/asynchronous-everything
- Theseus OSDI'20 (SAS/SPL, task = thread trong một address space) https://www.usenix.org/conference/osdi20/presentation/boos
- Nội bộ: scout `agent://CellosStdThreadScout`; files cite ở trên.

## Câu hỏi mở

- `available_parallelism` cần syscall trả số hart (hiện chưa có hay dùng `MemInfo`?).
- Có cho phép Tier‑2 (paged domain) Cell spawn thread không — scheduler map stack vào domain hiện có (`scheduler.rs:1025-1116`) đã đủ chưa, PKU state clone thế nào?
- `thread::scope` cần join tin cậy khi worker chết do fault: `Wait=8` có trả về khi task bị kernel kill không?
