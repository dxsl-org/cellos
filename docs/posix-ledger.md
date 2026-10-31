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
  compiles out every Tier A module (`libs/api/src/services/posix.rs:24-46`), and
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
| Native transport | `ViSyscall` plus typed IPC (VFS, Net and other services) through `api`/`ostd`. This is the native SAS ABI every Cell uses. | Native ABI; no POSIX names | `libs/api/src/abi/syscall.rs`, `libs/api/src/services/ipc.rs` |
| POSIX adapters | Optional userspace C ABI translation onto native transport: Tier A Rust shim or Tier B mlibc sysdeps. | This ledger | `libs/api/src/services/posix/`, `third_party/mlibc/sysdeps/vicell/` |
| Linux VM | Tier 3 `linux-guest`: a Linux kernel and userspace in a VM run by the Hypervisor Cell, with VirtIO devices forwarded to Cellos services. Linux syscalls are served by the guest kernel, not by Cellos. | Spec 05 §4 | `docs/specs/05-application.md` §4.2 |

## Tier A: `api::services::posix` (Rust C-ABI shim)

The adapter is active on bare-metal RV64, AArch64 and x86_64 (and wasm32) when
`mlibc` is off (`libs/api/src/services/posix.rs:13-46`). One row covers one symbol,
or one `libc-symbol-family` of C99 math functions with identical backing.

### Allocation and C++ ABI

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `malloc` | Rust global allocator via `malloc_impl`; size header | `libs/api/src/services/posix/alloc.rs:28-31` | any size; 16-byte aligned | `NULL` on size overflow or allocator failure; errno not set | unverified | none |
| tier-a | libc-symbol | `free` | `free_impl` | `libs/api/src/services/posix/alloc.rs:39-42` | pointers from this allocator; `NULL` is a no-op | pointer without the shim header magic is silently ignored | unverified | none |
| tier-a | libc-symbol | `realloc` | `realloc_impl` | `libs/api/src/services/posix/alloc.rs:51-54` | `NULL` pointer acts as `malloc`; size 0 frees and returns `NULL` | `NULL` on foreign pointer, overflow or allocator failure; errno not set | unverified | none |
| tier-a | libc-symbol | `calloc` | `malloc` then zero fill | `libs/api/src/services/posix/alloc.rs:62-73` | zero-filled block | `NULL` on `nmemb*size` overflow or allocator failure; errno not set | unverified | none |
| tier-a | cxx-abi-symbol | `_Znwm` | `malloc_impl` | `libs/api/src/services/posix/alloc.rs:147-150` | `operator new(size_t)` | returns `NULL`; never throws `std::bad_alloc` | unverified | none |
| tier-a | cxx-abi-symbol | `_ZdlPv` | `free_impl` | `libs/api/src/services/posix/alloc.rs:157-160` | `operator delete(void*)` | as `free` | unverified | none |
| tier-a | cxx-abi-symbol | `_ZdlPvm` | `free_impl` | `libs/api/src/services/posix/alloc.rs:169-172` | sized `operator delete`; size ignored | as `free` | unverified | none |
| tier-a | cxx-abi-symbol | `_Znam` | `malloc_impl` | `libs/api/src/services/posix/alloc.rs:180-183` | `operator new[](size_t)` | returns `NULL`; never throws | unverified | none |
| tier-a | cxx-abi-symbol | `_ZdaPv` | `free_impl` | `libs/api/src/services/posix/alloc.rs:190-193` | `operator delete[](void*)` | as `free` | unverified | none |
| tier-a | cxx-abi-symbol | `_ZdaPvm` | `free_impl` | `libs/api/src/services/posix/alloc.rs:202-205` | sized `operator delete[]`; size ignored | as `free` | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_pure_virtual` | `ViSyscall::Log` then `ViSyscall::Exit` | `libs/api/src/services/posix/cxxabi.rs:19-30` | logs and terminates the Cell | never returns | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_guard_acquire` | caller guard word | `libs/api/src/services/posix/cxxabi.rs:42-50` | single-threaded only: 1 when the guard is 0, else 0 | no locking; concurrent initialisation unprotected | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_guard_release` | caller guard word | `libs/api/src/services/posix/cxxabi.rs:56-59` | marks the guard initialised | n/a | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_guard_abort` | none | `libs/api/src/services/posix/cxxabi.rs:67-70` | leaves the guard at 0 so a later call retries | n/a | unverified | none |
| tier-a | libc-symbol | `atexit` | none | `libs/api/src/services/posix/cxxabi.rs:85-88` | returns 0 | handler is never run; silent divergence | unverified | none |
| tier-a | cxx-abi-symbol | `__cxa_atexit` | none | `libs/api/src/services/posix/cxxabi.rs:94-101` | returns 0 | destructor is never run; silent divergence | unverified | none |
| tier-a | libc-symbol | `abort` | `ViSyscall::Exit` with status 134 | `libs/api/src/services/posix/cxxabi.rs:109-115` | terminates the Cell | no `SIGABRT`; no signal handlers | unverified | none |

