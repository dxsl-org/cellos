# T-2 — `ocel-quickjs` x86_64 float calling-convention mismatch: reproducer and fix

Opened by owner approval T-2 (`handoff-and-xsave-approval.md`). Branch
`fix/t2-quickjs-float-abi`, base `6fee21927`.

**Verdict: confirmed, then fixed inside `cells/services/ocel-quickjs/**`.**
`libs/api` and its ABI are unchanged; no Law-1 CP1 is needed.

Evidence ceiling: **host only** (x86_64 Linux, WSL2) for the x86_64 defect and
its fix, plus RV64 cross-builds. An RV64 QEMU TCG boot was attempted, but it
never loaded the engine (§ 5.1). No x86_64 cell was booted.

## 1. The reproducer

`reports/t2-quickjs-float-repro/run.sh [TREE]` does not use QEMU:

1. builds `ocel-quickjs` for `x86_64-unknown-none` in TREE with the cell's own
   `build.rs`. The C compiler is clang (`CC_x86_64_unknown_none=clang`), the C
   flags include `-msse2 -mfpmath=sse`, and the step keeps `libquickjs.a`;
2. builds `lib.rs` as a staticlib for `x86_64-unknown-none`. This is the same
   soft-float target (`"rustc-abi": "softfloat"`, `+soft-float`) as the cell's
   Rust. The staticlib compiles TREE's real `libs/api/src/services/posix/math.rs`
   through `#[path]`. Before the fix, it also compiles the pre-fix `ffi.rs`
   `lrint` and `vios_js_as_float` declarations. After the fix, it compiles TREE's
   `src/float_abi.rs` instead (`build.rs` picks the variant);
3. links both into `driver.c`, a host program. The program runs JavaScript in
   the engine and prints the engine's own `String(value)` output. The host libc
   supplies only allocation, strings, stdio and the clock, and none of these
   carries a float.

The "Rust->C" case follows `engine.rs:277-278`: Rust checks the tag
(`vios_js_is_float`), then reads the value.

Toolchain: clang 18.1.3, gcc 13.3.0, riscv64-unknown-elf-gcc 13.2.0, rustc
1.97.0-nightly (f53b654a8 2026-04-30), libm 0.2.16 (the workspace lock).

## 2. Before (6fee21927 sources): mismatch confirmed

```
C objects: 32 undefined C99 math symbols
BAD  C->Rust  Math.sqrt(2)        = 2          (expect 1.4142135623730951)
BAD  C->Rust  2 ** 0.5            = 2          (expect 1.4142135623730951)
BAD  C->Rust  Math.pow(2.5, 2)    = 2.5        (expect 6.25)
BAD  C->Rust  Math.sin(0.5)       = 0.5        (expect 0.479425538604203)
BAD  C->Rust  Math.exp(1)         = 1          (expect 2.7182818284590455)
BAD  C->Rust  Math.atan2(1, 1)    = 1          (expect 0.7853981633974483)
BAD  C->Rust  Math.hypot(3, 4)    = 3          (expect 5)
BAD  C->Rust  Math.cbrt(27)       = 27         (expect 3)
BAD  C->Rust  Math.log2(8)        = 8          (expect 3)
BAD  C->Rust  Math.floor(-2.5)    = -2.5       (expect -3)
OK   C->Rust  Math.round(-2.5)    = -2         (expect -2)
BAD  C->Rust  5.5 % 2             = 5.5        (expect 1.5)
BAD  C->Rust  Array.from(new Uint8ClampedArray([1.5, 2.5, 254.5])).join() = 1,2,254 (expect 2,2,254)
BAD  Rust->C  0.1 + 0.2           = 4.9406564584124654e-324 bits=0x0000000000000001 (expect 0x3fd3333333333334)
t2-quickjs-float-repro: 13 of 14 cases wrong
```

The output is column-trimmed here; the raw lines are in `t2-quickjs-float-repro/before.txt`
and `after.txt`. Every math
call returns its first argument. The C caller passes the argument in `%xmm0`
and reads the result from `%xmm0`. The Rust callee reads the argument from
`%rdi` and writes the result to `%rax`, so `%xmm0` comes back unchanged.
`Math.round` is the control case: QuickJS implements it without calling into
libm. In the "Rust->C" case, Rust reads `%rax`, which still holds the `1` that
`vios_js_is_float` returned.

**Disassembly of the real artefacts**:

