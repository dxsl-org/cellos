---
phase: 4
title: "Hosted C++17 runtime profile: LLVM libc++, libc++abi, libunwind over mlibc"
status: in-progress (QEMU TCG witness passes; KVM not yet run)
priority: P1
effort: "8-14d"
dependencies: ["R3 (R3a minimum)", "ADR for cpp-hosted accepted (D-R4)"]
tier: thinking
checkpoint: true
---

# Phase R4: hosted C++ runtime

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [plan.md](plan.md) D-R4; [port gaps](../261104-0801-x86-igpu-compute/research/mesa-anv-port-gaps.md) §0.5, §1.3 (Mesa C++ without exceptions), §5 (llama.cpp needs exceptions, `<thread>`, `<future>`, iostreams), §6 G-i, G-j
- [ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md) §2.3; `docs/specs/05-application.md` §3.2; `docs/specs/18-cell-trust-tiers.md:58-60`
- LLVM `libcxx/`, `libcxxabi/`, `libunwind/` (Apache-2.0 WITH LLVM-exception)

## Overview

R4 adds a second C++ runtime profile, `cpp-hosted`, beside the unchanged `cpp-freestanding`:
- a static LLVM libc++ (C++17 at least), libc++abi and libunwind, built once with exceptions and RTTI;
- on mlibc `posix` (R3), for x86_64 Tier-2 Cells first.

Mesa compiles its translation units `-fno-exceptions -fno-rtti` and links the same library `[inference: standard libc++ practice; confirm at link]`. llama.cpp compiles with exceptions. One Cell, one runtime.

## Key Insights

**Why it is forbidden today:**
- The current profile is language-only. There are no standard headers (`docs/guides/tier1b-c-zig.md:278-282`), and exceptions, RTTI, thread-safe statics and the STL are "not available" (`:284-286`).
- Its runner rejects `__cxa_throw`, `_Unwind_*`, `__gxx_personality` and `_ZSt*` (`scripts/qemu-cpp-smoke.sh:52-54`).
- Spec 05 states that vendoring libc++ is "a separate decision with its own size measurement" (`docs/specs/05-application.md:204-205`).
- ADR-0018 gates hosted C++ on TLS and unwinder work (`0018-…:100-101`). R2 and R3 deliver that work.

**Doc drift.** The guide and spec 05 say the freestanding profile is RV64/AArch64 only (`tier1b-c-zig.md:288-290`; `05-application.md:207-210`). The code admits x86_64: the POSIX shim cfg (`libs/api/src/services/posix.rs:13-22`) and `cells/tests/cpp-smoke/build.rs:126-152`. The fix belongs to R0/R4 doc updates.

**The Tier-A C++ ABI layer is single-threaded.** `__cxa_guard_*` takes no lock, and `atexit`/`__cxa_atexit` never fire (`docs/posix-ledger.md:108-112`). It is gated off when `mlibc` is enabled (`posix.rs:27-33`), so libc++abi's guards and atexit take over in `cpp-hosted` Cells.

**Licence:**
- `libcxx/LICENSE.TXT`, `libcxxabi/LICENSE.TXT` and `libunwind/LICENSE.TXT` on llvm-project `main` read "Apache License v2.0 with LLVM Exceptions" (fetched 2026-11-04). libc++ also carries the legacy NCSA/MIT dual licence.
- `deny.toml:38` allows `Apache-2.0 WITH LLVM-exception`.
- The LLVM exception covers object-form embedding.
- Tags pinned at R4 start are re-checked per file.

**Profile budget.** ADR-0018 condition 4 (16 MiB heap, 32 MiB VA stride, `0018-…:94`) cannot hold for llama.cpp. The ADR restates the `cpp-hosted` budget against the large-memory CP1 quota.

**Unwinding stays out of Rust.**
- Cell Rust uses `panic-strategy: abort` (`targets/x86_64-unknown-cellos.json:20`). C++ exceptions must never cross a Rust frame.
- Every C ABI entry point from Rust into C++ is `noexcept`, so an escaping exception reaches `std::terminate` → `abort` → mlibc panic → Cell exit.

