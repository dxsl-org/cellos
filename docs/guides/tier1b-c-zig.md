# Tier 1 `ffi-posix` Profile — C ABI via POSIX or mlibc

> Legacy name: Tier 1b. Call trusted C/C++/Zig libraries from a Tier 1 Rust Cell,
> or link C code directly. Two profiles: POSIX shim and full mlibc.

---

## POSIX Shim vs mlibc

| | **POSIX shim profile** | **mlibc profile** |
|---|---|---|
| **Setup** | `api = { features = ["posix"] }` | Requires `scripts/build-mlibc.sh` in WSL2; then `api = { features = ["mlibc"] }` |
| **Function coverage** | ~20 POSIX symbols (getentropy, socket, printf, malloc) | Full POSIX + glibc extensions |
| **Link size** | ~5 KB | ~400 KB |
| **Use case** | Quick C interop for simple functions | Heavy C code (curl, zlib, sqlite, etc.) |
| **Complexity** | Low | High (mlibc build in separate shell) |

**Critical**: never enable both features. They are **mutually exclusive**.

---

## POSIX Shim Profile

Minimal C ABI for common functions. Declared in `libs/api/src/services/posix.rs`.

### Setup

```rust
// Cargo.toml
[dependencies]
api = { path = "libs/api", features = ["posix"] }

// main.rs
extern "C" {
    fn malloc(size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
    fn printf(fmt: *const u8, ...) -> i32;
    fn socket(domain: i32, socktype: i32, protocol: i32) -> i32;
    fn getentropy(buf: *mut u8, len: usize) -> i32;
    fn _time(tloc: *mut i64) -> i64;
    fn _gettimeofday(tv: *mut c_void, tz: *mut c_void) -> i32;
}
```

### Available Functions

The generated, checked symbol-and-failure table is
[POSIX shim export contract](posix-shim-contract.generated.md). It is the source of truth:
an absent symbol is unsupported rather than an invitation to discover an ABI boundary at final
link time.

For a C application port, use the [C porting guide](porting-c-apps.md) and its
`port-platform` host API. That guide defines the C hook boundary, CMake/Meson cross recipes, and
the explicit static-native refusals (`fork`, dynamic loading, `mprotect`, and `mmap`).

### Example: Getentropy

```rust
use core::ffi::c_void;

extern "C" {
    fn getentropy(buf: *mut c_void, len: usize) -> i32;
}

unsafe {
    let mut random = [0u8; 32];
    if getentropy(random.as_mut_ptr() as *mut c_void, 32) == 0 {
        // random[] filled with 32 bytes of entropy
    }
}
```

---

## mlibc Profile

Complete C standard library via mlibc (libc.a). Supports fork(), pthread, complex math, etc.

### Build mlibc (One-Time Setup)

On **Windows in WSL2**:

```bash
# In Cellos root
cd scripts
bash build-mlibc.sh

# Check result: should create mlibc/aarch64-Cellos/lib/libc.a (and other targets)
ls mlibc/aarch64-Cellos/lib/libc.a
```

Mlibc is **git-ignored**; it's rebuilt as part of the kernel build. No commit needed.

### Setup

```rust
// Cargo.toml
[dependencies]
api = { path = "libs/api", features = ["mlibc"] }

// Manifest: block_io false (unless you need raw disk)
api::declare_manifest!(block_io = false, network = false, spawn = false);

// main.rs
extern "C" {
    fn printf(fmt: *const u8, ...) -> i32;
    fn malloc(size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
    fn clock_gettime(clock_id: i32, tp: *mut libc::timespec) -> i32;
    fn sqrt(x: f64) -> f64;
    // ... any C symbol
}
```

### Example: Complex Math

```rust
extern "C" {
    fn sqrt(x: f64) -> f64;
    fn sin(x: f64) -> f64;
}

fn main() {
    unsafe {
        let result = sqrt(16.0);  // 4.0
        let sine = sin(3.14159 / 2.0);  // ~1.0
    }
}
```

### Common Functions

- **Stdio**: `printf`, `fprintf`, `sprintf`, `vprintf` (buffering via syscalls)
- **Memory**: `malloc`, `calloc`, `realloc`, `free`
- **String**: `strlen`, `strcpy`, `strcmp`, `strtok`, `snprintf`
- **Math**: `sqrt`, `sin`, `cos`, `exp`, `log`
- **Time**: `clock_gettime`, `gettimeofday`
- **Entropy**: `getentropy`
- **Network**: `socket`, `connect`, `send`, `recv`, `close` (as in the POSIX shim profile)

---

## Mutual Exclusion (CRITICAL)

**Never do this:**

```rust
#[cfg(feature = "posix")]
extern "C" { fn my_func(); }

#[cfg(feature = "mlibc")]
extern "C" { fn my_func(); }
```