- Caller, the cell's `libquickjs.a` (`quickjs.o`, `js_binary_arith_slow`):
  `cvtsi2sd %edx,%xmm0`; `cvtsi2sd %r13d,%xmm1`; `call pow`.
- Callee, the linked x86_64 cell ELF `target/x86_64-unknown-none/release/ocel-quickjs`.
  `js_pow` tail-jumps to `0xa140`. That function reads the arguments from
  `%rsi`/`%rdi` (`mov %rsi,%rdx; shr $0x20,%rdx`) and returns `1.0` for `y == 0`
  as `movabs $0x3ff0000000000000,%rax`. It never touches `%xmm0`.
- Rust→C: the glue `vios_js_as_float` is `movq %rdi,%xmm0; ret`, while Rust
  reads the result from `%rax`.

**Also found on RV64**: this is a separate prototype defect, not H1.
`vendor/include/math.h` declared `double lrint(double)`, but the Rust export
returned `i64`. The RV64 caller (`quickjs.o`, the `Uint8ClampedArray` store)
does `fmv.d.x fa0,a0; call lrint; fcvt.w.d a5,fa0,rtz`, so it reads the result
from `fa0`, while lp64d Rust returns it in `a0`. The fix below gives `lrint`
its C99 prototype `long lrint(double)`. The RV64 call site now reads `a0`
(`jalr ra` → `sext.w a5,a0`). This changes RV64 behaviour: `Uint8ClampedArray`
float stores now round half to even as specified. This was not exercised at
runtime on RV64 (§ 5.1).

**Float boundary checker** over the cell's boundary, with libs/api's math
exports included because the C links against them:

```
$ python3 scripts/check-float-ffi-boundary.py \
    --rust cells/services/ocel-quickjs/src libs/api/src/services/posix/math.rs \
    --c-header cells/services/ocel-quickjs/vendor/include cells/services/ocel-quickjs/src/c
exit=1, 56 findings: ffi.rs vios_js_as_float + lrint (2), vendor/include/math.h (34),
libs/api math.rs (20 = 16 direct + 4 macro templates)
```

## 3. Fix: an integer-only boundary inside the cell

- `vendor/include/math.h`: each standard name the engine uses is an object-like
  macro (`#define sin cellos_qjs_sin`) that names a `static inline` C wrapper.
  The wrapper passes the IEEE-754 bits as `uint64_t` to
  `cellos_qjs_<name>_bits` and reinterprets the `uint64_t` it gets back.
  Object-like macros keep the `Math` table's address-taking working. `lrint`
  now has its C99 prototype `long lrint(double)`. `modf` takes its integral
  part from `trunc`. All boundary declarations precede the first wrapper body,
  so the checker sees exactly the crossing prototypes. A canary `double`
  prototype placed in that block is flagged (`cellos_qjs_canary_bits: float/vector by value (double)`).
- `src/float_abi.rs` (new, no `unsafe`): one `#[no_mangle] extern "C"` export
  per wrapper, `u64 -> u64` (or `u64 -> i64` for `lrint`), over the same
  `libm` crate that libs/api uses. Each architecture computes the same result
  bit for bit.
- `src/ffi.rs`, `src/engine.rs`, `src/c/qjs_cellos_glue.c`:
  - `vios_js_is_float` and `vios_js_as_float` are removed. Rust reads every
    number through `JS_ToFloat64`'s out pointer, which `engine.rs` already used
    for integers.
  - The `f64` `lrint` export is removed.
  - `JSValue` by value stays. On 64-bit targets the C union's first word mixes
    integer, `double` and pointer members, so the SysV classification is
    INTEGER on both sides. The invariant is documented on `ffi::JSValue`.
- `build.rs`, `README.md`: the incorrect comment "the Rust side of this target
  compiles with it [SSE2] enabled" and the math provenance are corrected.

The same path runs on RV64, which keeps one boundary for the checker to verify.
`libs/api` is untouched; its 20 float exports stay for other C cells.

## 4. After

Same `run.sh`, fixed tree:

