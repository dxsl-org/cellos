# POSIX adapter ledger

Checked by `python3 scripts/check-posix-ledger.py`. This ledger records what each
optional POSIX adapter exports, what backs it, and how far it is behaviourally
proven. It is not a conformance statement.

## Scope

- **POSIX is optional.** It is a userspace runtime personality (ADR-0018 §2.1,
  `docs/decisions/0018-cell-native-portability-and-runtime-profiles.md`). It is not
  the native SAS ABI and not a kernel syscall coverage target. A native Cell links
  no POSIX code, and nothing here adds a kernel POSIX implementation.
- **Tier A and Tier B are mutually exclusive.** Enabling `api` feature `mlibc`
  compiles out every Tier A module (`libs/posix-shim/src/lib.rs:33-55`), and
  `mlibc-shim` then injects mlibc's `libc.a` (`libs/mlibc-shim/src/lib.rs:1-9`).
  A Cell uses one or the other, never both.
- **The toolchain picolibc/newlib is not a Cellos adapter.** Some RV64 application
  builds (Lua, DOOM, Tetris-Lua) also link the RISC-V toolchain's picolibc/newlib
  with `--allow-multiple-definition` to fill gaps (`cells/runtimes/lua/build.rs:52-55,261-284`,
  `cells/demos/doom/build.rs:189-214`, `cells/demos/tetris-lua/build.rs:38-51,204-228`).
  That is a per-application build workaround, not a supported profile, and it is
  not ledgered here. `tools/cellos-cc` only compiles. It refuses to link and adds no
  runtime symbols.
- **No Linux or POSIX conformance claim.** Source discovery proves only that a
  symbol is declared. It does not prove behaviour.
- **Tier A refusals do not set errno.** Refused Tier A entry points return `-1`
  or `NULL` and leave `errno` untouched. The shim has no `errno` storage. Tier A
  never reports `ENOSYS`, even though ADR-0018 §2.5 says `fork()` and `dlopen()`
  are `ENOSYS`. This ledger records the code as it is and leaves that gap open.
- **Sysdeps are not libc symbols.** Tier B rows list mlibc's internal `sys_*`
  hooks. The libc functions that reach a hook are shown as an inference in
  `consumer_visible_symbol`. They are not a one-to-one mapping.

### Status rule

| status | meaning |
|---|---|
| `implemented` | Supported subset behaves as documented. Needs a behavioural evidence anchor. |
| `partial` | Works with stated limits or divergences. Needs a behavioural evidence anchor. |
| `refused` | Deterministic refusal, shown by a negative behavioural test. Needs a behavioural evidence anchor. |
| `unverified` | No current passing behavioural witness. Source behaviour is still described. |

A behavioural evidence anchor is either `<path>::<fn>`, naming a Rust `#[test]` or
Python `test*` function, or a `docs/evidence/` artifact `path:line`. A source
line is never evidence.

**Current runner evidence: 2026-10-31, RV64.** The three posix-shim-test runners,
`tests/integration/tests/boot.rs::posix_shim_fstat`, `::posix_shim_getentropy` and
`::posix_shim_net`, pass on the RV64 boot suite of 2026-10-31. `posix-shim-test`
keeps its FFI protection tier and is launched from the shell as a Tier-2 paged
domain with `NET_CLIENT` rights (serial: `[domain] admitted cell 'posix-shim-test'
to Tier 2 Paged Domain (SATP isolation)`; ADR-0019 §2.1 amendment of 2026-10-31).
`tests/integration/tests/tier2_fault_isolation.rs::tier2_posix_ffi_execution_runs_in_paged_domain`
passes as well. A row cites a runner only when that runner
asserts the row's behaviour, and the row is then `partial`, because no runner
covers a symbol's full subset and refusals:

- **`posix_shim_fstat`** requires `POSIX-FSTAT-OPEN: OK` and `POSIX-FSTAT: OK`
  exactly once, and no `FAIL`, panic or Cell fault
  (`cells/tests/posix-shim-test/src/fstat.rs:58-115`). That covers:
  - `_open` of a read-only regular file.
  - `_fstat` of that file: `st_mode` regular, `st_size` matching the kernel
    record, all other bytes zero.
  - `_fstat` on fd `-1` returns `-1` and leaves the caller buffer unchanged.
  - `_fstat` with a `NULL` buffer returns `-1`.
  - `_close` of the kernel fd returns 0.
- **`posix_shim_getentropy`** requires `POSIX-ENTROPY: OK`: `getentropy` of 16 bytes
  returns 0 and writes non-zero output. The refusal cases are not asserted.
- **`posix_shim_net`** requires `POSIX-NET: OK` and `POSIX-DNS: OK`. That is one
  echo round trip through `socket(AF_INET, SOCK_STREAM)`, `connect`, `send` and
  `recv` to the literal host address, then `gethostbyname("gateway")`, which must
  return an `AF_INET` 4-byte address equal to that host and usable for a second
  round trip. Refusal cases and the socket `_close` result are not asserted.

`posix-shim-test` also prints `POSIX-STAT`, `POSIX-UNLINK`, `POSIX-MKDIR-RMDIR`,
`POSIX-RENAME` and `PORTING-SMOKE` markers. No runner asserts them, so their rows
stay `unverified`.

## Native transport, POSIX adapters, Linux VM

| layer | what it is | contract | where |
|---|---|---|---|
| Native transport | `ViSyscall` plus typed IPC (VFS, Net and other services) through `api`/`ostd`. This is the native SAS ABI every Cell uses. | Native ABI; no POSIX names | `libs/abi/src/syscall.rs`, `libs/wire/src/ipc.rs` |
| POSIX adapters | Optional userspace C ABI translation onto native transport: Tier A Rust shim or Tier B mlibc sysdeps. | This ledger | `libs/posix-shim/src/`, `third_party/mlibc/sysdeps/vicell/` |
| Linux VM | Tier 3 `linux-guest`: a Linux kernel and userspace in a VM run by the Hypervisor Cell, with VirtIO devices forwarded to Cellos services. Linux syscalls are served by the guest kernel, not by Cellos. | Spec 05 §4 | `docs/specs/05-application.md` §4.2 |

## Tier A: `api::services::posix` (Rust C-ABI shim)

The adapter is active on bare-metal RV64, AArch64 and x86_64 (and wasm32) when
`mlibc` is off (`libs/posix-shim/src/lib.rs:12-55`). One row covers one symbol,
or one `libc-symbol-family` of C99 math functions with identical backing.

