# LLVM C++ runtimes — the `cpp-hosted` profile

**Upstream**: https://github.com/llvm/llvm-project (`libcxx/`, `libcxxabi/`, `libunwind/`)
**Pin**: tag `llvmorg-19.1.7`, commit `cd708029e0b2869e80abe31ddb175f7c35361f90`, set in `scripts/build-llvm-runtimes.sh`
**License**: `Apache-2.0 WITH LLVM-exception` (libc++ also keeps its legacy NCSA/MIT dual licence)
**Purpose**: hosted C++17 for x86_64 Tier-2 Cells over mlibc `posix` ([ADR-0034](../../docs/decisions/0034-hosted-cxx-runtime-profile.md); runtime plan phase R4)

Not vendored. `scripts/build-llvm-runtimes.sh` fetches the pin (sparse: `runtimes`, the three
runtime trees and the CMake modules they use) into the gitignored `target/llvm-runtimes/src`,
checks the licence of every file, and installs static archives and headers into
`target/llvm-runtimes/x86_64/{lib,include/c++/v1}`.

The pin is the newest release whose libc++ supports clang 18, the Ubuntu 24.04 compiler of
every Cellos C/C++ row; libc++ 20 requires clang 19. Moving the pin is a deliberate edit:
rerun all rows, the witness link proof and the size table in ADR-0034.

## Licence check (per file)

Step 2 of the script reads every file under `libcxx/{include,src}`, `libcxxabi/{include,src}` and
`libunwind/{include,src}` at the pin (1204 files on 2026-11-05):

- every source and header carries `SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception`;
- build metadata without the line (`CMakeLists.txt`, `*.modulemap`, `*.in`, `*.txt`,
  `libcxxabi/src/demangle/cp-to-llvm.sh`) is never compiled into a Cell;
- three one-line init-priority fragments have no header of their own and are `#include`d
  into SPDX-tagged `.cpp` files: `libcxx/src/iostream_init.h`,
  `libcxx/src/chrono_system_time_init.h`, `libcxx/src/memory_resource_init_helper.h`. The
  script fails if one of them grows;
- the 12 Ryu files under `libcxx/src` (floating-point `to_chars`) also carry Ulf Adams' Boost
  Software License 1.0 notice. Boost-1.0 requires no notice for object code; `NOTICE` names
  it anyway.

The three `LICENSE.TXT` files must read "Apache License v2.0 with LLVM Exceptions". The SPDX
expression is in the `deny.toml` allow list, and ADR-0030 reuse order 1 applies. The LLVM
exception waives Sections 4(a), 4(b) and 4(d) of the Apache License for object-form
embedding.

## NOTICE text

Every image that ships a `cpp-hosted` Cell carries the `NOTICE` entry "LLVM C++ runtimes (libc++,
libc++abi, libunwind)".

## Profile contract

**Compile** (C++ consumers; `cells/tests/cpp-hosted-smoke/build.rs` is the reference):

```text
clang++ --target=x86_64-unknown-none-elf -std=c++17
        -msse2 -mfpmath=sse -mno-red-zone -mcmodel=small -fPIC -fno-stack-protector
        -D_GNU_SOURCE -nostdinc++ -nostdlibinc
        -isystem target/llvm-runtimes/x86_64/include/c++/v1
        -isystem third_party/mlibc/build-x86_64/sysroot/include
        -ffunction-sections -fdata-sections
```

- `-fexceptions -frtti` (the default) or `-fno-exceptions -fno-rtti`, chosen per translation unit.
- The float rows are those of `tools/cellos-cc`'s `x86_64-unknown-none-sse2` target (rule H1):
  no float or vector type crosses the Rust↔C++ boundary.
- `_GNU_SOURCE` matches what clang++ predefines on GNU targets. It opens mlibc's POSIX-2024 surface.
- `-nostdlibinc` keeps the host's `/usr/include` out; clang's resource headers stay.

**Link.** The Rust Cell host links (`tools/cellos-cc:3-4`):
- `-lc++ -lc++abi -lunwind` (static);
- mlibc's `libc.a` and `libgcc.a` through `libs/mlibc-shim`;
- `api` with `features = ["mlibc"]`, so the Tier-A C++ ABI layer stays out.

Every C ABI entry point called from Rust is `extern "C"` and `noexcept`.

