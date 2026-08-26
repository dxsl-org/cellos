# Phase 02 — POSIX Shim Extensions: pthread_mutex + mmap + ioctl + libm

**Track**: A (Tier 1b native FFI, but mmap/ioctl also needed for Track B)  
**Status**: 📋 PLANNED  
**Priority**: HIGH — gates Phase 04; mmap/ioctl gates Phase 05  
**Effort**: ~1.5 weeks  
**Depends on**: nothing (parallel-eligible with 01, 03)  
**Law 1**: `libs/api/src/posix.rs` — 1× confirmation required (additive shims, no interface change)

---

## Context Links
- `libs/api/src/posix.rs` (731 lines after Phase 01 of previous plan — the file to modify)
- `libs/api/src/syscall.rs` — `ViSyscall` enum + `raw_syscall` ABI
- `libs/ostd/src/sync.rs` or `spin::Mutex` — ViCell Spinlock
- `kernel/src/task/syscall.rs` — dispatch for any new syscalls (ioctl may need kernel support)

## Overview

`librknnrt.so` requires six symbol groups beyond what Phase G (Tier 1b shims) provided. This phase adds them to `libs/api/src/posix.rs`:

| Symbol group | Usage in RKNN | Implementation |
|---|---|---|
| `pthread_mutex_init/lock/unlock/destroy/trylock` | Context init, output buffer sync | Map to `Spinlock<()>` (8-slot table, same pattern as `SOCK_CAPS`) |
| `pthread_create/join/detach` | Async pipeline worker (avoid `RKNN_FLAG_ASYNC_MASK` to defer this) | Stub: `pthread_create` → `sys_spawn(trampoline)`; `join` → spin-poll exit flag |
| `pthread_cond_wait/signal/broadcast` | Worker coordination | Stub as no-ops (valid only in sync-inference mode; add real futex impl if needed later) |
| `mmap(MAP_ANON)` / `munmap` | Internal scratch buffers inside SDK | `mmap(MAP_ANON)` → `sys_grant_alloc` + return identity-mapped VA; `munmap` → `sys_grant_free` |
| `ioctl(fd, request, ...)` | DRM GEM allocation + RKNPU_ACTION ioctls to `/dev/rknpu` | Stub: forward to a future `/dev/rknpu` driver cell via IPC (return ENOSYS initially) |
| `sqrtf / expf / logf / fabsf / floorf / ceilf / fminf / fmaxf / roundf` | Dequantization math in output processing | Software implementations (compiler-rt `libm` or picolibc equivalents) |

**Note:** `pthread_mutex_*` overlaps conceptually with Phase 01's `pthread_shim.rs` in `ostd/dynlink`. The split is intentional: Phase 01's pthread shim handles symbols resolved via the runtime linker (for `.so` blob symbol resolution). Phase 02's symbols are exported from `posix.rs` as C ABI functions directly callable from linked C code or passed in the POSIX shim table.

---

## Key Insights

### pthread_mutex as Spinlock table
`SOCK_CAPS` in posix.rs demonstrates the pattern: a fixed-size static array indexed by an opaque handle. For mutex handles:
- 16-slot table (`static MUTEX_TABLE: [Spinlock<()>; 16]`)
- `pthread_mutex_t` is typically `union { char __size[40]; long __align; }` on aarch64 — we store the table index in the first 4 bytes
- `pthread_mutex_init` → find free slot, store index in `__size[0..4]`
- `pthread_mutex_lock` → read index, `MUTEX_TABLE[idx].lock()`

### mmap implementation
RKNN SDK uses `mmap(NULL, size, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)` for scratch buffers. Map this to `sys_grant_alloc(pages)` which zeroes memory and returns a physical address that is also the virtual address in SAS. The returned VA is valid for direct pointer use. `MAP_SHARED` with an `fd` (for DRM GEM) is handled differently — see ioctl section.

### ioctl stub strategy
For Phase 02, `ioctl` returns -1 / ENOSYS for all commands. This causes `rknn_init` to fail with `RKNN_ERR_DEVICE_UNAVAILABLE` — which is the correct behavior until Phase 04/05 wires up a real NPU driver cell. Track B (Phase 05) will replace this stub with a real implementation that opens the RKNPU device node and routes ioctls through the NPU driver cell.

