---
phase: 1
title: "x86 C/C++ toolchain and the SSE2 float boundary"
status: in-progress
priority: P1
effort: "2-4d"
dependencies: ["R0 (D-R3)"]
tier: thinking
checkpoint: false
---

# Phase R1: x86 toolchain and float ABI

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [plan.md](plan.md) D-R3; [port gaps](../261104-0801-x86-igpu-compute/research/mesa-anv-port-gaps.md) §3 rows "C toolchain" and "Hard-float SSE2 C ABI", §6 G-a
- `docs/guides/tier1b-c-zig.md` (profiles); Mesa `-Dsse2=true` adds `-msse2 -mfpmath=sse` (port gaps §1.1)

## Overview

Tier-2 C and C++ for the inference Cell (Mesa, llama.cpp, mlibc, libc++) need SSE2 hard-float code. Today:
- every x86 Cell's Rust code is soft-float;
- x86 C is compiled `-mno-sse`.

R1 provides:
- an x86 toolchain row;
- meson and CMake cross files;
- the H1 boundary rule with a mechanical check;
- a witness that the kernel preserves SSE state for Tier-2 threads.

R1 does not change any target JSON, `libs/api` or kernel file.

## Key Insights

**Rust is soft-float on every x86 Cell path:**
- the builtin `x86_64-unknown-none` target (spec printed this session: `+soft-float`, `rustc-abi: softfloat`);
- the std target `targets/x86_64-unknown-cellos.json:8,22`;
- the Cell RUSTFLAGS (`scripts/run-std-smoke-qemu.sh:114`; `scripts/app_tier_acceptance/source.py:96`).

**C defaults to `-mno-sse -mno-mmx`** (`.github/workflows/ci.yml:311`; `scripts/run-std-smoke-qemu.sh:112`). The other architectures are consistent:
- RV64 Cells are hard-float `lp64d` in both Rust and C (`targets/riscv64gc-unknown-cellos.json:9,12`; `tools/cellos-cc:9-11`);
- AArch64 C uses `-mgeneral-regs-only` (`tools/cellos-cc:13-15`).

**The kernel already preserves user SSE state for every task** `[inference: no witness test exists]`:
- Both the syscall and interrupt entries save the full FXSAVE image (x87, MMX, XMM0-15, MXCSR) on the task's kernel stack, then restore it (`hal/arch/x86/src/x86_64/syscall.rs:340-353`; `hal/arch/x86/src/x86_64/idt/entry.rs:144-156`).
- The context switch keeps only MXCSR and x87 control, because "asynchronous user state lives in the ISR/syscall stack's FXSAVE image" (`hal/arch/x86/src/x86_64/context.rs:4-8`).
- A fresh user thread starts with zeroed x87 and MXCSR `0x1F80` (`hal/arch/x86/src/x86_64/boot.rs:92-106`).
- CR4.OSFXSR and OSXMMEXCPT are set (`hal/arch/x86/src/x86_64.rs:562-565`).

**Precedent and hazard: `ocel-quickjs`** `[inference]`
- It drops `CFLAGS_<target>` (`cells/services/ocel-quickjs/build.rs:93-100`) and compiles x86 C with `-msse2 -mfpmath=sse` (`:184-191`).
- Its comment says "the Rust side of this target compiles with it enabled" (`:187-188`). The target spec above contradicts that.
- It takes its C99 math from `libs/api` (`build.rs:11-12`), which exports Rust `extern "C" fn sin(x: f64) -> f64` and similar (`libs/api/src/services/posix/math.rs:13-25`, `:53`).
- With soft-float Rust and hard-float C, a `double` argument travels in different registers, so x86 `Math.*` results may be wrong.
- Its integration test boots only RV64 (`tests/integration/tests/ocel-quickjs.rs:31-36`).
- Reported to Main as a separate defect. R1 does not fix it, but R1's checker would flag it.

**Missing pieces:**
- `tools/cellos-cc` has no x86 row (`tools/cellos-cc:7-18`).
- `meson/` holds only `cellos-riscv64.ini`, and `cmake/` only riscv64 and aarch64 (listed this session).
- `tools/cellos-zig-cc` maps x86_64 to `x86_64-freestanding-none` (`tools/cellos-zig-cxx:74-75`).

## Requirements

**H1 rule (if the owner chooses it):**
- Tier-2 C/C++ objects are compiled `-msse2 -mfpmath=sse`, with at most SSE4.2 until R5 lands.
- No function crossing Rust↔C may take or return `float`, `double`, `long double`, `_Complex`, vector types, or structs or unions containing them, in either direction. This includes callbacks.
- Rust code in such a Cell does not link the Tier-A `libs/api` POSIX shim. mlibc Cells already gate it off (`libs/api/src/services/posix.rs:27-33`).

**Toolchain:**
- An x86 row in `cellos-cc`: clang `--target=x86_64-unknown-none-elf -msse2 -mfpmath=sse -mno-red-zone -mcmodel=small -fPIC`, matching `cells/tests/cpp-smoke/build.rs:133-137` plus SSE2.
- `meson/cellos-x86_64.ini` and `cmake/cellos-x86_64.cmake`, following `meson/cellos-riscv64.ini` and `cmake/cellos-riscv64.cmake`. "Rust owns final cell linking" stays (`tools/cellos-cc:3-4`).
- A checker, `scripts/check-float-ffi-boundary.py`:
  - inputs: the Cell's Rust `extern "C"` declarations and exports, plus the C headers of the linked archives;
  - fails on any float or vector type at the boundary;
  - an allowlist is empty by default.