```
C objects: 0 undefined C99 math symbols
OK   C->Rust  Math.sqrt(2)        = 1.4142135623730951
OK   C->Rust  2 ** 0.5            = 1.4142135623730951
OK   C->Rust  Math.pow(2.5, 2)    = 6.25
OK   C->Rust  Math.sin(0.5)       = 0.479425538604203
OK   C->Rust  Math.exp(1)         = 2.7182818284590455
OK   C->Rust  Math.atan2(1, 1)    = 0.7853981633974483
OK   C->Rust  Math.hypot(3, 4)    = 5
OK   C->Rust  Math.cbrt(27)       = 3
OK   C->Rust  Math.log2(8)        = 3
OK   C->Rust  Math.floor(-2.5)    = -3
OK   C->Rust  Math.round(-2.5)    = -2
OK   C->Rust  5.5 % 2             = 1.5
OK   C->Rust  Array.from(new Uint8ClampedArray([1.5, 2.5, 254.5])).join() = 2,2,254
OK   Rust->C  0.1 + 0.2           = 0.30000000000000004 bits=0x3fd3333333333334
t2-quickjs-float-repro: 0 of 14 cases wrong
```

`Math.exp(1)` expects libm 0.2.16's value, `0x4005bf0a8b14576a`, which is one
ulp above `e`. A hard-float host program calling `libm::exp(1.0)` prints the
same `2.7182818284590455`, so this is a property of the library, not of the
boundary.

- The fixed `js_pow` is `movq %xmm0,%rbx` … `movq %xmm1,%rsi; mov %rbx,%rdi;
  call cellos_qjs_pow_bits; movq %rax,%xmm0`.
- The x86_64 `libquickjs.a` references no standard math symbol. Its 32 external
  math references are all `cellos_qjs_*_bits`.
- Checker, the cell's boundary: exit 0, no output.
  ```
  $ python3 scripts/check-float-ffi-boundary.py --rust cells/services/ocel-quickjs/src \
      --c-header cells/services/ocel-quickjs/vendor/include cells/services/ocel-quickjs/src/c
  exit=0
  ```
  If `libs/api/src/services/posix/math.rs` is added to `--rust`, the only
  findings are libs/api's own 20, unchanged. The cell's C no longer references
  those symbols.

## 5. RV64

- `cargo build --release -p ocel-quickjs --target riscv64gc-unknown-none-elf`
  ran with `CC=riscv64-unknown-elf-gcc` and `-C relocation-model=pic`, the CI
  values. Result: `Finished`; the ELF grew from 687 496 to 687 744 bytes.
- The engine archive's external symbols changed only in the math set: 32
  standard names were replaced by the 32 `cellos_qjs_*_bits`. Allocation,
  stdio, string and clock symbols are unchanged.
- RV64 integration lane: see § 5.1.

### 5.1 RV64 QEMU TCG lane

No runtime evidence on RV64: in both attempts the engine cell was never
loaded.

1. `bash scripts/gen-disk-ci.sh` built the fixed tree's kernel and
   `disk_v3.img` (`/bin/ocel-quickjs @ LBA 559009 (687880 bytes)`). It
   rewrote the tracked `kernel/src/embedded/init`, which was restored
   afterwards.
2. `cargo test --test ocel-quickjs` (QEMU TCG) failed at
   `tests/ocel-quickjs.rs:98`: `Ocel failed to start`. The shell refuses the
   lane's `ocel … &` before `ocel` runs:
   `shell: SDK background launch requires explicit nonterminal streams`
   (`t2-quickjs-float-repro/rv64-qemu-tcg-lane.txt:419`).
3. A scratch QEMU run (`t2-quickjs-float-repro/rv64-foreground.py`) used the
   same QEMU command line and launched `ocel` in the foreground. It reached
   `[ocel] Starting Ocel Document Viewer...` and then printed nothing for 60 s
   (`rv64-qemu-tcg-foreground.txt`).

Neither log contains `[domain] admitted cell 'ocel-quickjs'`. QuickJS is
demand-loaded, so both failures happen before any code this change touches
runs. A pre-fix boot was not run for comparison. The RV64 lane needs its own
triage.

## 6. Residual risks

- **Variadic floats are outside the checker.** `JS_DumpMemoryUsage` and the
  `DUMP_*` debug paths pass `double` to `fprintf`/`printf`. The driver never
  calls them, and README records that the engine formats only `%u`/`%s`.
- **Host unit tests (`cargo test -p ocel-quickjs`) cannot build on this Linux
  host.** `quickjs.c` pulls glibc `<malloc.h>`, which conflicts with
  `qjs_vios_config.h`'s `malloc_usable_size` macro. The pre-fix sources fail
  with the same error when built with host gcc.
- **RV64 now goes through the same integer boundary.** The change in symbols
  is listed in § 5. The RV64 lane could not confirm runtime behaviour (§ 5.1).
- **No x86_64 boot exists.** `tests/integration/tests/ocel-quickjs.rs` is RV64
  only, and x86_64 Tier-2 launch rows are held (H-1).