**Alternatives rejected:**
- GNU libstdc++: GPL-3.0 with the GCC Runtime Library Exception; a new licence class for `third_party/`, and GCC-only build glue.
- Two runtime builds (with and without exceptions) in one Cell: duplicate symbols.

## Requirements

**Build:**
- A pinned llvm-project tag, built with `LLVM_ENABLE_RUNTIMES="libcxx;libcxxabi;libunwind"` using the R1 `cmake/cellos-x86_64.cmake` and the mlibc sysroot from R2/R3.
- Static only.
- Options: `LIBCXXABI_USE_LLVM_UNWINDER=ON`; threads via pthread (R3); localization on (llama.cpp iostreams); `<filesystem>` and random-device according to the link inventory.
- Each disabled feature is listed in the profile contract.

**Compiler runtime builtins** (`__udivti3`, …) come from compiler-rt builtins or Rust `compiler_builtins`, exactly one of them. The duplicate-symbol policy is decided at the first link.

**Profile contract:**
- the exact flags (`-fexceptions -frtti` allowed; `-fno-exceptions -fno-rtti` per translation unit allowed);
- the libc tier (mlibc `posix`);
- the facilities;
- the refused features: `dlopen`, `std::thread` beyond the R3 thread limit `[inference: set by Spawn capacity]`, locale data beyond `C`/`POSIX` unless measured.

**Witness Cell `cells/tests/cpp-hosted-smoke` (x86 Tier-2):**
- STL containers, `<optional>`, `<variant>`, `<charconv>`;
- `throw`/`catch` across three frames, `dynamic_cast`;
- `std::thread` + `std::future` + `std::shared_mutex`, `thread_local`, a thread-safe static under contention;
- `std::cout`;
- `nm` shows `_ZSt*`, `__cxa_throw` and `_Unwind_RaiseException` present, and no Tier-A `__cxa_guard_acquire`.

**Size measurement:** the bytes added to the Cell by libc++ + libc++abi + libunwind, release and stripped, recorded in the ADR (spec 05 `:204-205`).

## Architecture

```text
cpp-hosted Cell (x86_64 Tier-2)
  Rust host (abort, soft-float) ──noexcept extern "C"──> C++ (llama.cpp: -fexceptions; Mesa: -fno-exceptions)
  libc++ ─> libc++abi ─> libunwind ─> dl_iterate_phdr (R3) ─> .eh_frame_hdr
  libc++ threads ─> pthread (mlibc R3) ─> Spawn/Futex
```

## Related Code Files

- Create: `scripts/build-llvm-runtimes.sh` (pinned tag, reproducible), `third_party/llvm-runtimes/README.md` (pin, licence, NOTICE text), `cells/tests/cpp-hosted-smoke/**` + runner
- Modify: `docs/guides/tier1b-c-zig.md` (new `cpp-hosted` section; fix the x86 drift), `docs/specs/05-application.md` §3.2, `docs/specs/18-cell-trust-tiers.md:58-60`, `NOTICE` (LLVM attribution when a shipped image includes it), `docs/posix-ledger.md` (C++ ABI rows for Tier B)
- Not touched: `libs/api/**`, `kernel/**`; `cells/tests/cpp-smoke` and its runner rule (the freestanding profile stays as is)

## Implementation Steps

1. Host dry run: build the runtimes for `x86_64-linux-gnu` with the same option set, so option errors surface without Cellos.
2. Cross build against the mlibc sysroot. Fix each missing libc symbol in R3 (ledger row) rather than stubbing it in libc++.
3. Build the witness Cell and run it under QEMU TCG and KVM.
4. Measure size and record it in the ADR. Update guide, spec and ledger.

## Todo List

