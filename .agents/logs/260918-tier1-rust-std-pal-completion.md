# 2026-09-18 — Tier 1 pure-Rust std PAL and custom targets complete

## Why
Tier 1 application developers needed standard library facilities (`std::alloc`, `std::env::args`, `std::time`, `std::thread::yield_now`, collections, serde) without relying on C/mlibc or compromising the Single Address Space (SAS) security boundary and Language-Based Isolation (LBI). The initial feasibility spike verified the hook inventory but left a 1 MiB bump allocator (which could not deallocate, leading to OOM on cyclic workloads) and an empty command-line argument stub.

## What landed
1. **Freeing Heap Allocator with Coalescing (`library/std/src/sys/pal/cellos/alloc.rs`)**:
   - 4 MiB heap arena with 16-byte alignment (`#[repr(align(16))]`).
   - Boundary-tag block header (`size`, `is_free`).
   - Dynamic block splitting on allocation when excess size >= 32 bytes.
   - Linear sweep contiguous block coalescing on `dealloc`, ensuring zero leaks during cyclic allocation/deallocation patterns.
   - Handled zero-sized allocations according to Rust allocator contract and returned `null_mut()` on exhaustion.

2. **Command-Line Arguments (`library/std/src/sys/args/cellos.rs`)**:
   - Restores command line via `ViSyscall::StateRestore` (411) from staging key `ARGV_STASH_KEY` (`0x0061_7267_7600_0000`).
   - Parses arguments into `Vec<OsString>` with `argv[0] = "cell"` fallback when empty.
   - Wires into `std::env::args()` and `std::env::args_os()`.

3. **Multi-Arch Target Specifications**:
   - `targets/riscv64gc-unknown-cellos.json`
   - `targets/aarch64-unknown-cellos.json`
   - `targets/x86_64-unknown-cellos.json`
   - All standardizing on `"relocation-model": "pic"`.

4. **Replicable Source Overlay Patch**:
   - Updated `patches/rust-std-cellos.patch` capturing the full set of additions and modifications against upstream `rust-src` nightly-2026-05-01.

5. **Validation and Evidence**:
   - `scripts/run-std-smoke-qemu.sh` boots in QEMU and passes 1000-cycle (2 KB/cycle) allocate/free stress test, monotonic timing, yield, parallelism count, argv parsing, serialization, and fail-closed checks.
   - `scripts/run-std-parity-benchmark.sh` passes across riscv64, aarch64, and x86_64 with p99 regression <= 5%.
   - `python3 scripts/cellos-sign --check` passes F1/F5 policy checks (90 crates, 602 files).
   - `bash scripts/check-baseline.sh` passes clean formatting and linting.

## Next Steps
- Production hardware floor qualification and owner admission consent gating for production releases.
- Address `CELLOS-LOADER-SIG-001` metadata relocation verification.
- Optional future expansion: capability-backed `std::fs` and `std::net` via VFS and Net broker IPC.
