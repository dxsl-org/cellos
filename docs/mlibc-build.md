# mlibc Build Guide — ViCell Tier B C Library

## Overview

ViCell uses a two-tier C library strategy:

| Tier | Crate | When to use |
|------|-------|------------|
| **A — posix shim** | `libs/api` (default) | Simple cells, embedded, no-std, small binary size |
| **B — mlibc** | `libs/mlibc-shim` + `api/mlibc` feature | Complex C apps needing Grisu3 printf, real malloc, broad POSIX |

The two tiers are **mutually exclusive within a single binary**. Never link both.

---

## Prerequisites (one-time setup in WSL2)

```bash
# Install Meson + aarch64 cross-compiler
sudo apt update && sudo apt install -y meson ninja-build \
    gcc-aarch64-linux-gnu g++-aarch64-linux-gnu

# Verify riscv xpack toolchain is reachable (installed at C:\RISCV on Windows)
/mnt/c/RISCV/riscv-none-elf-gcc-15.2.0-1/bin/riscv-none-elf-gcc --version
```

If the riscv toolchain path differs, update `scripts/mlibc-riscv64.cross` (`[binaries]` section).

---

## Building mlibc

**All Meson/ninja steps run in WSL2** — the Rust workspace never invokes Meson.

```bash
# From the ViCell WSL2 path, e.g. /mnt/d/ViCell
bash scripts/build-mlibc.sh
```

This produces:
- `third_party/mlibc/build/libc.a` — riscv64 static library
- `third_party/mlibc/build-aarch64/libc.a` — aarch64 static library

After the script completes, `cargo check -p app-mlibc-smoke` succeeds.

### Manual build steps (for debugging)

```bash
cd /path/to/ViCell/third_party/mlibc

# riscv64
meson setup build \
    --cross-file=../../scripts/mlibc-riscv64.cross \
    -Dsysdeps=vicell \
    -Ddefault_library=static \
    -Dposix_option=disabled \
    -Dlinux_option=disabled \
    -Dheaders_only=false
ninja -C build
ls -lh build/libc.a

# aarch64
meson setup build-aarch64 \
    --cross-file=../../scripts/mlibc-aarch64.cross \
    -Dsysdeps=vicell \
    -Ddefault_library=static \
    -Dposix_option=disabled \
    -Dlinux_option=disabled \
    -Dheaders_only=false
ninja -C build-aarch64
ls -lh build-aarch64/libc.a
```

---

## Applying the ViCell sysdeps patch to a fresh mlibc clone

When forking a new mlibc commit, add the ViCell branch to `third_party/mlibc/meson.build`:

```python
# In mlibc's top-level meson.build, find the host_machine.system() dispatch block
# and add this branch before the final else/error:
elif host_machine.system() == 'vicell'
    subdir('sysdeps/vicell')
```

The ViCell sysdeps live entirely under `sysdeps/vicell/` — no other mlibc files are modified.

**Pinned mlibc commit:** _(update this when you fork a new commit)_
```
SHA: TBD — record the commit used during first build here
```

---

## Using mlibc in a cell

### Cargo.toml

```toml
[dependencies]
api        = { path = "../../../libs/api", features = ["mlibc"] }
ostd       = { path = "../../../libs/ostd" }
mlibc-shim = { path = "../../../libs/mlibc-shim" }
```

**Critical:** `api` must have `features = ["mlibc"]`. Without it, posix.rs is NOT suppressed and you will get duplicate-symbol link errors.

### Rust source

```rust
#![no_std]
#![no_main]
extern crate mlibc_shim; // pulls in libc.a via build.rs link directives

extern "C" {
    fn printf(fmt: *const u8, ...) -> i32;
    fn malloc(size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
}
```

### Linker script

Use VA base `0x0E000000` for mlibc-smoke. Future mlibc-backed apps should pick the next free VA slot (see `docs/code-standards.md` for the VA map).

---

## Architecture: ViCell sysdeps

All syscalls route through `sysdeps/vicell/include/vicell/syscall.h`:

```
mlibc printf("%.15g", x)
  └─ Grisu3 formatter → sys_write(fd, buf, n)    ← declared in sysdeps.hpp
       └─ generic.cpp: vicell_syscall(109, fd, buf, n, 0)
            └─ riscv64: a7=109, ecall   │   aarch64: x0=109, svc #0
                                        └──── CRITICAL: aarch64 uses x0=nr, NOT x8
```

### Syscall constants (from `libs/api/src/syscall.rs`)

| Symbol | Number |
|--------|--------|
| Exit | 60 |
| Log | 11 |
| Open | 101 |
| Read | 102 |
| Close | 103 |
| Seek | 106 |
| Write | 109 |
| GetTime | 120 |

**GetTime op-selectors:** op=0 → 10 MHz monotonic ticks; op=2 → epoch nanoseconds (RTC); op=3 → epoch seconds.

**Open arg order:** `vicell_syscall(101, path_ptr, path_len, flags, mode)` — note `path_len` is passed separately, unlike POSIX where the string is null-terminated.

### Anonymous memory

mlibc's allocator (`frg::slab_allocator`) is backed by a **4 MB static bump arena** in `generic.cpp`. `AnonFree` is a no-op in G2. If a cell exhausts the arena, `sys_anon_allocate` returns `ENOMEM` and logs a message. The arena size is a compile-time constant — increase it for memory-hungry applications.

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `mlibc libc.a is missing` at `cargo check` | Haven't built mlibc yet | Run `bash scripts/build-mlibc.sh` in WSL2 |
| Duplicate symbol `printf` / `malloc` at link | `api` missing mlibc feature | Add `features = ["mlibc"]` to api dependency |
| `undefined reference to mlibc::sys_write` | mlibc build used wrong sysdeps | Ensure `meson setup -Dsysdeps=vicell` |
| Wrong output on aarch64 (garbage data) | Wrong register layout in syscall.h | aarch64 ABI uses x0=nr (not x8) — see syscall.h |
| Arena exhausted (malloc returns null) | Allocating >4 MB total | Increase `ARENA_SIZE` in generic.cpp |