### Memory and string functions

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `memcpy` | Rust byte loop | `libs/api/src/services/posix/strings.rs:13-14` | non-overlapping copy | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `memmove` | Rust byte loop | `libs/api/src/services/posix/strings.rs:28-29` | overlap-safe copy | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `memset` | Rust byte loop | `libs/api/src/services/posix/strings.rs:50-51` | fill bytes | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `memcmp` | Rust byte loop | `libs/api/src/services/posix/strings.rs:64-65` | byte compare | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strlen` | Rust byte loop | `libs/api/src/services/posix/strings.rs:82-83` | NUL-terminated length | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strcpy` | Rust byte loop | `libs/api/src/services/posix/strings.rs:96-97` | copy with terminator | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strncpy` | Rust byte loop | `libs/api/src/services/posix/strings.rs:117-118` | bounded copy | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strcmp` | Rust byte loop | `libs/api/src/services/posix/strings.rs:139-140` | byte-wise compare; no locale | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strncmp` | Rust byte loop | `libs/api/src/services/posix/strings.rs:160-161` | bounded byte-wise compare | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strcat` | `strlen` plus `strcpy` | `libs/api/src/services/posix/strings.rs:181-182` | append | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strchr` | Rust byte loop | `libs/api/src/services/posix/strings.rs:194` | first byte match | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `strrchr` | Rust byte loop | `libs/api/src/services/posix/strings.rs:215` | last byte match | no error path; C pointer preconditions | unverified | none |
| tier-a | libc-symbol | `memchr` | Rust byte loop | `libs/api/src/services/posix/strings.rs:236` | bounded byte search | no error path; C pointer preconditions | unverified | none |

### Entropy and non-local jumps

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `getentropy` | `ViSyscall::GetRandom` in chunks of up to 64 bytes | `libs/api/src/services/posix/entropy.rs:18-34` | `buflen` at most 256 | `-1` for `NULL` buffer, `buflen` over 256 or syscall failure; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_getentropy`; `cells/tests/posix-shim-test/src/main.rs::test_getentropy` |
| tier-a | libc-symbol | `arc4random_buf` | `getentropy` | `libs/api/src/services/posix/entropy.rs:42-45` | fills up to 256 bytes | requests over 256 bytes are silently truncated; failure is not reported | unverified | none |
| tier-a | libc-symbol | `setjmp` | naked assembly per target: RV64, AArch64 hard-float, AArch64 soft-float, x86_64 | `libs/api/src/services/posix/setjmp.rs:38`; `libs/api/src/services/posix/setjmp.rs:121`; `libs/api/src/services/posix/setjmp.rs:180`; `libs/api/src/services/posix/setjmp.rs:236`; `libs/api/src/services/posix/setjmp.rs:288` | saves callee-saved registers; one definition per target | other targets: returns 0 and saves nothing | unverified | none |
| tier-a | libc-symbol | `longjmp` | naked assembly per target, as `setjmp` | `libs/api/src/services/posix/setjmp.rs:76`; `libs/api/src/services/posix/setjmp.rs:144`; `libs/api/src/services/posix/setjmp.rs:209`; `libs/api/src/services/posix/setjmp.rs:258`; `libs/api/src/services/posix/setjmp.rs:298` | restores the saved context; `val` 0 becomes 1 | other targets: spins forever and never returns | unverified | none |

### C99 math

