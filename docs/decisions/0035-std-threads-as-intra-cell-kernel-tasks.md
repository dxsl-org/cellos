# ADR-0035 — Rust std::thread and std::sync as intra-Cell kernel tasks

**Date:** 2026-11-08
**Status:** Accepted
**Amends:** [ADR-0018](0018-cell-native-portability-and-runtime-profiles.md) §2.2 (promotes Class B OS threads from L2/unsupported to L1 with native PAL and TLS runtime).
**Decider:** Cellos maintainer (owner).

## Context

Cellos models protection, fault containment, resource quotas, and authorization around the **Cell** ([Spec 25](../specs/25-cell-kernel-authority-contract.md)).
Previously, Cellos lacked a standard threading and synchronization runtime in `std`:
- `Thread` was stubbed as `unsupported` in the `patches/rust-std-cellos.patch`.
- `available_parallelism` was pinned to 1.
- `thread_local` used the `no_threads` fallback.
- `std::sync` synchronization primitives either spun or lacked kernel futex integration.

A key architectural question was whether Cells could substitute for OS threads. As analyzed in research report `reports/research-261021-1540-std-threads-cell-vs-os-thread.md`:
- `std::thread::spawn` and `std::thread::scope` require shared heap, address space, and borrowed stack lifetimes (`Arc<Mutex<T>>`).
- Cells do not share heaps or address spaces; inter-Cell shared atomic mutation is non-viable and contradicts Cell recovery boundaries.
- Substituting Cells for threads breaks standard library semantics and language contracts.

Instead, the Cellos kernel substrate already possesses the primitives required for intra-Cell multi-task concurrency:
- `Syscall::Spawn = 5` spawns a kernel task within the same Cell, inheriting CellId, domain, PKU keys, and capabilities.
- `Syscall::FutexWait = 17` and `Syscall::FutexWake = 18` provide domain-scoped compare-and-park synchronization.
- `Syscall::Wait = 8` provides terminal join semantics.
- `Syscall::SetTlsBase = 9` installs architecture-native user thread pointers (`FS_BASE`, `TPIDR_EL0`, `tp`).
- `ViSyscall::GetCpuCount = 263` exposes the live online hardware execution units (harts/CPUs).

## Decision

1. **Threads are intra-Cell kernel tasks; Cells are not threads.**
   - A thread spawned by `std::thread::spawn` or `std::thread::scope` runs as a distinct schedulable kernel task sharing the Cell's heap, address space, PKU domain, and resource quota.
   - Cells remain the sole unit of recovery, capability authority, and IPC. Fault isolation remains at the Cell boundary: a fatal fault or panic aborts the entire Cell generation.
   - Inter-Cell communication remains strictly message-based via C2C IPC (`async_ipc`). `std::sync` primitives (`Mutex`, `Condvar`, `RwLock`, `Once`) are strictly intra-Cell.

2. **Standard Library PAL (`library/std/src/sys/pal/cellos/thread.rs`)**:
   - `Thread::new`: Converts `Box<ThreadInit>` to a raw pointer, invokes `sys_spawn(5)` to schedule the child task.
   - `thread_start`: Trampoline sets up per-thread TLS, executes `init.init()()`, runs TLS destructors (`destructors::run()`), performs `rt::thread_cleanup()`, frees the TLS block, and exits via `sys_exit(0)` (`Syscall::Exit = 60`).
   - `Thread::join`: Invokes `sys_wait(8)`. The scheduler zombie reaper releases and wakes joiners upon task quiescence with exit status.
   - `sleep(dur)`: Uses `futex_wait` on a private atomic word with timeout tick conversion, eliminating busy-wait spin.
   - `available_parallelism`: Returns the live online hart count from `ViSyscall::GetCpuCount`.

3. **Native Thread-Local Storage (TLS)**:
   - Target specifications (`targets/*.json`) set `"has-thread-local": true`, enabling `target_thread_local` in rustc and selecting `std::sys::thread_local::native`.
   - Every thread receives a dedicated TLS block configured before executing user code:
     - **x86_64**: Variant II TLS — `FS_BASE` points to the TCB; offset 0 contains the self-pointer (`*tcb = tcb`), enabling `%fs:0` and negative offsets for static thread-local variables.
     - **AArch64 / RISC-V**: Variant I TLS — `TPIDR_EL0` / `tp` points to the thread control/data block.
   - Main thread TLS is established during runtime startup in `sys::init`. Child thread TLS is allocated and established at the beginning of `thread_start` and reclaimed on thread exit.

4. **Task Bounds and Invariants**:
   - The scheduler bounds concurrency per Cell to `MAX_THREADS_PER_CELL = 32`.
   - Stacks are validated and mapped into the Cell's domain at spawn time; quota charges are exact and refunded upon task retirement.
   - Wait on an exited or reaped task completes without hang; worker thread exit terminates only that task and does not trigger whole-Cell teardown.

## Consequences

- Full Rust `std::thread` and `std::sync` (including `thread::scope`, `Arc<Mutex<T>>`, `Condvar`, `RwLock`, `Once`, `thread_local!`) are supported natively across `x86_64`, `aarch64`, and `riscv64gc`.
- The historical assumption `available_parallelism = 1` is retired.
- Multi-threaded Rust workloads (e.g., ray-tracing, parallel parsers, compute pipelines) run natively inside Cellos Cells without requiring emulated Linux shims.