- [x] Pin the llvm-project tag + licence check per file (`llvmorg-19.1.7` @ `cd708029e0b2`; 1204 files; `scripts/build-llvm-runtimes.sh` step 2)
- [x] Host dry run (x86_64-linux-gnu build + host witness run: 11/11 markers, uncaught exception aborts with the libc++abi line)
- [x] Cross build over mlibc (step 4; every runtime libc reference resolves in mlibc `libc.a` after patch 0002)
- [ ] Witness Cell (TCG + KVM). TCG passes: `CPP-HOSTED-SMOKE: PASS (runs: 5)` on 2026-11-05 ([log](reports/r4-evidence/qemu-x86-cpp-hosted-tcg-5runs.log)). KVM has not run (a separate known failure is under investigation by Main).
- [x] Size measurement in the ADR (ADR-0034 Consequences, size table)
- [x] Docs: guide, spec 05, spec 18, ledger, NOTICE

## Success Criteria

- [ ] The witness prints all markers, including a caught exception and a joined `std::thread`, under TCG and KVM. TCG is met: `CPP-HOSTED: 11/11 pass` ×5 in one boot. KVM is still open.
- [ ] An uncaught exception terminates the Cell with a logged reason and no kernel fault. TCG is met:
  - `libc++abi: terminating due to uncaught exception of type std::runtime_error: cpp-hosted uncaught`;
  - `mlibc: sigaction failed in abort` and `mlibc PANIC` are logged;
  - no `[fault] Cell` line appears, `DOMAIN-FRAME-RELEASE: PASS` follows each run, and the prompt returns.

  KVM is still open.
- [x] `cpp-smoke` still passes its own absence assertions (build-level: riscv64 and x86_64 `cpp-smoke` link with no `__cxa_throw`/`_Unwind_*`/`__gxx_personality`/`_ZSt*`; 2026-11-05).

## Risk Assessment

- **libc++ may need libc features mlibc lacks** (`<locale>` details, `aligned_alloc`, `timespec_get`). Mitigation: step 2 fixes them in R3; every gap is a ledger row.
- **Runtime-builtins duplicates between compiler-rt and Rust `compiler_builtins`.** Mitigation: decide one owner at the first link, and test with `--no-allow-multiple-definition`.
- **Size growth.** libc++ and localization can add MBs `[inference]`. Mitigation: measure; `-ffunction-sections` + `--gc-sections`.

## Security Considerations

The runtime runs inside the untrusted Tier-2 domain and adds no authority. Unwinding reads only the Cell's own `.eh_frame`. A corrupted unwind table faults inside the domain.

## Next Steps

G3 links Mesa (`brw`, `gen`, Jay) and llama.cpp against `cpp-hosted`.

## Deviation Log

All entries 2026-11-05.

1. **Decision — pin `llvmorg-19.1.7`.** libc++ 19 is the newest release that supports clang 18, the compiler every Cellos C/C++ row uses; libc++ 20 needs clang 19. Reversible by moving the pin.
2. **Surprise → fix in mlibc, not libc++.** libc++ needs `strtoll_l` and `strtoull_l` in `<locale>`, which every consumer TU includes, plus `strxfrm_l`, `wcscoll_l` and `wcsxfrm_l` in `locale.cpp`. mlibc at the pin has none of them. **Deviation (file set):** the fix is `third_party/mlibc/patches/0002-posix-locale-l-functions.patch`, which `scripts/build-mlibc.sh` applies after 0001. `patches/` is outside R4's §3 list, and Main approved committing it after R6 freed H-5. Ledger rows were added.
3. **Decision — libc++ configuration for mlibc.**
   - `_LIBCPP_PROVIDES_DEFAULT_RUNE_TABLE` goes into `__config_site`, as for musl: mlibc exposes no ctype table that libc++ knows.
   - `-D_GNU_SOURCE` is part of the profile flags (what clang++ predefines on GNU targets). Without it, mlibc hides `Dl_info`/`dladdr` and the POSIX-2024 surface.
   - The configure-time library probes (`*_HAS_{PTHREAD,DL,RT,...}_LIB`) are forced OFF. Static try-compiles made them all "succeed", and the resulting `.deplibs` made lld look for `-lpthread`/`-ldl`.
   - `LIBCXX_HAS_MUSL_LIBC` stays OFF: it needs musl's `<bits/alltypes.h>`.