### Allocation and C++ ABI

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `malloc` | Rust global allocator via `malloc_impl`; size header | `libs/posix-shim/src/alloc.rs:28-31` | any size; 16-byte aligned | `NULL` on size overflow or allocator failure; errno not set | unverified | none |
| tier-a | libc-symbol | `free` | `free_impl` | `libs/posix-shim/src/alloc.rs:39-42` | pointers from this allocator; `NULL` is a no-op | pointer without the shim header magic is silently ignored | unverified | none |
| tier-a | libc-symbol | `realloc` | `realloc_impl` | `libs/posix-shim/src/alloc.rs:51-54` | `NULL` pointer acts as `malloc`; size 0 frees and returns `NULL` | `NULL` on foreign pointer, overflow or allocator failure; errno not set | unverified | none |
| tier-a | libc-symbol | `calloc` | `malloc` then zero fill | `libs/posix-shim/src/alloc.rs:62-73` | zero-filled block | `NULL` on `nmemb*size` overflow or allocator failure; errno not set | unverified | none |
| tier-a | cxx-abi-symbol | `_Znwm` | `malloc_impl` | `libs/posix-shim/src/alloc.rs:147-150` | `operator new(size_t)` | returns `NULL`; never throws `std::bad_alloc` | unverified | none |
| tier-a | cxx-abi-symbol | `_ZdlPv` | `free_impl` | `libs/posix-shim/src/alloc.rs:157-160` | `operator delete(void*)` | as `free` | unverified | none |
| tier-a | cxx-abi-symbol | `_ZdlPvm` | `free_impl` | `libs/posix-shim/src/alloc.rs:169-172` | sized `operator delete`; size ignored | as `free` | unverified | none |
| tier-a | cxx-abi-symbol | `_Znam` | `malloc_impl` | `libs/posix-shim/src/alloc.rs:180-183` | `operator new[](size_t)` | returns `NULL`; never throws | unverified | none |
| tier-a | cxx-abi-symbol | `_ZdaPv` | `free_impl` | `libs/posix-shim/src/alloc.rs:190-193` | `operator delete[](void*)` | as `free` | unverified | none |
| tier-a | cxx-abi-symbol | `_ZdaPvm` | `free_impl` | `libs/posix-shim/src/alloc.rs:202-205` | sized `operator delete[]`; size ignored | as `free` | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_pure_virtual` | `ViSyscall::Log` then `ViSyscall::Exit` | `libs/posix-shim/src/cxxabi.rs:19-30` | logs and terminates the Cell | never returns | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_guard_acquire` | caller guard word | `libs/posix-shim/src/cxxabi.rs:42-50` | single-threaded only: 1 when the guard is 0, else 0 | no locking; concurrent initialisation unprotected | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_guard_release` | caller guard word | `libs/posix-shim/src/cxxabi.rs:56-59` | marks the guard initialised | n/a | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_guard_abort` | none | `libs/posix-shim/src/cxxabi.rs:67-70` | leaves the guard at 0 so a later call retries | n/a | unverified | none |
| tier-a | libc-symbol | `atexit` | none | `libs/posix-shim/src/cxxabi.rs:85-88` | returns 0 | handler is never run; silent divergence | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_atexit` | none | `libs/posix-shim/src/cxxabi.rs:94-101` | returns 0 | destructor is never run; silent divergence | unverified | none |
| tier-a | libc-symbol | `abort` | `ViSyscall::Exit` with status 134 | `libs/posix-shim/src/cxxabi.rs:109-115` | terminates the Cell | no `SIGABRT`; no signal handlers | unverified | none |

### Memory and string functions

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `memcpy` | Rust byte loop | `libs/posix-shim/src/strings.rs:13-14` | non-overlapping copy | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `memmove` | Rust byte loop | `libs/posix-shim/src/strings.rs:28-29` | overlap-safe copy | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `memset` | Rust byte loop | `libs/posix-shim/src/strings.rs:50-51` | fill bytes | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `memcmp` | Rust byte loop | `libs/posix-shim/src/strings.rs:64-65` | byte compare | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strlen` | Rust byte loop | `libs/posix-shim/src/strings.rs:82-83` | NUL-terminated length | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strcpy` | Rust byte loop | `libs/posix-shim/src/strings.rs:96-97` | copy with terminator | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strncpy` | Rust byte loop | `libs/posix-shim/src/strings.rs:117-118` | bounded copy | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strcmp` | Rust byte loop | `libs/posix-shim/src/strings.rs:139-140` | byte-wise compare; no locale | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strncmp` | Rust byte loop | `libs/posix-shim/src/strings.rs:160-161` | bounded byte-wise compare | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strcat` | `strlen` plus `strcpy` | `libs/posix-shim/src/strings.rs:181-182` | append | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strchr` | Rust byte loop | `libs/posix-shim/src/strings.rs:194` | first byte match | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strrchr` | Rust byte loop | `libs/posix-shim/src/strings.rs:215` | last byte match | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `memchr` | Rust byte loop | `libs/posix-shim/src/strings.rs:236` | bounded byte search | no error path; C pointer preconditions | unverified | none |

### Entropy and non-local jumps

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `getentropy` | `ViSyscall::GetRandom` in chunks of up to 64 bytes | `libs/posix-shim/src/entropy.rs:18-34` | `buflen` at most 256 | `-1` for `NULL` buffer, `buflen` over 256 or syscall failure; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_getentropy`; `cells/tests/posix-shim-test/src/main.rs::test_getentropy` |
| tier-a | libc-symbol | `arc4random_buf` | `getentropy` | `libs/posix-shim/src/entropy.rs:42-45` | fills up to 256 bytes | requests over 256 bytes are silently truncated; failure is not reported | unverified | none |
| tier-a | libc-symbol | `setjmp` | naked assembly per target: RV64, AArch64 hard-float, AArch64 soft-float, x86_64 | `libs/posix-shim/src/setjmp.rs:38`; `libs/posix-shim/src/setjmp.rs:121`; `libs/posix-shim/src/setjmp.rs:180`; `libs/posix-shim/src/setjmp.rs:236`; `libs/posix-shim/src/setjmp.rs:288` | saves callee-saved registers; one definition per target | other targets: returns 0 and saves nothing | unverified | none |
| tier-a | libc-symbol | `longjmp` | naked assembly per target, as `setjmp` | `libs/posix-shim/src/setjmp.rs:76`; `libs/posix-shim/src/setjmp.rs:144`; `libs/posix-shim/src/setjmp.rs:209`; `libs/posix-shim/src/setjmp.rs:258`; `libs/posix-shim/src/setjmp.rs:298` | restores the saved context; `val` 0 becomes 1 | other targets: spins forever and never returns | unverified | none |