### libm — software implementations
`libm` symbols (`sqrtf`, `expf`, `logf`, etc.) can be provided by:
- `compiler-rt` built-ins (already available in the Rust toolchain's `compiler-builtins` crate)
- Or inline Rust implementations in `posix.rs` (4–8 lines each, Taylor series / bit-manipulation tricks)

Recommended: thin wrappers that call `f32::sqrt()`, `f32::exp()`, `f32::ln()` — these compile to the corresponding hardware float instructions on aarch64 with no software overhead.

---

## Requirements

### Functional
- FR1: `pthread_mutex_init/lock/unlock/destroy/trylock` with 16-slot table
- FR2: `pthread_create` → `sys_spawn` trampoline; `pthread_join` → spin-poll `AtomicBool` exit flag
- FR3: `pthread_cond_wait/signal/broadcast` → no-op stubs (safe for sync mode)
- FR4: `mmap(MAP_ANONYMOUS)` → `sys_grant_alloc` + identity VA return
- FR5: `mmap(MAP_SHARED, fd)` → return grant VA if fd is a known grant handle (for DRM GEM path, Phase 04/05)
- FR6: `munmap` → `sys_grant_free` (looks up by VA in grant table)
- FR7: `ioctl` → return -1 / ENOSYS (stub); architecture for future routing to driver cell
- FR8: `sqrtf/expf/logf/fabsf/floorf/ceilf/fminf/fmaxf/roundf` — hardware-backed via `f32::*`

### Non-functional
- NF1: All new functions in `libs/api/src/posix.rs` under `#![cfg(any(target_arch = "aarch64", target_arch = "riscv64", ...))]`
- NF2: `pthread_mutex_t` layout assumption documented with a `// SAFETY:` comment on aarch64 struct size
- NF3: `cargo check -p api --target aarch64-unknown-none` clean after this phase

---

## Related Code Files

### Modify
- `libs/api/src/posix.rs` — add all new `#[no_mangle] pub unsafe extern "C"` functions

---

## Implementation Steps

1. Add `MUTEX_TABLE: [Spinlock<()>; 16]` + `MUTEX_INIT: AtomicU8` free-slot bitmap
2. Implement `pthread_mutex_init`, `pthread_mutex_lock`, `pthread_mutex_unlock`, `pthread_mutex_destroy`, `pthread_mutex_trylock`
3. Add `THREAD_TABLE` (8 slots): `index`, `exit_flag: AtomicBool`, `stack_base`
4. Implement `pthread_create` → `sys_spawn(ostd_pthread_entry)` where `ostd_pthread_entry(arg)` calls the function pointer then sets `exit_flag`
5. Implement `pthread_join` → spin-poll `exit_flag` (max 10 000 yields before timeout)
6. Implement `pthread_cond_wait/signal/broadcast` → no-ops returning 0
7. Add `mmap` handling: `MAP_ANONYMOUS` path → `sys_grant_alloc(size / PAGE_SIZE)` → return identity VA; document that `MAP_SHARED | MAP_FILE` path returns ENOTSUP until Phase 04
8. Add `munmap` → `sys_grant_free` (linear scan of grant VA range)
9. Add `ioctl` stub → `-ENOSYS` for all commands
10. Add libm: `sqrtf/expf/logf/fabsf/floorf/ceilf/fminf/fmaxf/roundf` as thin `f32::*` wrappers
11. `cargo check -p api --target riscv64gc-unknown-none-elf` + `--target aarch64-unknown-none` — both must pass

---

## Todo

- [ ] Add `MUTEX_TABLE` + `MUTEX_INIT` statics
- [ ] Implement `pthread_mutex_init/lock/unlock/destroy/trylock`
- [ ] Add `THREAD_TABLE` + exit-flag array
- [ ] Implement `pthread_create` → `sys_spawn` trampoline
- [ ] Implement `pthread_join` + `pthread_detach`
- [ ] Implement `pthread_cond_*` no-op stubs
- [ ] Implement `mmap` (MAP_ANONYMOUS path)
- [ ] Implement `munmap`
- [ ] Implement `ioctl` stub
- [ ] Add libm math functions
- [ ] `cargo check` on both targets passes

---

## Success Criteria

1. `cargo check -p api --target aarch64-unknown-none` clean
2. `cargo check -p api --target riscv64gc-unknown-none-elf` clean (no regression)
3. All new functions compile to valid aarch64 instructions (verify with `objdump` on a test build)
4. A C test that calls `pthread_mutex_lock/unlock` in a loop compiles and links against the shim

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `pthread_mutex_t` struct size differs across aarch64 libc variants | Use the largest known size (40 bytes); assert `size_of` in a debug assertion |
| `pthread_create` spawned thread outlives inference cell | `pthread_detach` marks thread as non-joinable; watchdog/RAII will reap orphaned tasks |
| `mmap` returning grant VA confuses SDK if it expects system-page-aligned addresses | `sys_grant_alloc` returns page-aligned VAs — should be fine |