4. **Deviation — build outputs under `target/llvm-runtimes/`**, not `third_party/llvm-runtimes/build-*`. A new `.gitignore` entry would have been outside the file set; `target/` is already ignored. `third_party/llvm-runtimes/` holds only the README.
5. **Deviation — new standalone scripts (H-4):** `scripts/build-x86_64-cpp-hosted-ci.sh` (image and link proof) and `scripts/x86/qemu-cpp-hosted-smoke.sh` (runner), modelled on the mlibc-witness pair. No shared lane was edited.
6. **Deviation (file set) — `scripts/unsafe-allowlist.toml`:** one `[[file]]` and one `[[crate]]` c-ffi entry for `app-cpp-hosted-smoke`, the same shape as R3's, approved by Main on 2026-11-05. Without them `cellos-sign --check` F1 refuses to sign the image.
7. **Decision — single-run witness.** The Cell prints the 11 markers and `CPP-HOSTED: 11/11 pass`, then deliberately throws past a `noexcept` entry. A passing run therefore always exits with status 1, and its verdict is the markers, the libc++abi terminate line, a fresh prompt and no fault. One launch row instead of two, and no argv dependency.
8. **Decision — size method.**
   - Runtime bytes are the kept input sections that the lld map attributes to the three archives (`build-llvm-runtimes.sh size`). This matches the real Cell link byte for byte.
   - `.eh_frame`/`.eh_frame_hdr` are linker-internal in lld maps, so they are reported per image.
   - No Mesa build exists yet, so the "Mesa-only link" row uses a Mesa-shaped probe (`cpp/mesa_probe.cpp`, `-fno-exceptions -fno-rtti`).
9. **Decision — compiler builtins:** `libgcc.a` from the mlibc build is the single provider; compiler-rt is not built, and no `compiler_builtins` member is extracted. Checked with `--no-allow-multiple-definition` and `--why-extract`.
10. **Surprise — Cell linker script.** `libs/cell-build/src/cell.ld.in` has no rules for `.gcc_except_table.*`, `.eh_frame` or `.eh_frame_hdr`, so lld places them as orphans.
    - The R-only PT_LOAD after `.bss` that holds `.eh_frame_hdr`/`.eh_frame` has the same shape as in `mlibc-posix-x86`, which already boots on QEMU.
    - New here: each per-function `.gcc_except_table.*` becomes its own output section, about 380 section headers in total.
    - The loader reads program headers, so this should not matter `[INFERENCE]`. It is unproven until the boot. A `.gcc_except_table : { *(.gcc_except_table .gcc_except_table.*) }` rule in `cell.ld.in` would remove the orphans, but `cell-build` is outside R4's file set.
11. **Merge:** `main` (R6, `7ca2b7c61`) was merged into `feat/r4-hosted-cxx`, and mlibc was rebuilt from the committed tree with patch 0002. The private pre-merge mlibc build in `target/r4-mlibc` was verification scaffolding only.
12. **Surprise → fix in mlibc (first TCG boot, after the H-1 rows landed on main `6c61ba1ec`).** The Cell faulted before `main`: `#PF user va=0x232af08 rip=0x108062ad0`, just below `rsp`.
    - `rip` is `ScopeTrace::ScopeTrace` in mlibc's pthread code, calling `mlibc::globalConfig()`.
    - `globalConfig()` was a function-local static, so it called `__cxa_guard_acquire`. In a `cpp-hosted` Cell that symbol is libc++abi's, which locks a pthread mutex. `pthread_mutex_lock` opens a `ScopeTrace`, which calls `globalConfig()` again: unbounded recursion that exhausted the 64-page (256 KiB) root stack.
    - mlibc's own hidden guard never takes a lock, so mlibc-only Cells never recursed.
    - The kernel stack size is not the cause.
    - **Fix:** `third_party/mlibc/patches/0003-guard-free-global-config.patch`. The config is constant-initialised and loaded once behind an atomic state word; a re-entrant call while loading sees every debug switch off.
    - Rebuilt mlibc, the runtimes and the image. TCG: `CPP-HOSTED-SMOKE: PASS (runs: 5 ...)`. The mlibc witness regression was rerun on TCG after the patch.