All math entry points call the Rust `libm` crate directly. They make no syscall
and never set `errno` or floating-point exception flags.

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol-family | `sin, cos, tan, asin, acos, atan, sinh, cosh, tanh, asinh, acosh, atanh, exp, exp2, expm1, log, log2, log10, log1p, sqrt, cbrt, floor, ceil, round, trunc, rint, fabs` | Rust `libm` (f64) | `libs/api/src/services/posix/math.rs:53-79` | libm results | no errno or `math_errhandling`; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `atan2, pow, hypot, fmod, remainder, fmin, fmax, copysign, nextafter` | Rust `libm` (f64) | `libs/api/src/services/posix/math.rs:97-105` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `fma, scalbn, ldexp, ilogb` | Rust `libm` (f64) | `libs/api/src/services/posix/math.rs:111-129` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `modf, frexp` | Rust `libm` (f64) | `libs/api/src/services/posix/math.rs:136-157` | libm results; a `NULL` out-pointer is tolerated and not written | no errno | unverified | none |
| tier-a | libc-symbol | `nearbyint` | `libm::rint` | `libs/api/src/services/posix/math.rs:81-85` | round to nearest | no floating-point environment; inexact state is not modelled | unverified | none |
| tier-a | libc-symbol | `logb` | `libm::ilogb` cast to double | `libs/api/src/services/posix/math.rs:87-91` | finite non-zero input | 0, infinity and NaN yield the integer `ilogb` sentinel as a double, not -inf, +inf or NaN [INFERENCE: libm sentinel values] | unverified | none |
| tier-a | libc-symbol-family | `sinf, cosf, tanf, asinf, acosf, atanf, sinhf, coshf, tanhf, asinhf, acoshf, atanhf, expf, exp2f, expm1f, logf, log2f, log10f, log1pf, sqrtf, cbrtf, floorf, ceilf, roundf, truncf, rintf, fabsf` | Rust `libm` (f32) | `libs/api/src/services/posix/math.rs:163-189` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `atan2f, powf, hypotf, fmodf, remainderf, fminf, fmaxf, copysignf, nextafterf` | Rust `libm` (f32) | `libs/api/src/services/posix/math.rs:204-212` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `fmaf, scalbnf, ldexpf, ilogbf` | Rust `libm` (f32) | `libs/api/src/services/posix/math.rs:218-236` | libm results | no errno; no FP exception flags | unverified | none |
| tier-a | libc-symbol-family | `modff, frexpf` | Rust `libm` (f32) | `libs/api/src/services/posix/math.rs:243-264` | libm results; a `NULL` out-pointer is tolerated | no errno | unverified | none |
| tier-a | libc-symbol | `nearbyintf` | `libm::rintf` | `libs/api/src/services/posix/math.rs:191-194` | round to nearest | no floating-point environment | unverified | none |
| tier-a | libc-symbol | `logbf` | `libm::ilogbf` cast to float | `libs/api/src/services/posix/math.rs:195-198` | finite non-zero input | as `logb` | unverified | none |

### stdio