### C99 math

All math entry points call the Rust `libm` crate directly. They make no syscall
and never set `errno` or floating-point exception flags.

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol-family | `sin, cos, tan, asin, acos, atan, sinh, cosh, tanh, asinh, acosh, atanh, exp, exp2, expm1, log, log2, log10, log1p, sqrt, cbrt, floor, ceil, round, trunc, rint, fabs` | Rust `libm` (f64) | `libs/posix-shim/src/math.rs:53-79` | libm results | no errno or `math_errhandling`; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `atan2, pow, hypot, fmod, remainder, fmin, fmax, copysign, nextafter` | Rust `libm` (f64) | `libs/posix-shim/src/math.rs:97-105` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `fma, scalbn, ldexp, ilogb` | Rust `libm` (f64) | `libs/posix-shim/src/math.rs:111-129` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `modf, frexp` | Rust `libm` (f64) | `libs/posix-shim/src/math.rs:136-157` | libm results; a `NULL` out-pointer is tolerated and not written | no errno | unverified | none |
| tier-a | libc-symbol | `nearbyint` | `libm::rint` | `libs/posix-shim/src/math.rs:81-85` | round to nearest | no floating-point environment; inexact state is not modelled | unverified | none |
| tier-a | libc-symbol | `logb` | `libm::ilogb` cast to double | `libs/posix-shim/src/math.rs:87-91` | finite non-zero input | 0, infinity and NaN yield the integer `ilogb` sentinel as a double, not -inf, +inf or NaN [INFERENCE: libm sentinel values] | unverified | none |
| tier-a | libc-symbol-family | `sinf, cosf, tanf, asinf, acosf, atanf, sinhf, coshf, tanhf, asinhf, acoshf, atanhf, expf, exp2f, expm1f, logf, log2f, log10f, log1pf, sqrtf, cbrtf, floorf, ceilf, roundf, truncf, rintf, fabsf` | Rust `libm` (f32) | `libs/posix-shim/src/math.rs:163-189` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `atan2f, powf, hypotf, fmodf, remainderf, fminf, fmaxf, copysignf, nextafterf` | Rust `libm` (f32) | `libs/posix-shim/src/math.rs:204-212` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `fmaf, scalbnf, ldexpf, ilogbf` | Rust `libm` (f32) | `libs/posix-shim/src/math.rs:218-236` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `modff, frexpf` | Rust `libm` (f32) | `libs/posix-shim/src/math.rs:243-264` | libm results; a `NULL` out-pointer is tolerated | no errno | unverified | none |
| tier-a | libc-symbol | `nearbyintf` | `libm::rintf` | `libs/posix-shim/src/math.rs:191-194` | round to nearest | no floating-point environment | unverified | none |
| tier-a | libc-symbol | `logbf` | `libm::ilogbf` cast to float | `libs/posix-shim/src/math.rs:195-198` | finite non-zero input | as `logb` | unverified | none |

### stdio

The shim keeps no stream buffer or error state. A `FILE` holds only a descriptor.
Formatting uses `vsnprintf_core` (`libs/posix-shim/src/stdio_fmt.rs:181-407`),
which supports the flags `-` and `0`, a decimal width and precision (no `*`), the
length modifiers `l`, `ll`, `h`, `hh`, `z` and `t`, and the conversions
`% c s d i u x X o p f e g E G`. `e`, `g`, `E` and `G` print fixed-point output.
An unknown conversion is printed literally.

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | c-global | `stdout` | static `FILE` for fd 1 | `libs/posix-shim/src/stdio.rs:38-39` | descriptor 1 | n/a | unverified | none |
| tier-a | c-global | `stderr` | static `FILE` for fd 2 | `libs/posix-shim/src/stdio.rs:40-41` | descriptor 2 | n/a | unverified | none |
| tier-a | c-global | `stdin` | static `FILE` for fd 0 | `libs/posix-shim/src/stdio.rs:42-43` | descriptor 0 | n/a | unverified | none |
| tier-a | libc-symbol | `vsnprintf` | `vsnprintf_core` | `libs/posix-shim/src/stdio.rs:81-89` | bounded format, conversion subset above | unsupported conversion printed literally; no error return | unverified | none |
| tier-a | libc-symbol | `vsprintf` | `vsnprintf_core` | `libs/posix-shim/src/stdio.rs:98-101` | unbounded format, conversion subset above | as `vsnprintf` | unverified | none |
| tier-a | libc-symbol | `vprintf` | `vsnprintf_core` then `_write(1)` | `libs/posix-shim/src/stdio.rs:108-114` | output truncated to 1024 bytes | write failure not reported; returns formatted length | unverified | none |
| tier-a | libc-symbol | `vfprintf` | `vsnprintf_core` then `_write` to the stream fd | `libs/posix-shim/src/stdio.rs:123-134` | output truncated to 1024 bytes | write failure not reported | unverified | none |
| tier-a | libc-symbol | `printf` | `vprintf` | `libs/posix-shim/src/stdio.rs:145-148` | as `vprintf` | as `vprintf` | unverified | none |
| tier-a | libc-symbol | `fprintf` | `vfprintf` | `libs/posix-shim/src/stdio.rs:156-159` | as `vfprintf` | as `vfprintf` | unverified | none |
| tier-a | libc-symbol | `sprintf` | `vsprintf` | `libs/posix-shim/src/stdio.rs:169-172` | as `vsprintf` | as `vsprintf` | unverified | none |
| tier-a | libc-symbol | `snprintf` | `vsnprintf` | `libs/posix-shim/src/stdio.rs:182-190` | as `vsnprintf` | as `vsnprintf` | unverified | none |
| tier-a | libc-symbol | `fopen` | `_open` plus heap `FILE` | `libs/posix-shim/src/stdio.rs:201-220` | a mode starting with `w` opens with create and truncate flags; any other mode opens read-only | `NULL` on open or allocation failure; errno not set | unverified | none |
| tier-a | libc-symbol | `fclose` | `_close`; frees a heap `FILE` | `libs/posix-shim/src/stdio.rs:228-239` | close stream | `-1` for `NULL`; errno not set | unverified | none |
| tier-a | libc-symbol | `fread` | looping `_read` | `libs/posix-shim/src/stdio.rs:247-274` | reads until complete, EOF or error | 0 items for `NULL` stream or buffer; no error flag | unverified | none |
| tier-a | libc-symbol | `fwrite` | single `_write` | `libs/posix-shim/src/stdio.rs:281-298` | one write call | 0 on failure; no error flag | unverified | none |
| tier-a | libc-symbol | `fputs` | `_write` | `libs/posix-shim/src/stdio.rs:305-312` | writes without a newline | `-1` for `NULL` string; returns the raw write result | unverified | none |
| tier-a | libc-symbol | `fputc` | `_write` | `libs/posix-shim/src/stdio.rs:318-326` | one byte | `-1` on failure | unverified | none |
| tier-a | libc-symbol | `fgetc` | `_read` | `libs/posix-shim/src/stdio.rs:333-343` | one byte; `NULL` stream reads fd 0 | `-1` on EOF or error (not distinguished) | unverified | none |
| tier-a | libc-symbol | `fgets` | a single `_read` | `libs/posix-shim/src/stdio.rs:350-362` | one read of up to n-1 bytes; does not stop at a newline | `NULL` on bad arguments, EOF or error | unverified | none |
| tier-a | libc-symbol | `putchar` | `_write(1)` | `libs/posix-shim/src/stdio.rs:368-373` | one byte to stdout | write failure not reported | unverified | none |
| tier-a | libc-symbol | `puts` | `_write(1)` | `libs/posix-shim/src/stdio.rs:379-388` | string plus newline | `-1` for `NULL`; write failure not reported | unverified | none |
| tier-a | libc-symbol | `fseek` | `_lseek` | `libs/posix-shim/src/stdio.rs:402-414` | `SEEK_SET`, `SEEK_CUR`, `SEEK_END` passed through | `-1` for `NULL` stream or seek failure | unverified | none |
| tier-a | libc-symbol | `ftell` | `_lseek(SEEK_CUR)` | `libs/posix-shim/src/stdio.rs:420-426` | current offset | `-1` for `NULL` stream; otherwise the raw seek result | unverified | none |
| tier-a | libc-symbol | `rewind` | `_lseek(SEEK_SET)` | `libs/posix-shim/src/stdio.rs:432-438` | seek to 0 | failure not reported | unverified | none |
| tier-a | libc-symbol | `ferror` | none | `libs/posix-shim/src/stdio.rs:448-451` | always 0 | no error state is tracked | unverified | none |
| tier-a | libc-symbol | `feof` | none | `libs/posix-shim/src/stdio.rs:456-459` | always 0 | no EOF state is tracked | unverified | none |
| tier-a | libc-symbol | `clearerr` | none | `libs/posix-shim/src/stdio.rs:464-465` | no-op | no error state is tracked | unverified | none |
| tier-a | libc-symbol | `fflush` | none | `libs/posix-shim/src/stdio.rs:470-473` | always 0; writes are unbuffered | n/a | unverified | none |
| tier-a | libc-symbol | `fileno` | stream descriptor | `libs/posix-shim/src/stdio.rs:478-485` | descriptor of the stream | `-1` for `NULL`; errno not set | unverified | none |