If both features are enabled, the linker will fail with duplicate symbols or undefined references. **Pick one and stick with it.**

Use a build.rs to enforce exclusivity:

```rust
// build.rs
fn main() {
    let posix = cfg!(feature = "posix");
    let mlibc = cfg!(feature = "mlibc");
    if posix && mlibc {
        panic!("cannot enable both 'posix' and 'mlibc' features");
    }
}
```

---

## C Runtime Constraints

Cellos SAS laws apply to C code too (since it's in a Rust Cell):

❌ **Fork / subprocess spawning** — SAS has no fork. Use `spawn = true` manifest + `sys_spawn` from trusted Rust orchestration only.
❌ **Mmap** — No virtual memory per-cell. Use heap (malloc) or VFS.
❌ **Signals / SIGCHLD** — Not applicable in SAS.
✅ **Pthreads** — Supported via `sys_task_spawn` (POSIX threads map to kernel tasks).
✅ **Sockets** — Full support via POSIX shim or mlibc.

---

## Manifest & Syscalls

```rust
api::declare_manifest!(
    block_io = false,     // Use VFS, not raw disk
    network = true,       // If using sockets
    spawn = false         // Only if you're init/shell
);

api::declare_syscalls![
    Send, Recv, Log, Exit,
    GetTime,
    GetRandom,
    LookupService,
    AnonAllocate  // for malloc
];
```

---

## Canonical Examples

- **POSIX shim profile**: [cells/tests/posix-shim-test/src/main.rs](../../cells/tests/posix-shim-test/src/main.rs) — getentropy, socket, connect, send/recv.
- **mlibc profile**: [cells/tests/mlibc-smoke/src/main.rs](../../cells/tests/mlibc-smoke/src/main.rs) — malloc, printf, clock_gettime.

---

## When to Use Tier 1 `ffi-posix`

✅ Have existing C/C++/Zig code
✅ Need glibc functions (complex math, pthreads, stdio)
✅ Interfacing with a library (zlib, curl, etc.)

❌ Building from scratch in Rust → stay with Tier 1
❌ Need untrusted code isolation → Tier 2 contains native FFI code (private page table) and its
admission control is on the path ([ADR-0019](../decisions/0019-tier2-admission-control-on-path.md));
a fleet-secure profile denies domain-class artifacts, and qualification claims stay ledger-gated.

---

## Build & Run

```bash
# POSIX shim profile — no special build
cargo build --release --target riscv64gc-unknown-none-elf

# mlibc profile — requires WSL2 + build-mlibc.sh
# (already built by kernel/Makefile, linked automatically)
cargo build --release --target riscv64gc-unknown-none-elf
```

---

## Troubleshooting

**Linker error: undefined reference to `sqrt`?**
→ You're using the POSIX shim profile. For math functions, build mlibc or implement them in Rust.

**Both features enabled?**
→ The linker fails with duplicate symbol errors. Remove one feature from Cargo.toml.

**Malloc returns null?**
→ Heap exhausted (cell quota too small). See [code-standards.md](../code-standards.md) § Cell quotas.

---

## Next Steps

- Need to write unsafe code? → Keep it behind the trusted Tier 1 `ffi-posix` boundary.
- Need UIs in Rust? → [Tier 1 + ViUI](viui-guide.md)
- Need a protected relay key? → use the purpose-specific KMS client; the [development Silo provider](tier1-silo.md) is not an app API.
- See [mlibc-build.md](../mlibc-build.md) for mlibc compilation details.
- Want to write a cell in Zig? → See **Pure Zig Cells** section below.

---

## C++ (freestanding subset) — `cpp-freestanding`

C++ is a **language subset** profile, not hosted C++
([ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md) §2.3).
Reference cell: [cells/tests/cpp-smoke](../../cells/tests/cpp-smoke/); runner:
[scripts/qemu-cpp-smoke.sh](../../scripts/qemu-cpp-smoke.sh).

**Required flags:** `-fPIC -ffreestanding -fno-exceptions -fno-rtti -fno-threadsafe-statics
-fno-use-cxa-atexit`, plus `cc::Build::cpp_link_stdlib(None)` so no `-lstdc++` directive is
emitted.

**Runtime:** you do not supply it. The POSIX shim already provides the C++ ABI layer —
`operator new`/`delete` (all six forms) in `libs/api/src/services/posix/alloc.rs`,
`__cxa_pure_virtual`, `__cxa_guard_*`, `abort`, and `atexit`/`__cxa_atexit` in
`libs/api/src/services/posix/cxxabi.rs`. Static destructors never run (registration is accepted
and ignored — a cell has no process teardown). Static constructors do run: `ostd` crt0 walks
`__init_array` (`libs/ostd/src/startup.rs:42-43,67-70,91-92`).

**No C++ standard headers.** Neither cross toolchain ships them (`riscv64-unknown-elf-g++` has no
libstdc++ headers; `clang++ --target=aarch64-unknown-none-elf` has no libc++ sysroot), so
`#include <cstdint>` fails on both. Use compiler builtins (`__UINT32_TYPE__`, `__SIZE_TYPE__`, …)
as `cpp/cxx_support.hpp` does. Language features — classes, virtual dispatch, templates, static
construction, `new`/`delete` — are all available.

**Not available:** exceptions, RTTI, thread-safe statics, and the STL runtime (`std::string`,
`std::vector`, iostreams, locale). Do not link libstdc++/libc++ silently — the runner asserts
`__cxa_throw`/`_Unwind_*`/`_ZSt*` are absent from the linked cell.

**Architectures:** RV64 and AArch64 only, because the profile's runtime layer is the POSIX shim
(`#![cfg(any(riscv64, aarch64, wasm32, doc))]`). The build script fails with that reason on other
targets.

**Launch edge:** a new `/bin/<cell>` path needs a reviewed row in
`kernel/src/loader/launch_profile/targets.rs` before the shell or desktop may launch it —
`c-ffi` cells take a `CapSet::EMPTY` ceiling there unless the cell genuinely needs authority.

---

## TLS (thread-local storage) contract

Cellos gives each **task** its own user thread pointer; the kernel never
dereferences it. The contract is short, and it is the same on every architecture:

| Step | Who does it |
|---|---|
| Allocate a TLS block | the cell (heap, static arena — its choice) |
| Claim it as this thread's base | `sys_set_tls_base(base)` → returns the previous base |
| Read the base back | the return value of a re-set, or the register where the architecture allows an unprivileged read |
| Per-thread isolation across switches | the kernel reinstalls the base on every resume |

Reference cell: [cells/tests/tls-test](../../cells/tests/tls-test/); runner:
[scripts/qemu-tls-test.sh](../../scripts/qemu-tls-test.sh) (`--harts 2` repeats the
run with a second hart online).

**The kernel owns the register.** A cell must set its base through the syscall, not
by writing `tp`/`TPIDR_EL0`/`FS_BASE` itself: a direct write is overwritten on the
next resume. `SetTlsBase` is self-only (the ABI has no target parameter) and always
permitted, because one word of the caller's own register state carries no authority.

**Per-architecture carrier:**

| Arch | Where the value lives |
|---|---|
| riscv64 | the user `tp` (x4) slot of the task's trap frame; `__trap_exit` restores it. The kernel's own `tp` (the HartLocal pointer) is a different value, reloaded on every U→S transition. |
| aarch64 | `TPIDR_EL0`, written by the kernel on every resume (`TPIDR_EL1` stays the kernel's). |
| x86_64 | `FS_BASE` (`IA32_FS_BASE`), written on every resume; `GS_BASE`/`KERNEL_GS_BASE` stay kernel context state. |

**Threads inherit their creator's base** until they set their own — a thread started
inside a runtime that already set one up keeps working. A worker thread's `Exit`
terminates only that thread (and wakes a `Wait` joiner); only the cell's root task
exit retires the cell generation.

**C/C++ `__thread` / `thread_local` is not supported yet.** It needs a TLS runtime:
the loader must expose the program's `PT_TLS` segment (size + initial image) and the
userspace side must allocate a block per thread and place it so the linker's
`initial-exec` offsets resolve. The kernel primitive here is the part that runtime
needs; the block management is a separate piece of work, tracked in
`.agents/260922-1549-cell-native-portability-program/phase-03-per-task-tls-base.md`
§ Deviation Log.

---

## Pure Zig Cells

Write a Cellos cell entirely in Zig — no Rust, no Cargo. Supported since Cellos v1.x (Mycelium).

Two integration levels mirror the C POSIX-shim/mlibc split:

| Level | C equivalent | mlibc? | Use case |
|-------|-------------|--------|---------|
| **A** — raw syscalls | POSIX shim profile | No | Minimal Zig logic, custom tooling |
| **B** — mlibc linked | mlibc profile | Yes | Existing Zig code using libc functions |

---

### Prerequisites

- Zig 0.13+ in `$PATH` (`zig version` should show 0.13.x or later)
- For Level B: mlibc must be built first — `pwsh scripts/setup-mlibc.ps1` (riscv64 / Windows)
  or `bash scripts/build-mlibc.sh` in WSL2 (aarch64)
- `libs/zig-syscall/` provides the Cellos syscall shim and manifest helper

---

### Level A — Raw Syscalls (no mlibc)

Copy `cells/tests/zig-hello/` as your starting point.

**`build.zig.zon`** (declare the zig-syscall dependency):
```zig
.{
    .name = "my-zig-cell",
    .version = "0.1.0",
    .minimum_zig_version = "0.13.0",
    .dependencies = .{
        .zig_syscall = .{ .path = "../../../libs/zig-syscall" },
    },
    .paths = .{"."},
}
```

**`src/main.zig`** (minimal cell skeleton):
```zig
const sys = @import("zig-syscall").syscall;
const manifest = @import("zig-syscall").manifest;

// Emit __ViCell_manifest ELF section with capability flags
comptime {
    manifest.declare(.{ .flags = 0 });
}

export fn _start() callconv(.C) noreturn {
    sys.write(1, "Hello from Zig!\n");
    sys.exit(0);
}
```

**Build:**
```powershell
cd cells/tests/my-zig-cell
zig build -Dtarget=riscv64-freestanding-none -Doptimize=ReleaseSmall
# ELF at: zig-out/bin/my-zig-cell
```

---

### Level B — Full mlibc (printf, malloc, clock_gettime)

Copy `cells/tests/zig-mlibc-smoke/` as your starting point. Key differences from Level A:

1. `build.zig` adds `exe.addLibraryPath(mlibc_lib_dir)` + `exe.linkSystemLibrary("c")`
2. `_start` calls `__libc_start_main` (initialises mlibc's slab allocator before your code runs)
3. `exe.bundle_compiler_rt = true` (provides 128-bit arithmetic builtins)

**`src/main.zig`** skeleton:
```zig
const sys = @import("zig-syscall").syscall;
const manifest = @import("zig-syscall").manifest;

comptime {
    manifest.declare(.{ .flags = 0 });
}

extern fn printf(fmt: [*:0]const u8, ...) c_int;
extern fn malloc(size: usize) ?*anyopaque;
extern fn __libc_start_main(
    main_fn: *const fn (c_int, [*][*:0]u8, [*][*:0]u8) callconv(.C) c_int,
    argc: c_int,
    argv: [*][*:0]u8,
) c_int;

export fn _start() callconv(.C) noreturn {
    const S = struct {
        fn callMain(_: c_int, _: [*][*:0]u8, _: [*][*:0]u8) callconv(.C) c_int {
            _ = printf("Hello from Zig + mlibc!\n");
            return 0;
        }
    };
    const dummy_argv = [1][*:0]u8{@ptrFromInt(1)};
    _ = __libc_start_main(S.callMain, 0, @constCast(&dummy_argv));
    sys.exit(1); // unreachable
}
```

---

### `libs/zig-syscall` API Reference

```zig
const sys = @import("zig-syscall").syscall;

sys.exit(code: u8) noreturn        // sys_exit (nr=60)
sys.log(msg: []const u8) void      // sys_log  (nr=11) — kernel-side logging
sys.write(fd: usize, buf: []const u8) void  // sys_write (nr=109)
sys.get_time(op: GetTimeOp) u64    // sys_get_time (nr=120)

pub const GetTimeOp = enum(usize) {
    ticks    = 0,   // arch-specific monotonic ticks (10 MHz riscv64 / 62.5 MHz aarch64 / ns x86_64)
    epoch_ns = 2,   // wall-clock epoch nanoseconds (requires RTC)
    epoch_secs = 3, // wall-clock epoch seconds (requires RTC)
};
```

**Manifest flags** (`@import("zig-syscall").manifest.Flags`):

| Constant | Bit | Grants |
|----------|-----|--------|
| `BLOCK_IO` | 0 | Raw FAT32/littlefs block access |
| `NETWORK` | 1 | TCP/UDP socket syscalls |
| `SPAWN` | 2 | sys_spawn (init/shell only) |
| `GPIO` | 3 | GPIO MMIO via sys_request_mmio |
| `UART` | 4 | UART MMIO via sys_request_mmio |

---

### ⚠️ ARM64 ABI Warning

Cellos ARM64 uses `x0=syscall_nr` for `svc #0`. Linux uses `x8=syscall_nr`.

If you copy ARM64 syscall patterns from Linux examples and use `x8`, **every call will silently misdispatch**. The `libs/zig-syscall` shim handles this correctly — use it rather than writing raw inline asm.

---

### x86_64 Support

Level A (raw syscalls) works on x86_64. Level B (mlibc linking) is **not yet available** on x86_64 — mlibc's sysdeps currently only include riscv64 and aarch64.

---

### Canonical Examples

- **Level A**: [cells/tests/zig-hello/src/main.zig](../../cells/tests/zig-hello/src/main.zig)
- **Level B**: [cells/tests/zig-mlibc-smoke/src/main.zig](../../cells/tests/zig-mlibc-smoke/src/main.zig)
- **Syscall shim**: [libs/zig-syscall/src/syscall.zig](../../libs/zig-syscall/src/syscall.zig)