The shim keeps no stream buffer or error state. A `FILE` holds only a descriptor.
Formatting uses `vsnprintf_core` (`libs/api/src/services/posix/stdio_fmt.rs:181-407`),
which supports the flags `-` and `0`, a decimal width and precision (no `*`), the
length modifiers `l`, `ll`, `h`, `hh`, `z` and `t`, and the conversions
`% c s d i u x X o p f e g E G`. `e`, `g`, `E` and `G` print fixed-point output.
An unknown conversion is printed literally.

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | c-global | `stdout` | static `FILE` for fd 1 | `libs/api/src/services/posix/stdio.rs:38-39` | descriptor 1 | n/a | unverified | none |
| tier-a | c-global | `stderr` | static `FILE` for fd 2 | `libs/api/src/services/posix/stdio.rs:40-41` | descriptor 2 | n/a | unverified | none |
| tier-a | c-global | `stdin` | static `FILE` for fd 0 | `libs/api/src/services/posix/stdio.rs:42-43` | descriptor 0 | n/a | unverified | none |
| tier-a | libc-symbol | `vsnprintf` | `vsnprintf_core` | `libs/api/src/services/posix/stdio.rs:81-89` | bounded format, conversion subset above | unsupported conversion printed literally; no error return | unverified | none |
| tier-a | libc-symbol | `vsprintf` | `vsnprintf_core` | `libs/api/src/services/posix/stdio.rs:98-101` | unbounded format, conversion subset above | as `vsnprintf` | unverified | none |
| tier-a | libc-symbol | `vprintf` | `vsnprintf_core` then `_write(1)` | `libs/api/src/services/posix/stdio.rs:108-114` | output truncated to 1024 bytes | write failure not reported; returns formatted length | unverified | none |
| tier-a | libc-symbol | `vfprintf` | `vsnprintf_core` then `_write` to the stream fd | `libs/api/src/services/posix/stdio.rs:123-134` | output truncated to 1024 bytes | write failure not reported | unverified | none |
| tier-a | libc-symbol | `printf` | `vprintf` | `libs/api/src/services/posix/stdio.rs:145-148` | as `vprintf` | as `vprintf` | unverified | none |
| tier-a | libc-symbol | `fprintf` | `vfprintf` | `libs/api/src/services/posix/stdio.rs:156-159` | as `vfprintf` | as `vfprintf` | unverified | none |
| tier-a | libc-symbol | `sprintf` | `vsprintf` | `libs/api/src/services/posix/stdio.rs:169-172` | as `vsprintf` | as `vsprintf` | unverified | none |
| tier-a | libc-symbol | `snprintf` | `vsnprintf` | `libs/api/src/services/posix/stdio.rs:182-190` | as `vsnprintf` | as `vsnprintf` | unverified | none |
| tier-a | libc-symbol | `fopen` | `_open` plus heap `FILE` | `libs/api/src/services/posix/stdio.rs:201-220` | a mode starting with `w` opens with create and truncate flags; any other mode opens read-only | `NULL` on open or allocation failure; errno not set | unverified | none |
| tier-a | libc-symbol | `fclose` | `_close`; frees a heap `FILE` | `libs/api/src/services/posix/stdio.rs:228-239` | close stream | `-1` for `NULL`; errno not set | unverified | none |
| tier-a | libc-symbol | `fread` | looping `_read` | `libs/api/src/services/posix/stdio.rs:247-274` | reads until complete, EOF or error | 0 items for `NULL` stream or buffer; no error flag | unverified | none |
| tier-a | libc-symbol | `fwrite` | single `_write` | `libs/api/src/services/posix/stdio.rs:281-298` | one write call | 0 on failure; no error flag | unverified | none |
| tier-a | libc-symbol | `fputs` | `_write` | `libs/api/src/services/posix/stdio.rs:305-312` | writes without a newline | `-1` for `NULL` string; returns the raw write result | unverified | none |
| tier-a | libc-symbol | `fputc` | `_write` | `libs/api/src/services/posix/stdio.rs:318-326` | one byte | `-1` on failure | unverified | none |
| tier-a | libc-symbol | `fgetc` | `_read` | `libs/api/src/services/posix/stdio.rs:333-343` | one byte; `NULL` stream reads fd 0 | `-1` on EOF or error (not distinguished) | unverified | none |
| tier-a | libc-symbol | `fgets` | a single `_read` | `libs/api/src/services/posix/stdio.rs:350-362` | one read of up to n-1 bytes; does not stop at a newline | `NULL` on bad arguments, EOF or error | unverified | none |
| tier-a | libc-symbol | `putchar` | `_write(1)` | `libs/api/src/services/posix/stdio.rs:368-373` | one byte to stdout | write failure not reported | unverified | none |
| tier-a | libc-symbol | `puts` | `_write(1)` | `libs/api/src/services/posix/stdio.rs:379-388` | string plus newline | `-1` for `NULL`; write failure not reported | unverified | none |
| tier-a | libc-symbol | `fseek` | `_lseek` | `libs/api/src/services/posix/stdio.rs:402-414` | `SEEK_SET`, `SEEK_CUR`, `SEEK_END` passed through | `-1` for `NULL` stream or seek failure | unverified | none |
| tier-a | libc-symbol | `ftell` | `_lseek(SEEK_CUR)` | `libs/api/src/services/posix/stdio.rs:420-426` | current offset | `-1` for `NULL` stream; otherwise the raw seek result | unverified | none |
| tier-a | libc-symbol | `rewind` | `_lseek(SEEK_SET)` | `libs/api/src/services/posix/stdio.rs:432-438` | seek to 0 | failure not reported | unverified | none |
| tier-a | libc-symbol | `ferror` | none | `libs/api/src/services/posix/stdio.rs:448-451` | always 0 | no error state is tracked | unverified | none |
| tier-a | libc-symbol | `feof` | none | `libs/api/src/services/posix/stdio.rs:456-459` | always 0 | no EOF state is tracked | unverified | none |
| tier-a | libc-symbol | `clearerr` | none | `libs/api/src/services/posix/stdio.rs:464-465` | no-op | no error state is tracked | unverified | none |
| tier-a | libc-symbol | `fflush` | none | `libs/api/src/services/posix/stdio.rs:470-473` | always 0; writes are unbuffered | n/a | unverified | none |
| tier-a | libc-symbol | `fileno` | stream descriptor | `libs/api/src/services/posix/stdio.rs:478-485` | descriptor of the stream | `-1` for `NULL`; errno not set | unverified | none |