### Sockets

Sockets are client-side only. The shim keeps a socket table of 8 slots at fds 10-17
and talks to the Net service over typed IPC (`libs/posix-shim/src/net.rs:19-43`).

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `socket` | local socket table | `libs/posix-shim/src/net.rs:146-154` | `AF_INET` with `SOCK_STREAM` only | `-1` for any other domain or type, or when the table is full; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::echo_round_trip` |
| tier-a | libc-symbol | `connect` | Net IPC TCP connect | `libs/posix-shim/src/net.rs:162-185` | IPv4 `sockaddr_in` | `-1` on a bad fd, bad address or transport error; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::echo_round_trip` |
| tier-a | libc-symbol | `send` | Net IPC `TcpSend` | `libs/posix-shim/src/net.rs:205-254` | at most 495 bytes per call; waits up to 5 s for the handshake | `-1` on `NULL`, bad fd, refused peer, timeout or transport error; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::echo_round_trip` |
| tier-a | libc-symbol | `recv` | Net IPC receive | `libs/posix-shim/src/net.rs:261-282` | copies at most `len` bytes | `-1` on `NULL`, bad fd or transport error; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::echo_round_trip` |
| tier-a | libc-symbol | `_close` | socket fds 10-17 use the local table; other fds use `ViSyscall::Close` | `libs/posix-shim/src/net.rs:292-297` | closes a socket or a kernel fd | `-1` on failure; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_fstat`; `cells/tests/posix-shim-test/src/fstat.rs::test_fstat` |
| tier-a | libc-symbol | `close` | `_close` | `libs/posix-shim/src/net.rs:302-304` | as `_close` | as `_close` | unverified | none |
| tier-a | libc-symbol | `gethostbyname` | Net service resolver over IPC | `libs/posix-shim/src/net.rs:392-438` | IPv4 only; a single address; static result storage | `NULL` for a `NULL`, empty, over-long or unresolvable name, or an unreachable service; `h_errno` not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::test_net_dns` |

