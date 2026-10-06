# Vendored: QuickJS (Fabrice Bellard, MIT)

Full source snapshot, owned by this repository — not a git submodule
(`third_party/README.md` policy). Vendored here rather than in `third_party/`
because the snapshot is only ever consumed by `cells/services/ocel-quickjs`,
the same way Lua 5.4 lives inside `cells/runtimes/lua`.

| Item | Value |
|---|---|
| Upstream | <https://bellard.org/quickjs/quickjs-2026-06-04.tar.xz> |
| Version | `2026-06-04` (see `vendor/quickjs/VERSION`) |
| SHA-256 (tarball) | `b376e839b322978313d929fd20663b11ba58b75df5a46c126dd19ea2fa70ad2a` |
| Licence | MIT — `vendor/quickjs/LICENSE`, Copyright (c) 2017-2021 Fabrice Bellard, Charlie Gordon |
| Vendored files | `quickjs.c/.h`, `quickjs-atom.h`, `quickjs-opcode.h`, `cutils.c/.h`, `libregexp.c/.h`, `libregexp-opcode.h`, `libunicode.c/.h`, `libunicode-table.h`, `dtoa.c/.h`, `list.h`, `LICENSE`, `VERSION`, `Changelog` |

`quickjs-libc.c`, `qjs.c`, `qjsc.c`, `run-test262.c`, `repl.js`, `examples/`
and `doc/` are deliberately **not** vendored: `quickjs-libc.c` is the `std`/`os`
module layer for the `qjs` command-line host and pulls `FILE*` stdio, `dlopen`,
`time` and process APIs that a cell does not have. The engine core is
`quickjs.c` + `libregexp.c` + `libunicode.c` + `cutils.c` + `dtoa.c`, which is
exactly the upstream `libquickjs` file set (`Makefile`: `QJS_LIB_OBJS`).

## Local deviations from upstream (complete inventory)

1. **`quickjs.c`: `CONFIG_ATOMICS` is behind `!defined(VIOS_NO_ATOMICS)`.**
   Upstream defines it for every non-Emscripten build, which drags in
   `pthread.h`, `stdatomic.h` and six pthread entry points. A Cellos cell is
   single-threaded and has no pthread implementation; the build defines
   `VIOS_NO_ATOMICS`, so the Atomics/SharedArrayBuffer surface is compiled out
   the same way an Emscripten build compiles it out. No other source line is
   modified — re-syncing from upstream means re-applying exactly this guard
   (the surrounding comment marks it).

Everything else the engine needs from libc is satisfied without an external
libc:

- memory, string, stdio and the C99 math set come from the Tier-A C ABI that
  `libs/api` already exports (`api::services::posix`), and `libm` backs the
  math symbols — never pass `-lm`;
- the cell passes its own `JSMallocFunctions` to `JS_NewRuntime2`, so the
  allocator is the cell's Rust heap;
- `malloc_usable_size` does not exist in that ABI and the C code only calls it
  from the glibc/macOS branch of `js_def_malloc_usable_size`; the build
  `-include`s `qjs_vios_config.h`, which maps it to `0` for the default
  fallback branch, matching upstream's own `EMSCRIPTEN` branch;
- `abort`, `snprintf`/`vsnprintf`/`fprintf`/`fputc`/`fwrite` and `stdout`/
  `stderr` are Tier-A exports; `strtod` and float `printf` are **not** needed
  because this release carries its own float64 parse/print (`dtoa.c`,
  `js_dtoa`/`js_atod`) and only formats `%u`/`%s`;
- `setjmp`/`longjmp` are not used by this release (`dtoa.c` includes
  `<setjmp.h>` but never jumps); Tier A exports them anyway;
- `gettimeofday`, `clock_gettime` and `localtime_r` are the three symbols the
  cell shim supplies itself (`src/ffi.rs`), over the cell's own clock.

Unsupported architectures compile a cfg-gated stub instead of the C engine
(`qjs_c_unavailable`, same shape as the Lua cell), so a workspace build on a
host without an ELF-capable C compiler still links.