- A witness Cell, `cells/tests/x86-fpu-preserve` (Tier-2): two threads load distinct XMM0-15 patterns and MXCSR, then spin across timer preemption, `Yield` and a blocking syscall, then verify.

## Architecture

```text
Tier-2 inference Cell (one private root)
  Rust host (soft-float, ostd crt0, mlibc-shim) ──int/pointer-only C ABI──┐
  C/C++ (SSE2 hard-float): mlibc, libc++, Mesa, llama.cpp <───────────────┘
kernel entry: FXSAVE64 image per entry (R5 widens to XSAVE)
```

## Related Code Files

- Create: `meson/cellos-x86_64.ini`, `cmake/cellos-x86_64.cmake`, `scripts/check-float-ffi-boundary.py`, `cells/tests/x86-fpu-preserve/**`, its QEMU runner script
- Modify: `tools/cellos-cc` (x86 row), `docs/guides/tier1b-c-zig.md` (x86 float rule), workspace `Cargo.toml` member line
- Not touched: `targets/*.json`, `libs/api/**`, `hal/**`, `kernel/**`

## Implementation Steps

1. Add the `cellos-cc` x86 row and both cross files. Host check: `meson setup` on a trivial project, and CMake try-compile.
2. Write the checker with unit tests. Each failure has a fixture: `double` arg, `float` return, `struct {double}`, `__m128`, a callback taking `float`.
3. Build the witness Cell. Run it under QEMU TCG and QEMU/KVM in the development profile, where x86 Tier-2 admission is enabled once the substrate qualifies (`kernel/src/loader/domain_admission.rs:368-399`; `kernel/src/main.rs:1283-1293`).
4. Run the checker over `ocel-quickjs` as a data point and record the result in the Deviation Log. Do not fix it here.

## Todo List

- [x] `cellos-cc` x86 rows: `x86_64-unknown-none` (no SSE, the default) and `x86_64-unknown-none-sse2` (H1)
- [x] meson and CMake cross files; both carry `CELLOS_C_TARGET` themselves. Host check: `meson setup`/`compile` and a CMake static library produce SSE2 objects (`mulsd`), with the variable unset
- [x] Float-boundary checker + fixtures (`scripts/test_check_float_ffi_boundary.py`, 7 tests)
- [ ] FPU-preserve witness (TCG + KVM). **Blocked (H-1):** launching any Cell path needs a row in `kernel/src/loader/launch_profile/profiles.rs`, which S1 holds
- [x] Guide update (`docs/guides/tier1b-c-zig.md`, "x86_64 C and the float boundary")

## Success Criteria

- [ ] The witness passes 10 000 preemptions per thread under TCG and KVM with no XMM or MXCSR corruption.
- [ ] The checker rejects all fixtures and accepts a clean int/pointer-only boundary.
- [ ] No existing x86 suite changes behaviour, because the default CFLAGS are untouched.

## Risk Assessment

- **Soft-float Rust inside the inference Cell is slow for numeric Rust code.** Mitigation: the numeric work is in C/C++ (llama.cpp, Mesa) or on the GPU. If a Rust engine must do float math across the boundary, reopen D-R3 with H2 (Law 1).
- **The checker cannot see casts through `void*`.** Mitigation: the rule is documented, and the boundary is kept small (mlibc sysdeps, Cell `main`, transport shim entry points).

## Security Considerations

None: compile-time flags and a test Cell. A float-ABI mismatch is a correctness bug, not an isolation bug, because both sides run in the same untrusted domain.

## Next Steps

R2 uses the toolchain row and the meson cross file to build mlibc.

## Deviation Log

- Decision: two x86 rows instead of one. The default keeps `-mno-sse -mno-mmx`, the existing x86 C convention (`.github/workflows/ci.yml:311`); SSE2 is opt-in by name.
  Why: a single hard-float row would let any x86 C caller pass `double` to soft-float Rust.
  Impact: none on existing Cells; `cellos-cc` had no x86 row.
  Revert: drop the rows.
- Surprise: setting `ENV{CELLOS_C_TARGET}` in a CMake toolchain file covers configure time only. With the variable unset, the build-time compile fell back to the riscv64 default (`riscv64-unknown-elf-gcc: error: unrecognized command-line option '--target=x86_64-unknown-none-elf'`).
  Why: CMake does not export toolchain-file `ENV` to make or ninja.
  Impact: the x86_64 file uses `CMAKE_C_COMPILER_LAUNCHER env CELLOS_C_TARGET=…`; the meson file uses an `env` prefix. The existing `cmake/cellos-aarch64.cmake` has the same defect: reproduced with the variable unset, the same error with `--target=aarch64-unknown-none-elf`. Reported, not fixed here.
  Revert: n/a.
- Data point (step 4): `check-float-ffi-boundary.py --rust libs/api/src/services/posix/math.rs` reports 20 exports. Sixteen are direct (`nearbyint`, `fma`, `ldexp`, `frexpf`, …), and 4 are `macro_rules!` templates (`$c`) that generate `sin`, `cos` and the rest. `ocel-quickjs` compiles x86 C with `-msse2 -mfpmath=sse` against these exports, so the suspected `ocel-quickjs` x86 math defect is confirmed at the boundary-type level; it was not run. Not fixed here.
- Blocker H-1: the witness Cell needs a launch row in `kernel/src/loader/launch_profile/profiles.rs`, held and dirty under S1 (lock inventory, S1 phase 05 extension). Owner choice: S1 adds an append-only, empty-capability row, or the witness waits for S1's release.