### File, process and time

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `_open` | `ViSyscall::Open` | `libs/posix-shim/src/sysio.rs:144-154` | path, flags and mode passed through to the kernel | `-1` on kernel failure; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_fstat`; `cells/tests/posix-shim-test/src/fstat.rs::test_fstat` |
| tier-a | libc-symbol | `open` | `_open` | `libs/posix-shim/src/sysio.rs:158-160` | as `_open` | as `_open` | unverified | none |
| tier-a | libc-symbol | `_write` | `ViSyscall::Write` | `libs/posix-shim/src/sysio.rs:502-505` | raw kernel write | kernel error value returned as `-1`; errno not set | unverified | none |
| tier-a | libc-symbol | `write` | `_write` | `libs/posix-shim/src/sysio.rs:509-512` | as `_write` | as `_write` | unverified | none |
| tier-a | libc-symbol | `_read` | `ViSyscall::Read` | `libs/posix-shim/src/sysio.rs:516-519` | raw kernel read | kernel error value returned as `-1`; errno not set | unverified | none |
| tier-a | libc-symbol | `read` | `_read` | `libs/posix-shim/src/sysio.rs:523-526` | as `_read` | as `_read` | unverified | none |
| tier-a | libc-symbol | `_lseek` | `ViSyscall::Seek` | `libs/posix-shim/src/sysio.rs:530-540` | signed offset and whence passed through | kernel error value returned; errno not set | unverified | none |
| tier-a | libc-symbol | `lseek` | `_lseek` | `libs/posix-shim/src/sysio.rs:544-547` | as `_lseek` | as `_lseek` | unverified | none |
| tier-a | libc-symbol | `_fstat` | `ViSyscall::Fstat` (ViFstatV1) | `libs/posix-shim/src/sysio.rs:567-598` | only `st_mode` (character, regular or directory) and `st_size` are set; other fields are zero | `-1` on `NULL`, bad fd or bad wire record; caller buffer left unchanged; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_fstat`; `cells/tests/posix-shim-test/src/fstat.rs::test_fstat`; `docs/evidence/posix-fstat-verification.txt:25-34` |
| tier-a | libc-symbol | `fstat` | `_fstat` | `libs/posix-shim/src/sysio.rs:604-607` | as `_fstat` | as `_fstat` | unverified | none |
| tier-a | libc-symbol | `_stat` | typed VFS `Stat` IPC, or `StatAt` below the installed working-directory handle (`posix/vfs_at.rs`); falls back to `_open` plus `_fstat` | `libs/posix-shim/src/sysio.rs:616-636` | `st_mode` (regular or directory) and `st_size`; with a working directory installed, paths resolve below it | `-1` on `NULL` or a missing path; caller buffer left unchanged; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_stat` |
| tier-a | libc-symbol | `stat` | `_stat` | `libs/posix-shim/src/sysio.rs:643-645` | as `_stat` | as `_stat` | unverified | none |
| tier-a | libc-symbol | `_unlink` | typed VFS `Unlink` IPC, or `UnlinkAt` below the installed working-directory handle (`posix/vfs_at.rs`) (`VfsMutate`) | `libs/posix-shim/src/sysio.rs:471-481` | UTF-8 path; VFS authority applies; with a working directory installed, paths resolve below it | `-1` on `NULL`, non-UTF-8 path, a `..` or out-of-working-directory path, VFS refusal or transport error; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_unlink` |
| tier-a | libc-symbol | `unlink` | `_unlink` | `libs/posix-shim/src/sysio.rs:486-488` | as `_unlink` | as `_unlink` | unverified | none |
| tier-a | libc-symbol | `_mkdir` | typed VFS `Mkdir` IPC, or `MkdirAt` below the installed working-directory handle (`posix/vfs_at.rs`) (`VfsMutate`) | `libs/posix-shim/src/sysio.rs:392-402` | UTF-8 path; caller mode bits ignored; with a working directory installed, paths resolve below it | `-1` on `NULL`, non-UTF-8 path, a `..` or out-of-working-directory path, VFS refusal or transport error; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_mkdir_rmdir` |
| tier-a | libc-symbol | `mkdir` | `_mkdir` | `libs/posix-shim/src/sysio.rs:407-409` | as `_mkdir` | as `_mkdir` | unverified | none |
| tier-a | libc-symbol | `_rmdir` | typed VFS `Rmdir` IPC, or `RmdirAt` below the installed working-directory handle (`posix/vfs_at.rs`) (`VfsMutate`) | `libs/posix-shim/src/sysio.rs:414-424` | UTF-8 path; with a working directory installed, paths resolve below it | `-1` on `NULL`, non-UTF-8 path, a `..` or out-of-working-directory path, VFS refusal or transport error; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_mkdir_rmdir` |
| tier-a | libc-symbol | `rmdir` | `_rmdir` | `libs/posix-shim/src/sysio.rs:429-431` | as `_rmdir` | as `_rmdir` | unverified | none |
| tier-a | libc-symbol | `_rename` | typed VFS `Rename` IPC, or `RenameAt` below the installed working-directory handle (`posix/vfs_at.rs`) (`VfsMutate`) | `libs/posix-shim/src/sysio.rs:363-378` | UTF-8 paths; with a working directory installed, both resolve below it | `-1` on `NULL`, non-UTF-8 path, a `..` or out-of-working-directory path, VFS refusal or transport error; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_rename` |
| tier-a | libc-symbol | `rename` | `_rename` | `libs/posix-shim/src/sysio.rs:383-385` | as `_rename` | as `_rename` | unverified | none |
| tier-a | libc-symbol | `_putchar` | `ViSyscall::Write` to fd 1 | `libs/posix-shim/src/sysio.rs:490-494` | one byte to stdout | failure not reported | unverified | none |
| tier-a | libc-symbol | `_isatty` | constant | `libs/posix-shim/src/sysio.rs:696-703` | fds 0-2 report 1; all others 0 | errno not set | unverified | none |
| tier-a | libc-symbol | `_getpid` | constant | `libs/posix-shim/src/sysio.rs:707-710` | always 1 | not a Cell identity | unverified | none |
| tier-a | libc-symbol | `_times` | none | `libs/posix-shim/src/sysio.rs:236-245` | zeroes the `tms` fields and returns 0 | no CPU accounting | unverified | none |
| tier-a | libc-symbol | `_exit` | `ViSyscall::Exit` | `libs/posix-shim/src/sysio.rs:722-728` | terminates the Cell with a status | never returns | unverified | none |
| tier-a | libc-symbol | `_time` | `ViSyscall::GetTime` epoch seconds | `libs/posix-shim/src/sysio.rs:737-745` | whole seconds | returns 0 when the time syscall fails; errno not set | unverified | none |
| tier-a | libc-symbol | `_gettimeofday` | `ViSyscall::GetTime` epoch seconds | `libs/posix-shim/src/sysio.rs:750-760` | `tv_usec` always 0; timezone ignored | returns 0 even when the time syscall fails, leaving `tv` unwritten | unverified | none |
| tier-a | libc-symbol | `_fcntl` | none | `libs/posix-shim/src/sysio.rs:166-168` | none | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_execve` | none | `libs/posix-shim/src/sysio.rs:173-180` | none; no exec in SAS | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_fork` | none | `libs/posix-shim/src/sysio.rs:184-186` | none; no fork in SAS | `-1` always; errno not set (not `ENOSYS`) | unverified | none |
| tier-a | libc-symbol | `fork` | `_fork` | `libs/posix-shim/src/sysio.rs:191-193` | none | `-1` always; errno not set (not `ENOSYS`) | unverified | none |
| tier-a | libc-symbol | `dlopen` | none | `libs/posix-shim/src/sysio.rs:199-201` | none; static linking only | `NULL` always; errno not set and no `dlerror` | unverified | none |
| tier-a | libc-symbol | `mprotect` | none | `libs/posix-shim/src/sysio.rs:206-208` | none; W^X is fixed by the loader | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `mmap` | none | `libs/posix-shim/src/sysio.rs:214-224` | none, including `MAP_ANONYMOUS` | `NULL` always, not `MAP_FAILED`; errno not set | unverified | none |
| tier-a | libc-symbol | `_wait` | none | `libs/posix-shim/src/sysio.rs:228-230` | none | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_link` | none | `libs/posix-shim/src/sysio.rs:249-251` | none | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_kill` | none | `libs/posix-shim/src/sysio.rs:714-717` | none; no signals in SAS | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_sbrk` | none; the Rust allocator owns the heap | `libs/posix-shim/src/sysio.rs:766-769` | none | `NULL` always, not `(void*)-1`; errno not set | unverified | none |

