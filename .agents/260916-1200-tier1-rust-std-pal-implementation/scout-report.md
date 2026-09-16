# Scout Report: Tier 1 Rust `std` PAL In-Tree Implementation

## 1. Context and Objective
- Target: Implement an in-tree Rust `std` Platform Abstraction Layer (PAL) for CellOS Tier 1 Real-time SAS.
- Upstream Dependency: `PAL-IMPLEMENTATION-CHECKPOINT` unblocked on 2026-09-16 following Umbrella Phase 03 baseline approval and ledger implementation transition.
- Pinned Compiler: `nightly-2026-05-01`, rustc `1.97.0-nightly (f53b654a8)`.
- Approved Strategy: Strategy B from `RUST-STD-COMPILER-STRATEGY-001` — content-addressed source-overlay patch against private matching Rust checkout, without vendoring the full Rust tree in the repository.

## 2. Key Codebase Findings and Existing Substrates
1. **Toolchain & Targets**:
   - `rust-toolchain.toml`: Pins `channel = "nightly-2026-05-01"` with components `["rust-src", "rustfmt", "clippy", "llvm-tools-preview"]`.
   - Active architectures: `riscv64gc-unknown-none-elf`, `aarch64-unknown-none`, `x86_64-unknown-none`.
   - In-tree CellOS target specification will define `riscv64gc-unknown-cellos`, `aarch64-unknown-cellos`, and `x86_64-unknown-cellos`.
2. **Upstream Pinned `std::sys::pal`**:
   - `library/std/src/sys/pal/mod.rs`: Contains top-level `cfg_select!` mapping `target_os` to internal PAL submodules.
   - Adding `target_os = "cellos" => { mod cellos; pub use self::cellos::*; }` enables direct routing to the in-tree PAL.
   - Minimal reference PALs in tree: `hermit`, `xous`, `zkvm`, `unsupported`.
3. **CellOS System Calls (`libs/ostd/src/syscall.rs`)**:
   - `sys_get_time()` / `sys_get_time_ms()`: Monotonic time via `ViSyscall::GetTime`.
   - `sys_yield()`: Scheduler yield via `ViSyscall::Yield`.
   - `sys_exit(code)`: Task termination via `ViSyscall::Exit`.
   - `sys_log(msg)`: Admitted logging via `ViSyscall::Log`.
   - `sys_get_random(buf)`: Secure entropy via `ViSyscall::GetRandom` (validated under `PAL-019` and `PAL-031`).
4. **Memory Allocation (`libs/ostd/src/heap.rs`)**:
   - Per-cell freeing heap allocator implementing `core::alloc::GlobalAlloc`.
   - `PAL-007` contract binds `std::alloc::System` to this per-cell allocator without cross-cell aliasing.
5. **Security & Boundary Invariants**:
   - Abort-only panic strategy: `panic=abort`, unwinding is `Unsupported`.
   - Single-task model: `available_parallelism = 1`, OS thread spawning is `Unsupported`.
   - No ambient filesystem or network: `fs` and `net` operations return explicit `ErrorKind::Unsupported` unless backed by explicit held capabilities.
   - `PAL-019`: Governed release tuple omits `dev-weak-rng`, zero/error fail-closed.
   - `PAL-031`: Bounded caller-owned writable validation on memory buffers before read/write.

## 3. Implementation Approach and Boundaries
- The implementation does NOT vendor a complete Rust compiler or fork.
- It delivers:
  1. Target JSON files under `targets/`.
  2. The patch file `patches/rust-std-cellos.patch` for `rust-src`.
  3. The sysroot builder script `scripts/build-cellos-sysroot.sh`.
  4. The implementation of `sys/pal/cellos/` supporting the 36 scoped hooks.
  5. Workload parity benchmarks comparing `no_std` vs `std` on QEMU.
