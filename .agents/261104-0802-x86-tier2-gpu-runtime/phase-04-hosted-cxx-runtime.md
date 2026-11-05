---
phase: 4
title: "Hosted C++17 runtime profile: LLVM libc++, libc++abi, libunwind over mlibc"
status: pending
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

- [ ] Pin the llvm-project tag + licence check per file
- [ ] Host dry run
- [ ] Cross build over mlibc
- [ ] Witness Cell (TCG + KVM)
- [ ] Size measurement in the ADR
- [ ] Docs: guide, spec 05, spec 18, ledger, NOTICE

## Success Criteria

- [ ] The witness prints all markers, including a caught exception and a joined `std::thread`, under TCG and KVM.
- [ ] An uncaught exception terminates the Cell with a logged reason and no kernel fault.
- [ ] `cpp-smoke` still passes its own absence assertions.

## Risk Assessment

- **libc++ may need libc features mlibc lacks** (`<locale>` details, `aligned_alloc`, `timespec_get`). Mitigation: step 2 fixes them in R3; every gap is a ledger row.
- **Runtime-builtins duplicates between compiler-rt and Rust `compiler_builtins`.** Mitigation: decide one owner at the first link, and test with `--no-allow-multiple-definition`.
- **Size growth.** libc++ and localization can add MBs `[inference]`. Mitigation: measure; `-ffunction-sections` + `--gc-sections`.

## Security Considerations

The runtime runs inside the untrusted Tier-2 domain and adds no authority. Unwinding reads only the Cell's own `.eh_frame`. A corrupted unwind table faults inside the domain.

## Next Steps

G3 links Mesa (`brw`, `gen`, Jay) and llama.cpp against `cpp-hosted`.

## Deviation Log

None.