## Tier B: mlibc `sysdeps/vicell`

Opt-in with `api = { features = ["mlibc"] }` plus `mlibc-shim`. The mlibc build
enables its `posix` option set and keeps `linux`, `glibc` and `bsd` off
(`third_party/mlibc/sysdeps/vicell/meson.build:9-18`). mlibc's own `libc.a`
exports the consumer-visible libc functions. Cellos supplies the sysdep hooks
listed here, each a `Sysdeps<Tag>::operator()` named by its tag
(`include/mlibc/sysdeps.hpp`), for riscv64, aarch64 and x86_64, plus one
extension, `cellos_fd_provider_register`. A tag not listed returns `ENOSYS`
through mlibc's `sysdep_or_enosys`. Hooks report failure by returning an errno
value to mlibc, which then sets `errno` for its caller.

**Start-up.** ostd's `_start` calls `__cellos_libc_start(entry_sp)` with the
kernel's Cell initial stack (argc, argv, envp, auxv); mlibc's strong definition
(`third_party/mlibc/sysdeps/vicell/generic/startup.cpp`) sets up the Tcb, static
TLS, the stack guard and `environ`, then runs the constructors once (runtime plan
CP1 `cp1-cell-initial-stack-auxv.md`; mlibc pin patch
`third_party/mlibc/patches/0001-static-start-honours-at-phdr.patch`). Rows marked
`partial` were exercised on x86_64 QEMU (TCG and KVM) by the witnesses below;
riscv64 and aarch64 are not run.

