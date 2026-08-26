# Phase 01 — `ostd` ELF Runtime Linker for Companion `.so` Bundles

**Track**: A (Tier 1b native FFI)  
**Status**: 📋 PLANNED  
**Priority**: HIGH — gates Phase 04  
**Effort**: ~3 weeks  
**Depends on**: nothing (parallel-eligible with 02, 03)

---

## Context Links
- `kernel/src/loader/` — existing ELF cell loader (maps PT_LOAD segments, applies relocations)
- `libs/ostd/src/startup.rs` — cell startup code (called before `main()`)
- `libs/ostd/src/lib.rs` — ostd crate root
- Research: `.so`-only librknnrt.so, DRM/ioctl deps, full `.so` deps list in plan.md

## Overview

`librknnrt.so` (and its companions `libstdc++.so.6`, `libgcc_s.so.1`, `libm.so.6`) ship as ELF shared objects — precompiled, position-independent, with PLT/GOT-based symbol dispatch. ViCell cells are statically linked ELFs with no `PT_INTERP`; there is no `ld.so` on ViCell.

This phase adds a **userspace runtime linker** to `ostd` that:
1. Accepts `.so` blobs embedded in the cell's binary at build time (`include_bytes!`)
2. Maps each blob into the SAS at a build-time-assigned fixed VA
3. Applies `R_AARCH64_JUMP_SLOT`, `R_AARCH64_GLOB_DAT`, `R_AARCH64_RELATIVE` relocations
4. Resolves undefined symbols against a **symbol table** built from:
   - ViCell POSIX shims (`libs/api/src/posix.rs`)  
   - `pthread_*` shim (Phase 02)  
   - `libstdc++`/`libgcc_s`/`libm` companion blobs  
5. Runs `.init_array` constructors (required by `libstdc++`)

**No kernel changes required.** The runtime linker runs entirely in the cell's address space during `ostd::startup` (before `main()`). The kernel sees a single valid ViCell ELF.

---

## Key Insights

### Why userspace, not kernel
The kernel loader already applies static cell relocations. Adding dynamic `.so` support to the kernel would bloat the kernel and couple it to vendor library ABI details. A userspace linker in `ostd::startup` is consistent with ViCell's "kernel stays small" principle and mirrors how seL4 and Singularity handle native-library loading.

### VA assignment for embedded .so
All cells share one SAS. VA ranges for `.so` blobs must be disjoint from existing cell bases. New VA range for `.so` blob mapping: **0x7C000000–0xBC000000** (1 GiB window, above all current cell VAs). Each blob gets a sub-range assigned at cell build time via build.rs constants.

### Symbol resolution priority
1. Explicit POSIX shim table (symbols from `libs/api/src/posix.rs` + Phase 02 additions)
2. Loaded companion `.so` blobs (inter-library symbols like `libstdc++` pulling from `libgcc_s`)
3. Undefined → log warning and return `NULL` (for optional symbols like `dlopen`)

### pthread_create first approach
`librknnrt.so` likely calls `pthread_create` on `rknn_init`. Strategy: implement `pthread_create` → `sys_spawn(ostd_pthread_trampoline, stack)` where `ostd_pthread_trampoline` calls the thread function then exits. This is sufficient for `RKNN_FLAG_ASYNC_MASK=0` (synchronous mode). If the SDK calls `pthread_create` unconditionally on init, this implementation ensures it doesn't crash — the spawned thread parks waiting for async work that never comes.

---

## Requirements

### Functional
- FR1: `dynlink::init(blobs: &[&[u8]])` called from `ostd::startup` before `main()`
- FR2: Each blob is a valid aarch64 ELF shared object (`ET_DYN`, `EM_AARCH64`)
- FR3: All `R_AARCH64_RELATIVE`, `R_AARCH64_GLOB_DAT`, `R_AARCH64_JUMP_SLOT` relocation types handled
- FR4: `dynlink::resolve(name: &str) -> *const ()` looks up a symbol across all loaded blobs
- FR5: `.init_array` functions called in load order after all relocations applied
- FR6: `pthread_create` → `sys_spawn` (see Key Insights)
- FR7: `pthread_mutex_*` → ViCell `Spinlock` (see Phase 02)

### Non-functional
- NF1: Runtime linker code lives in `libs/ostd/src/dynlink.rs` and `libs/ostd/src/dynlink/` — NOT in the kernel
- NF2: Cell binary size increase from embedded `.so` blobs is acceptable (librknnrt.so ≈ 3–5 MB on aarch64)
- NF3: `#![forbid(unsafe_code)]` cannot apply to `dynlink.rs` — mark explicitly `#[allow(unsafe_code)]` with `// SAFETY:` on each block
- NF4: No `.so` blob loading on `riscv64` target (conditional compilation)

---

## Architecture

```
Cell ELF (rknn-infer)
├── .text / .data / .bss        ← Rust code
├── __rknn_blob                 ← include_bytes!("librknnrt.so") via INCBIN in linker
├── __stdcpp_blob               ← include_bytes!("libstdc++.so.6")
├── __libm_blob                 ← include_bytes!("libm.so.6")
└── __ViCell_manifest

ostd::startup (before main)
└── dynlink::init(&[__rknn_blob, __stdcpp_blob, __libm_blob])
    ├── parse ELF headers
    ├── mmap each blob into 0x7C000000+ window
    ├── build global symbol table
    ├── apply AARCH64 relocations
    └── call .init_array[]
```