**Compiler builtins.** These come from `libgcc.a` of the compiler that built mlibc, which `libs/mlibc-shim` places ahead of Rust `compiler_builtins`. compiler-rt is not used (`*_USE_COMPILER_RT=OFF`). The witness links with `--no-allow-multiple-definition`, and no member of `compiler_builtins` is extracted.

**Libc tier.** mlibc with the `posix` option (runtime plan R2/R3). Step 4 of the script fails if any libc symbol referenced by the archives is missing from `libc.a`. A gap is fixed in mlibc with a ledger row, never stubbed here. The first gap was five locale-suffixed functions: `strtoll_l` and `strtoull_l`, which `<locale>` needs in every consumer, and `strxfrm_l`, `wcscoll_l` and `wcsxfrm_l`, which libc++'s `locale.cpp` needs. `third_party/mlibc/patches/0002-posix-locale-l-functions.patch` adds them.

**Facilities.**
- Exceptions and RTTI: libc++abi, plus libunwind with `.eh_frame_hdr` lookup through mlibc's `dl_iterate_phdr`. The Cell links with `--eh-frame-hdr`, which rustc passes, so `PT_GNU_EH_FRAME` is present.
- Thread-safe statics: libc++abi guards on mlibc pthread mutex and condvar.
- `std::thread`, `std::future`, `std::shared_mutex`: mlibc pthread on `Spawn`, `FutexWait` and `FutexWake`.
- `thread_local`: mlibc's static TLS block.
- iostreams and the `C`/`POSIX` locale: `<locale>` uses libc++'s own C-locale classification table (`_LIBCPP_PROVIDES_DEFAULT_RUNE_TABLE` in `__config_site`), as on musl.

**Build options** (script `COMMON_OPTS`):
- **On:**
  - static only;
  - exceptions and RTTI;
  - threads (pthread API);
  - monotonic clock;
  - localization;
  - wide characters;
  - Unicode;
  - `<filesystem>`;
  - random device;
  - non-silent terminate.
- **Off:**
  - shared libraries;
  - the time-zone database (it needs a zoneinfo tree);
  - the experimental library;
  - C++20 module sources;
  - compiler-rt.

**Refused or limited:**
- `dlopen`: refused.
- `std::random_device`: opens `/dev/urandom`. libc++ selects that source for this target. The Cell's VFS must provide the device, or the constructor throws `std::system_error`.
- `<filesystem>` operations: they call mlibc functions whose vicell sysdeps do not exist yet (`opendir`, `stat` family beyond `Stat`, `statvfs`), so they fail with `ENOSYS` errors.
- Locales other than `C`/`POSIX`: refused unless measured. mlibc collation (`strcoll_l`, `strxfrm_l`, `wcscoll_l`, `wcsxfrm_l`) is byte order.
- `std::thread` while R3b is absent:
  - every thread runs on the kernel's 64 KiB Spawn stack (`sysdeps/vicell/generic/thread.cpp`);
  - every allocation, thread control blocks included, comes from mlibc's 16 MiB static arena, which never returns memory to the kernel;
  - deep recursion, large stack arrays and long-running thread churn are out of contract until R3b.
- An uncaught exception runs this sequence:
  - `std::terminate` → libc++abi prints `libc++abi: terminating due to uncaught exception of type …` to `stderr`;
  - `abort()`;
  - mlibc has no `Sigaction` sysdep, so it logs `mlibc: sigaction failed in abort` and `mlibc PANIC`;
  - the Cell exits with status 1.
- `ELAST`: libc++ has no `ELAST` for this target (build warning in `system_error.cpp`). `std::error_code` messages for unknown values use mlibc's `strerror`.

## Build

mlibc first, then the runtimes. The host dry run needs no Cellos sysroot:

```sh
bash scripts/build-mlibc.sh
bash scripts/build-llvm-runtimes.sh host     # x86_64-linux-gnu dry run + host witness run
bash scripts/build-llvm-runtimes.sh x86_64   # cross build over mlibc + libc symbol check
bash scripts/build-llvm-runtimes.sh size     # ADR-0034 size table
RUSTFLAGS="-C relocation-model=pic" \
  cargo build --release --target x86_64-unknown-none -p app-cpp-hosted-smoke
```

`MLIBC_X86_64_DIR` points the runtime build at another mlibc output directory. The witness
build always uses `third_party/mlibc/build-x86_64`, the directory that `libs/mlibc-shim` links.