| profile | kind | sysdep | consumer_visible_symbol | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|---|
| tier-b | mlibc-sysdep | `LibcLog` | none; mlibc internal logging | `VI_SYS_LOG` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:50-52` | ASCII message | n/a | unverified | none |
| tier-b | mlibc-sysdep | `LibcPanic` | none; mlibc internal assertion path | log then `VI_SYS_EXIT` with status 1 | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:55-59` | terminates the calling task | never returns | unverified | none |
| tier-b | mlibc-sysdep | `Exit` | `exit`, `_exit` [INFERENCE: mlibc dispatch] | `VI_SYS_EXIT` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:61-64` | from the root thread, ends the Cell | from a worker thread, ends only that thread | unverified | none |
| tier-b | mlibc-sysdep | `TcbSet` | none; TLS setup at start-up and per thread | `SetTlsBase` (9) with mlibc's Linux offsets: Tcb + `sizeof(Tcb)` on RV64, Tcb + `sizeof(Tcb)` − 16 on AArch64, Tcb on x86_64 | `third_party/mlibc/sysdeps/vicell/generic/thread.cpp:49-59` | the kernel reinstalls the base on every resume | kernel refusal gives `EINVAL` | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:608` |
| tier-b | mlibc-sysdep | `FutexWait` | `pthread_mutex_*`, `pthread_cond_*`, `pthread_join` [INFERENCE: mlibc dispatch] | `FutexWait` (17) | `third_party/mlibc/sysdeps/vicell/generic/thread.cpp:65-87` | timeout rounded up to 10 ms ticks plus one, so never early; a zero timeout is answered without the kernel | `EAGAIN` on value mismatch, `ETIMEDOUT`, `EINVAL` for a bad timespec or address | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:606-609` |
| tier-b | mlibc-sysdep | `FutexWake` | as `FutexWait` | `FutexWake` (18); one waiter or all | `third_party/mlibc/sysdeps/vicell/generic/thread.cpp:89-93` | keyed by address space, generation and address | `EINVAL` for a bad address | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:606-607` |
| tier-b | mlibc-sysdep | `Yield` | `sched_yield` [INFERENCE: mlibc dispatch] | `Yield` (104) | `third_party/mlibc/sysdeps/vicell/generic/thread.cpp:95-97` | gives up the time slice | n/a | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:606` |
| tier-b | mlibc-sysdep | `GetTid` | none; the root thread's tid before its Tcb exists | constant `0x3fffffff` | `third_party/mlibc/sysdeps/vicell/generic/thread.cpp:99-101` | inside mlibc's 30-bit futex owner mask; Spawn tids are small and never reused | n/a | unverified | none |
| tier-b | mlibc-sysdep | `PrepareStack` | `pthread_create` [INFERENCE: mlibc dispatch] | start block (entry, arg, Tcb) for `__vicell_start_thread` | `third_party/mlibc/sysdeps/vicell/generic/thread.cpp:103-130` | a caller stack (`pthread_attr_setstack`) is used; otherwise the kernel's 64 KiB Spawn stack, reported as the stack size | a caller stack under 4 KiB gives `EINVAL`; `ENOMEM` from the arena | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:606` |
| tier-b | mlibc-sysdep | `Clone` | `pthread_create` [INFERENCE: mlibc dispatch] | `Spawn` (5) at `__vicell_start_thread` | `third_party/mlibc/sysdeps/vicell/generic/thread.cpp:132-138` | the thread shares the Cell's address space, authority and quota; 32 threads per Cell | `EAGAIN` when the kernel refuses | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:606` |
| tier-b | mlibc-sysdep | `ThreadExit` | `pthread_exit`, thread return [INFERENCE: mlibc dispatch] | `VI_SYS_EXIT` | `third_party/mlibc/sysdeps/vicell/generic/thread.cpp:141-144` | ends only the calling thread | never returns | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:606` |
| tier-b | mlibc-sysdep | `Open` | `open`, `fopen` [INFERENCE: mlibc dispatch] | a path under a registered fd provider prefix goes to that provider; any other path to `VI_SYS_OPEN` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:93-95`; `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:67-73`; `third_party/mlibc/sysdeps/vicell/include/vicell/fd_provider_core.h:132-180` | path, flags and mode passed through; provider fds come from the reserved range `0x100000`-`0x10003f` | kernel failure maps to `ENOENT`; a kernel fd at or above `0x100000` is closed and gives `EMFILE`; no free provider slot gives `EMFILE`; provider errors pass through | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:612` |
| tier-b | mlibc-sysdep | `Read` | `read`, `fread` [INFERENCE: mlibc dispatch] | `VI_SYS_READ` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:97-102` | raw read; provider fds are not routed | kernel failure maps to `EIO` | unverified | none |
| tier-b | mlibc-sysdep | `Write` | `write`, `printf`, `fwrite` [INFERENCE: mlibc dispatch] | `VI_SYS_WRITE` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:104-109` | raw write; provider fds are not routed | kernel failure maps to `EIO` | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:577-580` |
| tier-b | mlibc-sysdep | `Close` | `close`, `fclose` [INFERENCE: mlibc dispatch] | provider fd: the provider's `close`, run once after the last in-flight op on the fd; otherwise `VI_SYS_CLOSE` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:111-113`; `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:75-78`; `third_party/mlibc/sysdeps/vicell/include/vicell/fd_provider_core.h:220-236` | the fd number is released at once | kernel failure maps to `EBADF`; a provider fd not open gives `EBADF`; a deferred provider close reports 0 | unverified | none |
| tier-b | mlibc-sysdep | `Ioctl` | `posix_devctl`, `tcgetpgrp`, `tcsetpgrp`, `tcgetsid`; `ioctl` from libdrm-lite (note below the table) [INFERENCE: mlibc dispatch] | provider fd: the provider's `ioctl` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:116-118`; `third_party/mlibc/sysdeps/vicell/include/vicell/fd_provider_core.h:253-268` | the request is cut to its 32-bit Linux number (undoes `posix_devctl`'s `int` sign extension); the provider sets the result | a kernel fd gives `ENOTTY` (the kernel has no ioctl); a provider fd not open gives `EBADF` | unverified | none |
| tier-b | mlibc-sysdep | `Seek` | `lseek`, `fseek` [INFERENCE: mlibc dispatch] | `VI_SYS_SEEK` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:120-125` | offset and whence passed through | kernel failure maps to `ESPIPE` | unverified | none |
| tier-b | mlibc-sysdep | `ClockGet` | `clock_gettime` [INFERENCE: mlibc dispatch] | `VI_SYS_GETTIME`: op 2 for realtime ns; monotonic from op 0 × 100 (riscv64, 10 MHz), op 1 × 10⁶ (aarch64, ms), op 0 (x86_64, ns) | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:144-157` | `CLOCK_REALTIME` and `CLOCK_MONOTONIC` only; aarch64 monotonic has 1 ms resolution | other clocks give `EINVAL`; a negative time gives `EIO` | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:579` |
| tier-b | mlibc-sysdep | `Sleep` | `nanosleep`, `clock_nanosleep`, `sleep` [INFERENCE: mlibc dispatch] | timed `FutexWait` on a private word, re-armed against `CLOCK_MONOTONIC` | `third_party/mlibc/sysdeps/vicell/generic/posix.cpp:37-57` | never returns early; up to one 10 ms tick late; not interruptible (remaining time always 0) | `EINVAL` for a bad timespec; `EIO` if the clock fails | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:611` |
| tier-b | mlibc-sysdep | `AnonAllocate` | `malloc` through mlibc's slab allocator [INFERENCE: mlibc dispatch] | static 16 MiB bump arena, CAS-advanced (interim until R3b) | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:161-175` | 16-byte aligned, zeroed, thread-safe | `ENOMEM` when the arena is exhausted | partial | `docs/evidence/mlibc-runtime-x86-tcg.log:577` |
| tier-b | mlibc-sysdep | `AnonFree` | `free` through the slab allocator [INFERENCE: mlibc dispatch] | no-op | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:177-179` | none | memory is never reclaimed | unverified | none |
| tier-b | mlibc-sysdep | `VmMap` | `mmap` [INFERENCE: mlibc dispatch] | `AnonAllocate` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:182-185` | `MAP_ANONYMOUS` only; provider `mmap` is not routed until R3b | any other mapping, including a provider fd, gives `EINVAL` | unverified | none |
| tier-b | mlibc-sysdep | `VmUnmap` | `munmap` [INFERENCE: mlibc dispatch] | `AnonFree` (no-op) | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:187-189` | none | memory is never reclaimed | unverified | none |
| tier-b | mlibc-sysdep | `Isatty` | `isatty` and stdio buffering choice [INFERENCE: mlibc dispatch] | constant | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:192-194` | fds 0-2 are terminals (returns 0) | other fds return `ENOTTY` | unverified | none |
| tier-b | mlibc-sysdep | `Stat` | `fstat`, `stat`, `fstatat(AT_FDCWD, …)` [INFERENCE: mlibc dispatch] | provider fd: the provider's `fstat`; kernel fd: `VI_SYS_FSTAT` (ViFstatV1 wire record); by path through `Open`, then either, then `Close` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:204-232` | kernel files set only `st_mode` (character, regular or directory) and `st_size`; a provider fills what it serves | `EINVAL` for `NULL` or a directory fd other than `AT_FDCWD`; `EBADF` on a bad fd or short record; `EIO` on an unknown kind; otherwise the `Open` or provider error | unverified | none |
| tier-b | mlibc-sysdep | `GetPid` | `getpid` [INFERENCE: mlibc dispatch] | constant 1 | `third_party/mlibc/sysdeps/vicell/generic/posix.cpp:61-63` | a Cell is one process; no process hierarchy | n/a | unverified | none |
| tier-b | mlibc-sysdep | `Sysconf` | `sysconf` [INFERENCE: mlibc dispatch] | `_SC_NPROCESSORS_*` = 1 (no hart-count syscall); `_SC_PHYS_PAGES`/`_SC_AVPHYS_PAGES` from `MemInfo` (243) | `third_party/mlibc/sysdeps/vicell/generic/posix.cpp:66-88` | other names use mlibc's defaults | `-1`/`ENOSYS` for the page counts without the `MemInfo` allowlist bit | unverified | none |
| tier-b | mlibc-sysdep | `GetEntropy` | `getentropy`, `getrandom` [INFERENCE: mlibc dispatch] | `GetRandom` (214), 64 bytes per call | `third_party/mlibc/sysdeps/vicell/generic/posix.cpp:91-104` | at most 256 bytes | `EIO` without an RNG device or above 256 bytes | unverified | none |

Two Tier-B entry points are not mlibc sysdeps and so have no table row:
- `cellos_fd_provider_register` (`third_party/mlibc/sysdeps/vicell/include/vicell/fd_provider.h`, not installed in the sysroot; registry in `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:236-239` over `fd_provider_core.h`): up to 4 absolute path prefixes ending in `/`, 64 provider fds, permanent. `open`/`close`/`ioctl`/`fstat` are routed; `mmap`/`munmap` are in the v1 ops table but not routed until R3b. Errors: `EINVAL` for a bad prefix or ops table, `EEXIST` when prefixes nest or repeat, `ENOSPC` past 4 providers. Unverified.
- `ioctl` from libdrm-lite (`libs/drm-cellos/compat/sys/ioctl.h`, `libs/drm-cellos/c/ioctl.c`): one pointer argument, through `posix_devctl` and then the `Ioctl` sysdep; `-1` with `errno` from `Ioctl`. mlibc's own `ioctl` needs its `glibc` option, which stays off. Unverified.