### Sockets

Sockets are client-side only. The shim keeps a socket table of 8 slots at fds 10-17
and talks to the Net service over typed IPC (`libs/api/src/services/posix/net.rs:19-43`).

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `socket` | local socket table | `libs/api/src/services/posix/net.rs:100-109` | `AF_INET` with `SOCK_STREAM` only | `-1` for any other domain or type, or when the table is full; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::echo_round_trip` |
| tier-a | libc-symbol | `connect` | Net IPC TCP connect | `libs/api/src/services/posix/net.rs:116-121` | IPv4 `sockaddr_in` | `-1` on a bad fd, bad address or transport error; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::echo_round_trip` |
| tier-a | libc-symbol | `send` | Net IPC `TcpSend` | `libs/api/src/services/posix/net.rs:211-291` | at most 495 bytes per call; waits up to 5 s for the handshake | `-1` on `NULL`, bad fd, refused peer, timeout or transport error; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::echo_round_trip` |
| tier-a | libc-symbol | `recv` | Net IPC receive | `libs/api/src/services/posix/net.rs:297-346` | copies at most `len` bytes | `-1` on `NULL`, bad fd or transport error; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::echo_round_trip` |
| tier-a | libc-symbol | `_close` | socket fds 10-17 use the local table; other fds use `ViSyscall::Close` | `libs/api/src/services/posix/net.rs:355-361` | closes a socket or a kernel fd | `-1` on failure; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_fstat`; `cells/tests/posix-shim-test/src/fstat.rs::test_fstat` |
| tier-a | libc-symbol | `close` | `_close` | `libs/api/src/services/posix/net.rs:365-368` | as `_close` | as `_close` | unverified | none |
| tier-a | libc-symbol | `gethostbyname` | Net service resolver over IPC | `libs/api/src/services/posix/net.rs:493-551` | IPv4 only; a single address; static result storage | `NULL` for a `NULL`, empty, over-long or unresolvable name, or an unreachable service; `h_errno` not set | partial | `tests/integration/tests/boot.rs::posix_shim_net`; `cells/tests/posix-shim-test/src/main.rs::test_net_dns` |

### File, process and time

| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|
| tier-a | libc-symbol | `_open` | `ViSyscall::Open` | `libs/api/src/services/posix/sysio.rs:140-150` | path, flags and mode passed through to the kernel | `-1` on kernel failure; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_fstat`; `cells/tests/posix-shim-test/src/fstat.rs::test_fstat` |
| tier-a | libc-symbol | `open` | `_open` | `libs/api/src/services/posix/sysio.rs:154-157` | as `_open` | as `_open` | unverified | none |
| tier-a | libc-symbol | `_write` | `ViSyscall::Write` | `libs/api/src/services/posix/sysio.rs:502-505` | raw kernel write | kernel error value returned as `-1`; errno not set | unverified | none |
| tier-a | libc-symbol | `write` | `_write` | `libs/api/src/services/posix/sysio.rs:509-512` | as `_write` | as `_write` | unverified | none |
| tier-a | libc-symbol | `_read` | `ViSyscall::Read` | `libs/api/src/services/posix/sysio.rs:516-519` | raw kernel read | kernel error value returned as `-1`; errno not set | unverified | none |
| tier-a | libc-symbol | `read` | `_read` | `libs/api/src/services/posix/sysio.rs:523-526` | as `_read` | as `_read` | unverified | none |
| tier-a | libc-symbol | `_lseek` | `ViSyscall::Seek` | `libs/api/src/services/posix/sysio.rs:530-540` | signed offset and whence passed through | kernel error value returned; errno not set | unverified | none |
| tier-a | libc-symbol | `lseek` | `_lseek` | `libs/api/src/services/posix/sysio.rs:544-547` | as `_lseek` | as `_lseek` | unverified | none |
| tier-a | libc-symbol | `_fstat` | `ViSyscall::Fstat` (ViFstatV1) | `libs/api/src/services/posix/sysio.rs:567-598` | only `st_mode` (character, regular or directory) and `st_size` are set; other fields are zero | `-1` on `NULL`, bad fd or bad wire record; caller buffer left unchanged; errno not set | partial | `tests/integration/tests/boot.rs::posix_shim_fstat`; `cells/tests/posix-shim-test/src/fstat.rs::test_fstat`; `docs/evidence/posix-fstat-verification.txt:25-34` |
| tier-a | libc-symbol | `fstat` | `_fstat` | `libs/api/src/services/posix/sysio.rs:604-607` | as `_fstat` | as `_fstat` | unverified | none |
| tier-a | libc-symbol | `_stat` | typed VFS `Stat` IPC; falls back to `_open` plus `_fstat` | `libs/api/src/services/posix/sysio.rs:613-634` | `st_mode` (regular or directory) and `st_size` | `-1` on `NULL` or a missing path; caller buffer left unchanged; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_stat` |
| tier-a | libc-symbol | `stat` | `_stat` | `libs/api/src/services/posix/sysio.rs:640-643` | as `_stat` | as `_stat` | unverified | none |
| tier-a | libc-symbol | `_unlink` | typed VFS `Unlink` IPC (`VfsMutate`) | `libs/api/src/services/posix/sysio.rs:468-479` | UTF-8 path; VFS authority applies | `-1` on `NULL`, non-UTF-8 path, VFS refusal or transport error; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_unlink` |
| tier-a | libc-symbol | `unlink` | `_unlink` | `libs/api/src/services/posix/sysio.rs:483-486` | as `_unlink` | as `_unlink` | unverified | none |
| tier-a | libc-symbol | `_mkdir` | typed VFS `Mkdir` IPC (`VfsMutate`) | `libs/api/src/services/posix/sysio.rs:365-376` | UTF-8 path; caller mode bits ignored | `-1` on `NULL`, non-UTF-8 path, VFS refusal or transport error; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_mkdir_rmdir` |
| tier-a | libc-symbol | `mkdir` | `_mkdir` | `libs/api/src/services/posix/sysio.rs:380-383` | as `_mkdir` | as `_mkdir` | unverified | none |
| tier-a | libc-symbol | `_rmdir` | typed VFS `Rmdir` IPC (`VfsMutate`) | `libs/api/src/services/posix/sysio.rs:387-398` | UTF-8 path | `-1` on `NULL`, non-UTF-8 path, VFS refusal or transport error; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_mkdir_rmdir` |
| tier-a | libc-symbol | `rmdir` | `_rmdir` | `libs/api/src/services/posix/sysio.rs:402-405` | as `_rmdir` | as `_rmdir` | unverified | none |
| tier-a | libc-symbol | `_rename` | typed VFS `Rename` IPC (`VfsMutate`) | `libs/api/src/services/posix/sysio.rs:336-352` | UTF-8 paths | `-1` on `NULL`, non-UTF-8 path, VFS refusal or transport error; errno not set | unverified | `cells/tests/posix-shim-test/src/fstat.rs::test_rename` |
| tier-a | libc-symbol | `rename` | `_rename` | `libs/api/src/services/posix/sysio.rs:356-359` | as `_rename` | as `_rename` | unverified | none |
| tier-a | libc-symbol | `_putchar` | `ViSyscall::Write` to fd 1 | `libs/api/src/services/posix/sysio.rs:490-494` | one byte to stdout | failure not reported | unverified | none |
| tier-a | libc-symbol | `_isatty` | constant | `libs/api/src/services/posix/sysio.rs:696-703` | fds 0-2 report 1; all others 0 | errno not set | unverified | none |
| tier-a | libc-symbol | `_getpid` | constant | `libs/api/src/services/posix/sysio.rs:707-710` | always 1 | not a Cell identity | unverified | none |
| tier-a | libc-symbol | `_times` | none | `libs/api/src/services/posix/sysio.rs:232-241` | zeroes the `tms` fields and returns 0 | no CPU accounting | unverified | none |
| tier-a | libc-symbol | `_exit` | `ViSyscall::Exit` | `libs/api/src/services/posix/sysio.rs:722-728` | terminates the Cell with a status | never returns | unverified | none |
| tier-a | libc-symbol | `_time` | `ViSyscall::GetTime` epoch seconds | `libs/api/src/services/posix/sysio.rs:737-745` | whole seconds | returns 0 when the time syscall fails; errno not set | unverified | none |
| tier-a | libc-symbol | `_gettimeofday` | `ViSyscall::GetTime` epoch seconds | `libs/api/src/services/posix/sysio.rs:750-760` | `tv_usec` always 0; timezone ignored | returns 0 even when the time syscall fails, leaving `tv` unwritten | unverified | none |
| tier-a | libc-symbol | `_fcntl` | none | `libs/api/src/services/posix/sysio.rs:162-165` | none | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_execve` | none | `libs/api/src/services/posix/sysio.rs:169-176` | none; no exec in SAS | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_fork` | none | `libs/api/src/services/posix/sysio.rs:180-183` | none; no fork in SAS | `-1` always; errno not set (not `ENOSYS`) | unverified | none |
| tier-a | libc-symbol | `fork` | `_fork` | `libs/api/src/services/posix/sysio.rs:187-190` | none | `-1` always; errno not set (not `ENOSYS`) | unverified | none |
| tier-a | libc-symbol | `dlopen` | none | `libs/api/src/services/posix/sysio.rs:195-198` | none; static linking only | `NULL` always; errno not set and no `dlerror` | unverified | none |
| tier-a | libc-symbol | `mprotect` | none | `libs/api/src/services/posix/sysio.rs:202-205` | none; W^X is fixed by the loader | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `mmap` | none | `libs/api/src/services/posix/sysio.rs:210-220` | none, including `MAP_ANONYMOUS` | `NULL` always, not `MAP_FAILED`; errno not set | unverified | none |
| tier-a | libc-symbol | `_wait` | none | `libs/api/src/services/posix/sysio.rs:224-227` | none | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_link` | none | `libs/api/src/services/posix/sysio.rs:245-248` | none | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_kill` | none | `libs/api/src/services/posix/sysio.rs:714-717` | none; no signals in SAS | `-1` always; errno not set | unverified | none |
| tier-a | libc-symbol | `_sbrk` | none; the Rust allocator owns the heap | `libs/api/src/services/posix/sysio.rs:766-769` | none | `NULL` always, not `(void*)-1`; errno not set | unverified | none |