---

## Related Code Files

### Create
- `libs/ostd/src/dynlink.rs` — public `init` + `resolve` API
- `libs/ostd/src/dynlink/` — module directory:
  - `elf.rs` — ELF header/segment/dynamic parsing (`#[repr(C)]` ELF types)
  - `reloc.rs` — AARCH64 relocation appliers
  - `symtab.rs` — symbol table lookup across loaded blobs
  - `pthread_shim.rs` — pthread_create → sys_spawn + pthread_mutex → Spinlock
- `libs/ostd/src/dynlink/blob_map.rs` — VA range allocator for `.so` blobs

### Modify
- `libs/ostd/src/startup.rs` — call `dynlink::init` before `main()`
- `libs/ostd/src/lib.rs` — expose `pub mod dynlink`

### Cell-level (rknn-infer build.rs)
- Bundle `.so` blobs via `build.rs` `INCBIN` or `include_bytes!` + linker section trick

---

## Implementation Steps

1. Add `libs/ostd/src/dynlink/elf.rs`: `#[repr(C)]` types for `Elf64_Ehdr`, `Elf64_Phdr`, `Elf64_Dyn`, `Elf64_Sym`, `Elf64_Rela` (aarch64 only, cfg-gated)
2. Add `blob_map.rs`: static VA bump allocator starting at `0x7C000000`; returns base VA for each blob
3. Add `reloc.rs`: handle `R_AARCH64_RELATIVE` (base + addend), `R_AARCH64_GLOB_DAT` (symbol value), `R_AARCH64_JUMP_SLOT` (PLT stub fixup)
4. Add `symtab.rs`: iterate `DT_SYMTAB` + `DT_HASH` / `DT_GNU_HASH` in each loaded blob; build a flat `&[(&str, usize)]` symbol map; `resolve(name)` does linear scan (small enough at startup)
5. Add `pthread_shim.rs`: implement `pthread_mutex_init/lock/unlock/destroy/trylock` over `Spinlock<()>`; implement `pthread_create` → `sys_spawn`; implement `pthread_cond_wait/signal` as no-ops initially (valid only for sync inference mode)
6. Add `dynlink.rs` public API: `pub fn init(blobs: &[&[u8]])` → calls elf/blob_map/reloc/symtab in order; panics if any mandatory symbol from the POSIX shim table is missing
7. Wire into `libs/ostd/src/startup.rs`: call `dynlink::init(&[])` unconditionally (no-op when slice is empty; existing cells pass empty slice)
8. Verify: `cargo check -p ostd --target aarch64-unknown-none` clean
9. Unit test: create a minimal `ET_DYN` ELF blob in-test (just `R_AARCH64_RELATIVE` relocations) and verify they're applied correctly — no hardware needed

---

## Todo

- [ ] Create `libs/ostd/src/dynlink/elf.rs` with ELF ABI types (aarch64-only)
- [ ] Create `libs/ostd/src/dynlink/blob_map.rs` with VA bump allocator
- [ ] Create `libs/ostd/src/dynlink/reloc.rs` with three AARCH64 relocation handlers
- [ ] Create `libs/ostd/src/dynlink/symtab.rs` with symbol table builder + resolver
- [ ] Create `libs/ostd/src/dynlink/pthread_shim.rs` with Spinlock-backed mutex + spawn-backed thread create
- [ ] Create `libs/ostd/src/dynlink.rs` with public `init` + `resolve` API
- [ ] Update `libs/ostd/src/startup.rs` to call `dynlink::init`
- [ ] Update `libs/ostd/src/lib.rs` to expose `pub mod dynlink`
- [ ] `cargo check -p ostd --target aarch64-unknown-none` passes
- [ ] Unit test: minimal R_AARCH64_RELATIVE relocation in-test

---

## Success Criteria

1. `cargo check -p ostd --target aarch64-unknown-none` clean (no errors, no new warnings)
2. Unit test `dynlink::reloc_relative` passes on host (can run on std test harness using a heap-allocated mock blob)
3. Cell that calls `dynlink::init(&[])` with an empty slice starts and exits normally (no regression)
4. `dynlink::init` with a real `libstdc++.so.6` blob (sourced from a cross-compiled aarch64 sysroot) applies relocations and calls `.init_array` without crashing

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `librknnrt.so` uses `DT_GNU_HASH` (newer hash format) rather than `DT_HASH` | Implement both; prefer `DT_GNU_HASH` when present |
| `libstdc++.so.6` `.init_array` calls `malloc` before heap is initialized | Ensure `ostd` heap init runs before `dynlink::init` |
| `R_AARCH64_COPY` relocation type (copies symbol data into cell .bss) | Add handler; `librknnrt.so` may not use it but `libstdc++` might |
| `pthread_create` called before heap is ready | Assert in pthread_shim that heap is initialized |

---

## Security Considerations

- The `.so` blobs are bundled at build time — no runtime code loading from untrusted sources
- `dynlink` only runs once during `startup` — no hot-patching surface
- All `unsafe` blocks in `dynlink/` require `// SAFETY:` comments explaining invariants
- Law 4 (`#![forbid(unsafe_code)]` on cells) is not violated: `dynlink` lives in `ostd`, not in cell source