`cells/tests/mlibc-smoke` exercises mlibc `malloc`/`free`, `printf` and
`clock_gettime` (`cells/tests/mlibc-smoke/src/main.rs:60-114`);
`cells/tests/mlibc-posix-x86` (x86_64) exercises threads, mutexes, `pthread_once`,
timed condition waits, `clock_nanosleep`, TLS and `errno` per thread, and
`dl_iterate_phdr` with a build-id note (`cells/tests/mlibc-posix-x86/c/witness.c`).
`scripts/x86/qemu-mlibc-witness.sh` runs both from the shell of the image built by
`scripts/build-x86_64-mlibc-witness-ci.sh`; the logs and their summary are
`docs/evidence/mlibc-runtime-x86.txt`.

The fd provider rows (`Open`, `Close`, `Ioctl`, `Stat`, `cellos_fd_provider_register`
and libdrm-lite's `ioctl`; runtime plan R6) have no runtime witness yet, so they
stay `unverified` where no older evidence covers them. The same routing core
(`include/vicell/fd_provider_core.h`) is host-tested by
`libs/drm-cellos/tests/host/run.sh`, a C suite on Linux, which is not an evidence
anchor type. The QEMU witness `cells/tests/drm-lite-x86` (runner
`cells/tests/drm-lite-x86/run-qemu-x86.sh`) waits for its `/bin/drm-lite-x86`
launch row.

### Tier B: C++ runtime for `cpp-hosted`

A `cpp-hosted` Cell (ADR-0034) links LLVM libc++, libc++abi and libunwind
(`llvmorg-19.1.7`, `scripts/build-llvm-runtimes.sh`) beside mlibc. The C++ ABI
symbols come from those archives, not from Cellos code. The Tier-A C++ ABI layer
is compiled out under `mlibc`. The rows below record the provider and how it
reaches mlibc. They are not sysdep rows, and `scripts/check-posix-ledger.py`
does not parse them. All are `unverified`: the witness
`cells/tests/cpp-hosted-smoke` passes on QEMU TCG (2026-11-05, five runs per boot;
log in `.agents/261104-0802-x86-tier2-gpu-runtime/reports/r4-evidence/`, which is not
a `docs/evidence/` anchor, so the rows stay `unverified`). KVM is not yet run.

| symbol group | provider | backing in mlibc | semantic_subset | unsupported_result | status |
|---|---|---|---|---|---|
| `__cxa_throw`, `__cxa_begin_catch`, `__cxa_end_catch`, `__cxa_rethrow`, `__cxa_allocate_exception`, `__gxx_personality_v0`, `std::exception_ptr` | libc++abi | `malloc`; per-thread globals in static TLS | exceptions within C++ frames | an exception reaching a `noexcept` Rust→C++ entry or the end of the stack calls `std::terminate` | unverified |
| `_Unwind_RaiseException`, `_Unwind_Resume`, `_Unwind_*` | libunwind | `dl_iterate_phdr` finds `PT_GNU_EH_FRAME`; `pthread_rwlock_*` | DWARF CFI of the Cell's own image | no frame info for a Rust frame (`panic=abort`) ends the search: terminate | unverified |
| `__cxa_guard_acquire`, `__cxa_guard_release`, `__cxa_guard_abort` | libc++abi | `pthread_mutex_*`, `pthread_cond_*` | thread-safe function-local statics under contention | n/a | unverified |
| `__cxa_atexit` (static destructors) | mlibc | mlibc's exit-handler list | handlers run when C code calls `exit` | not run when the Cell ends through Rust's `Exit` (ostd `_start`) or `abort`; libc++abi provides no `__cxa_thread_atexit` for this target | unverified |
| `operator new`/`delete` (all forms, aligned and nothrow) | libc++abi | `malloc`, `aligned_alloc`, `free` (16 MiB arena until R3b) | throws `std::bad_alloc` on failure | arena exhaustion is `std::bad_alloc`; memory is never returned to the kernel | unverified |
| `std::terminate`, `abort` | libc++abi, mlibc | `fprintf(stderr)`, then `abort()` | prints `libc++abi: terminating due to uncaught exception of type …` | mlibc has no `Kill`/`Sigaction` sysdeps: logs the missing-sysdep `__ensure`, `mlibc: sigaction failed in abort`, `mlibc PANIC`, exit status 1 (seen on QEMU TCG) | unverified |
| `strtoll_l`, `strtoull_l`, `strxfrm_l`, `wcscoll_l`, `wcsxfrm_l` | mlibc pin patch `third_party/mlibc/patches/0002-posix-locale-l-functions.patch` | the unsuffixed function | C/POSIX locale only; collation is code-point order | other locales are not distinguished | unverified |

## Consumers

| consumer | adapter | note |
|---|---|---|
| `cells/tests/posix-shim-test` | Tier A or Tier B, whichever the build selects | Declares the C ABI directly (`cells/tests/posix-shim-test/src/main.rs:56-75`). Witness for the rows that cite it. |
| `cells/tests/mlibc-smoke`, `cells/tests/mlibc-posix-x86` | Tier B | Runtime witnesses on x86_64 QEMU (`docs/evidence/mlibc-runtime-x86.txt`). |
| `cells/tests/drm-lite-x86`, `libs/drm-cellos` | Tier B | libdrm-lite (`third_party/libdrm`) and the fd provider route against the test-only fake provider; QEMU witness pending its launch row. |
| `cells/tests/cpp-hosted-smoke` | Tier B plus the `cpp-hosted` C++ runtime | x86_64 witness, passing on QEMU TCG (`scripts/x86/qemu-cpp-hosted-smoke.sh`); KVM not yet run. |
| `cells/demos/tetris-c` | Tier A | Class-A reference C port. |
| `cells/runtimes/lua`, `cells/demos/doom`, `cells/demos/tetris-lua` | Tier A, plus toolchain picolibc on RV64 | The picolibc overlap is outside this ledger (see Scope). |
| `cells/services/ocel-quickjs`, `cells/services/ocel-pdf` | Tier A | Use the allocation, string, stdio and math rows. |