## Tier B: mlibc `sysdeps/vicell`

Opt-in with `api = { features = ["mlibc"] }` plus `mlibc-shim`. The mlibc build
turns off its `posix`, `linux`, `glibc` and `bsd` option sets
(`third_party/mlibc/sysdeps/vicell/meson.build:8-14`). mlibc's own `libc.a`
exports the consumer-visible libc functions. Cellos supplies only the `sys_*`
hooks listed here. Hooks report failure by returning an errno value to mlibc,
which then sets `errno` for its caller.

| profile | kind | sysdep | consumer_visible_symbol | backing | source | semantic_subset | unsupported_result | status | evidence |
|---|---|---|---|---|---|---|---|---|---|
| tier-b | mlibc-sysdep | `sys_libc_log` | none; mlibc internal logging | `VI_SYS_LOG` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:41-44` | ASCII message | n/a | unverified | none |
| tier-b | mlibc-sysdep | `sys_libc_panic` | none; mlibc internal assertion path | log then `VI_SYS_EXIT` with status 1 | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:47-51` | terminates the Cell | never returns | unverified | none |
| tier-b | mlibc-sysdep | `sys_exit` | `exit`, `_exit` [INFERENCE: mlibc dispatch] | `VI_SYS_EXIT` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:54-57` | terminates the Cell with a status | never returns | unverified | none |
| tier-b | mlibc-sysdep | `sys_tcb_set` | none; TLS setup at start-up | writes `tp` on RV64 or `TPIDR_EL0` on AArch64 | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:61-70` | RV64 and AArch64 | other targets: no-op that reports success | unverified | none |
| tier-b | mlibc-sysdep | `sys_futex_wait` | none; mlibc internal locks (pthreads disabled) | Yield loop while the value is unchanged | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:74-81` | single-threaded Cells | timeout ignored; not a blocking futex | unverified | none |
| tier-b | mlibc-sysdep | `sys_futex_wake` | none; mlibc internal locks | no-op | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:84-86` | no waiters exist | always 0 | unverified | none |
| tier-b | mlibc-sysdep | `sys_open` | `open`, `fopen` [INFERENCE: mlibc dispatch] | `VI_SYS_OPEN` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:90-97` | path, flags and mode passed through | any kernel failure maps to `ENOENT` | unverified | none |
| tier-b | mlibc-sysdep | `sys_read` | `read`, `fread` [INFERENCE: mlibc dispatch] | `VI_SYS_READ` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:100-106` | raw read | kernel failure maps to `EIO` | unverified | none |
| tier-b | mlibc-sysdep | `sys_write` | `write`, `printf`, `fwrite` [INFERENCE: mlibc dispatch] | `VI_SYS_WRITE` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:109-115` | raw write | kernel failure maps to `EIO` | unverified | none |
| tier-b | mlibc-sysdep | `sys_close` | `close`, `fclose` [INFERENCE: mlibc dispatch] | `VI_SYS_CLOSE` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:118-122` | close a kernel fd | kernel failure maps to `EBADF` | unverified | none |
| tier-b | mlibc-sysdep | `sys_seek` | `lseek`, `fseek` [INFERENCE: mlibc dispatch] | `VI_SYS_SEEK` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:125-131` | offset and whence passed through | kernel failure maps to `ESPIPE` | unverified | none |
| tier-b | mlibc-sysdep | `sys_clock_get` | `clock_gettime` [INFERENCE: mlibc dispatch] | `VI_SYS_GETTIME` (op 2 for realtime ns, op 0 for monotonic ticks) | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:137-153` | `CLOCK_REALTIME` and `CLOCK_MONOTONIC` only | other clocks give `EINVAL`; a negative time gives `EIO` | unverified | none |
| tier-b | mlibc-sysdep | `sys_anon_allocate` | `malloc` through mlibc's slab allocator [INFERENCE: mlibc dispatch] | static 4 MiB bump arena | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:158-170` | 16-byte aligned, zeroed | `ENOMEM` when the arena is exhausted | unverified | none |
| tier-b | mlibc-sysdep | `sys_anon_free` | `free` through the slab allocator [INFERENCE: mlibc dispatch] | no-op | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:172-175` | none | memory is never reclaimed | unverified | none |
| tier-b | mlibc-sysdep | `sys_vm_map` | `mmap` [INFERENCE: mlibc dispatch] | `sys_anon_allocate` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:178-183` | `MAP_ANONYMOUS` only | any other mapping gives `EINVAL` | unverified | none |
| tier-b | mlibc-sysdep | `sys_vm_unmap` | `munmap` [INFERENCE: mlibc dispatch] | `sys_anon_free` (no-op) | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:185-187` | none | memory is never reclaimed | unverified | none |
| tier-b | mlibc-sysdep | `sys_isatty` | `isatty` and stdio buffering choice [INFERENCE: mlibc dispatch] | constant | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:191-193` | fds 0-2 are terminals | other fds report not-a-terminal | unverified | none |
| tier-b | mlibc-sysdep | `sys_fstat` | `fstat` [INFERENCE: mlibc dispatch] | `VI_SYS_FSTAT` (ViFstatV1 wire record) | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:207-224` | only `st_mode` (character, regular or directory) and `st_size` are set | `EINVAL` for `NULL`; `EBADF` on a bad fd or short record; `EIO` on an unknown kind | unverified | none |
| tier-b | mlibc-sysdep | `sys_stat` | `stat` [INFERENCE: mlibc dispatch] | `sys_open`, `sys_fstat`, `sys_close` | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:226-234` | as `sys_fstat` on an opened path | `EINVAL` for `NULL`; otherwise the `sys_open` or `sys_fstat` error | unverified | none |

`cells/tests/mlibc-smoke` exercises mlibc `malloc`/`free`, `printf` and
`clock_gettime` (`cells/tests/mlibc-smoke/src/main.rs:60-114`). No integration
runner asserts its markers, so it is not cited as evidence.

## Consumers

| consumer | adapter | note |
|---|---|---|
| `cells/tests/posix-shim-test` | Tier A or Tier B, whichever the build selects | Declares the C ABI directly (`cells/tests/posix-shim-test/src/main.rs:56-75`). Witness for the rows that cite it. |
| `cells/tests/mlibc-smoke` | Tier B | Link and runtime smoke only. |
| `cells/demos/tetris-c` | Tier A | Class-A reference C port. |
| `cells/runtimes/lua`, `cells/demos/doom`, `cells/demos/tetris-lua` | Tier A, plus toolchain picolibc on RV64 | The picolibc overlap is outside this ledger (see Scope). |
| `cells/services/ocel-quickjs`, `cells/services/ocel-pdf` | Tier A | Use the allocation, string, stdio and math rows. |
