# ADR-0034 — Hosted C++ runtime profile (`cpp-hosted`): LLVM libc++, libc++abi and libunwind over mlibc

**Date:** 2026-11-04
**Status:** Accepted (owner decision D-R4, 2026-11-04). The owner approved D-R1…D-R10 of the [x86 Tier-2 runtime plan](../../.agents/261104-0802-x86-tier2-gpu-runtime/plan.md) as recommended ("duyệt"); see the [approval record](../../.agents/261104-0802-x86-tier2-gpu-runtime/owner-decisions-approval.md).
**Amends:** [ADR-0018](0018-cell-native-portability-and-runtime-profiles.md) §2.3. It adds a second C++ profile. `cpp-freestanding` is unchanged.
**Decider:** Cellos maintainer (owner).
**Implementation authority:** direction only.
- The work is phase R4 of the plan above. It needs its own CELL-CONTRACT-0024 lock handoff and falls under the Intel single-WIP rule ([ADR-0033](0033-x86-intel-igpu-native-compute.md) §6).
- No `libs/api` or `libs/types` change is authorised. If one becomes necessary, it goes through Law 1 (`docs/code-standards.md:40-46`).

## Context

**Who needs it.** The Intel iGPU inference Cell (ADR-0033, [phase G3](../../.agents/261104-0801-x86-igpu-compute/phase-03-mesa-anv-tier2.md)) links Mesa `anv` and llama.cpp into one x86_64 Tier-2 Cell:
- Mesa's compiler back ends (`brw`, `gen`; Jay links `gen`) are C++17 and use `<vector>`, `<optional>`, `<variant>`, `<map>`, `<string>` and `<charconv>`, without exceptions or RTTI ([port gaps](../../.agents/261104-0801-x86-igpu-compute/research/mesa-anv-port-gaps.md) §1.3).
- llama.cpp's Vulkan backend needs exceptions (`vk::SystemError`), RTTI, `<thread>`, `<future>`, `<shared_mutex>` and iostreams (port gaps §5).

**Why the shipped profile is not enough.** `cpp-freestanding` is language-only:
- It has no standard headers (`docs/guides/tier1b-c-zig.md:278-282`).
- Exceptions, RTTI, thread-safe statics and the STL are "not available" (`:284-286`).
- Its runner rejects `__cxa_throw`, `_Unwind_*`, `__gxx_personality` and `_ZSt*` in the linked Cell (`scripts/qemu-cpp-smoke.sh:52-54`).
- Its C++ ABI layer comes from the Tier-A POSIX shim. That layer is single-threaded (`__cxa_guard_*` takes no lock) and never runs `atexit`/`__cxa_atexit` handlers (`docs/posix-ledger.md:108-112`).

**What ADR-0018 requires.**
- A language enters as a runtime profile under five conditions (`0018-…:85-95`).
- Escalation to hosted C++ "is gated on the TLS and unwinder work, not on this ADR" (`0018-…:100-101`).
- Spec 05 makes vendoring a libc++ "a separate decision with its own size measurement" (`docs/specs/05-application.md:204-205`).

## Decision

1. **New runtime profile `cpp-hosted`.**
   - Hosted C++ (C++17 at least) on LLVM libc++, libc++abi (C++ ABI, exceptions, RTTI, thread-safe statics) and LLVM libunwind.
   - Built once, static, with exceptions and RTTI enabled.
   - Translation units may still compile `-fno-exceptions -fno-rtti` (Mesa) and link the same runtime. One Cell links exactly one C++ runtime.
2. **Scope.**
   - x86_64 Tier-2 (private-root) Cells first.
   - RV64 and AArch64 come later, as an amendment with their own witness.
   - Tier-1 (SAS) Cells may not use the profile.
3. **libc tier.** Tier-B mlibc with the `posix` option (plan phases R2-R3). The Tier-A POSIX shim's C++ ABI layer is not linked into `cpp-hosted` Cells; it is already gated off under `mlibc` (`libs/api/src/services/posix.rs:27-33`).
4. **Unwinding stays inside C++.**
   - Cell Rust uses `panic-strategy: abort` (`targets/x86_64-unknown-cellos.json:20`).
   - Every C ABI entry point from Rust into C++ is `noexcept`. An escaping exception reaches `std::terminate` → `abort` → mlibc panic → Cell exit.
   - No exception crosses a Rust frame.
5. **Floating point.** C++ objects are compiled SSE2 hard-float under the H1 boundary rule (owner decision D-R3): no float or vector types cross the Rust↔C ABI.

### ADR-0018 §2.3 conditions applied to `cpp-hosted`

| # | Condition (`0018-…:90-95`) | How `cpp-hosted` meets it |
|---|---|---|
| 1 | Static PIC PIE, RELATIVE relocations only | Static archives linked by the Rust Cell host ("Rust owns final cell linking", `tools/cellos-cc:3-4`); `-fPIC`; no shared libc++ |
| 2 | Declares its libc tier with a published symbol contract | mlibc `posix`. The Tier-B rows of `docs/posix-ledger.md` are extended for every sysdep and C++ ABI symbol the profile uses |
| 3 | Declares its runtime facilities; threads need TLS and futex | TLS through `SetTlsBase = 9` (`libs/api/src/abi/syscall.rs:161-172`) and mlibc's static TLS block; futex `17`/`18` (`:173-191`); threads on `Spawn = 5`; libunwind's `.eh_frame_hdr` lookup through `dl_iterate_phdr` (plan R3). The facilities exist as kernel primitives; their mlibc wiring is R2-R3 |
| 4 | Budget (16 MiB heap quota, 32 MiB VA stride, 256 KiB stack) | **Restated for this profile** (below) |
| 5 | No dynamic loading, JIT, file-backed shared mappings or async signals | Holds. `dlopen` is refused, and Mesa tolerates `NULL` (port gaps §3). The anonymous-memory primitive allows no `PROT_EXEC` (large-memory CP1 D-M7). GPU shader compilation produces GPU code in GPU buffers, not CPU-executable pages. The 1 GiB memfd is emulated over one anonymous reservation (CP1 D-M12), not a file mapping |

### Budget (amends condition 4 for `cpp-hosted` only)

The 16 MiB heap quota and 32 MiB VA stride cannot hold an LLM or anv's state pools. A `cpp-hosted` Cell's large allocations go through the approved large-memory CP1, `AnonMemory = 440` ([cp1-tier2-large-memory.md](../../.agents/261104-0801-x86-igpu-compute/cp1-tier2-large-memory.md), Law-1 confirmation 1 approved 2026-11-04 per its status line, `:3`):
- **Budget source.** The limit is a signed per-path `POLICY.BIN` v9 row `anon_limit_mib`. Without a verified row (development profile), it is the system-config default `memory.tier2_anon_default_mib`, whose default is **0**, meaning refused (D-M4, `:109-118`).
- **Charging.** Committed frames and their page-table frames are charged to a per-Cell anonymous row (D-M5, `:120-126`). The window is 64 GiB, with at most 64 MiB committed per call (D-M14, `:180-183`).
- **What stays as before.** The kernel-heap quota (16 MiB) still bounds the Cell's kernel-side metadata. The 256 KiB default stack stays for the root thread. Thread stacks come from anonymous memory.
- **Admission.** Production admission of a `cpp-hosted` artifact on x86 is a separate decision (plan D-R10; CP1 D-M10).

### Licence

- `libcxx/LICENSE.TXT`, `libcxxabi/LICENSE.TXT` and `libunwind/LICENSE.TXT` on llvm-project `main` state "Apache License v2.0 with LLVM Exceptions" (read 2026-11-04). libc++ additionally keeps its legacy NCSA/MIT dual licence.
- SPDX: `Apache-2.0 WITH LLVM-exception`, already in the `deny.toml` allow list (`deny.toml:38`).
- [ADR-0030](0030-license-model-and-copyleft-boundary.md) reuse order 1 (a permissive SPDX identifier per file, `0030-…:39-46`) applies. The LLVM exception waives Sections 4(a), 4(b) and 4(d) for portions embedded in object form.
- Implementation pins one llvm-project tag and re-checks the licence of every vendored file.
- Images that ship the runtime carry the LLVM attribution in `NOTICE`.

### Size measurement (required before first ship)

R4 records, in this ADR's Consequences, the bytes added to a release, stripped Cell by libc++ + libc++abi + libunwind:
- for the witness Cell;
- for a Mesa-only link;
- with `-ffunction-sections -Wl,--gc-sections`.

A `cpp-hosted` Cell does not ship in an image before that number is recorded (Spec 05 `:204-205`).

## Considered options

| Option | Outcome | Reason |
|---|---|---|
| LLVM libc++ / libc++abi / libunwind, static, over mlibc | **Chosen** | Permissive (`Apache-2.0 WITH LLVM-exception`, already allowed); clang-native; libunwind locates `.eh_frame` through `dl_iterate_phdr`, which mlibc provides |
| GNU libstdc++ + libsupc++ + libgcc_s unwinder | Rejected | GPL-3.0 with the GCC Runtime Library Exception: a new licence class for `third_party/` beside ADR-0030's model; GCC-only build glue; mlibc already works around libstdc++ assertion symbols (`third_party/mlibc-src/meson.build:628-629`) |
| Two runtime builds (with and without exceptions) for Mesa and llama.cpp | Rejected | Mesa and llama.cpp live in one Cell (G3); two libc++ builds would collide on symbols |
| Extend `cpp-freestanding` with header-only STL subsets | Rejected | Does not provide exceptions, RTTI, `<thread>` or iostreams for llama.cpp; the language-only profile's absence assertions stay meaningful as they are |
| Run llama.cpp + anv as a Tier-3 Linux guest | Rejected here | Contradicts ADR-0033 §3 (native Tier-2 Cell with a transport shim) |

## Consequences

**At R4 close.** These doc changes are listed here and applied only when R4 closes:
- `docs/guides/tier1b-c-zig.md`:
  - add a `cpp-hosted` section (flags, libc tier, facilities, refused features, witness, size);
  - correct the `cpp-freestanding` architecture line (`:288-290`), which says "RV64 and AArch64 only" although the code admits x86_64 (`libs/api/src/services/posix.rs:13-22`; `cells/tests/cpp-smoke/build.rs:126-152`);
  - update the x86_64 Level-B note (`:483-485`) once R2 lands.
- `docs/specs/05-application.md` §3.2: add `cpp-hosted` beside `cpp-freestanding`, and correct the "Admitted on RV64 and AArch64 only" paragraph (`:207-210`) as above.
- `docs/specs/18-cell-trust-tiers.md:58-60`: add `cpp-hosted` to the Tier-2 profile list.
- `docs/posix-ledger.md`: Tier-B C++ ABI rows (`__cxa_*`, `_Unwind_*`, `operator new`/`delete` from libc++abi).

**Unchanged.** `cpp-freestanding`, its runner assertions (`scripts/qemu-cpp-smoke.sh:52-54`) and its Tier-A runtime layer.

**Witness.** A `cpp-hosted-smoke` x86 Tier-2 Cell:
- containers, `<charconv>`, `throw`/`catch` across frames, `dynamic_cast`;
- `std::thread`, `std::future`, `thread_local`, a contended thread-safe static;
- `std::cout`;
- an uncaught exception that terminates only the Cell.

The evidence ceiling is QEMU TCG and QEMU/KVM.

**Size measurement (R4, 2026-11-05; host build, not run).** Bytes the three runtime archives add,
as input sections that `--gc-sections` keeps, attributed from the lld link map by
`bash scripts/build-llvm-runtimes.sh size`. The build is pinned at `llvmorg-19.1.7`, uses
`-O2`/`Release` with `-ffunction-sections -fdata-sections`, and links over mlibc `posix` x86_64.

| Link | libc++ | libc++abi | libunwind | Runtime total | Runtime `.bss`/`.tbss` | Stripped static PIE |
|---|---:|---:|---:|---:|---:|---:|
| Witness logic (`cpp-hosted-smoke/cpp/witness.cpp`) | 327,044 | 140,578 | 25,533 | **493,155** | 8,337 | 1,234,792 |
| Mesa-shaped probe (`-fno-exceptions -fno-rtti`, containers, `<optional>`, `<variant>`, `<charconv>`; `cpp-hosted-smoke/cpp/mesa_probe.cpp`) | 4,534 | 135,689 | 25,407 | **165,630** | 2,655 | 769,168 |

- The witness Cell itself, linked by cargo, attributes exactly the witness row: 327,044 + 140,578 + 25,533 bytes. Its stripped ELF is 1,309,224 bytes.
- `.eh_frame` and `.eh_frame_hdr` are linker-generated, so lld does not attribute them per archive. Whole-image values are 157,416 bytes for the witness and 106,844 bytes for the probe, covering mlibc and C++ together.
- The stripped totals include mlibc. Its 16 MiB arena is `.bss` and adds no file bytes.
- Even a Mesa-only link carries most of libc++abi. Throwing `operator new` reaches `std::terminate`, and the default terminate handler pulls the demangler, about 100 KiB.

**Compiler builtins (decided at the first link).** They come from `libgcc.a` of the compiler that built mlibc, linked by `libs/mlibc-shim` ahead of Rust `compiler_builtins`. compiler-rt is not built. The witness Cell also links with `--no-allow-multiple-definition` passed explicitly, and its link map shows no `compiler_builtins` member extracted.

**Risks.**
- Compiler-runtime builtins: `libgcc.a` is the only provider today (above). lld rejects duplicate strong definitions by default, so a toolchain change that makes two providers collide fails the link rather than choosing silently.
- Size growth.
- Missing libc features. Each one is fixed in mlibc with a ledger row, never stubbed in libc++.

## Related

- [ADR-0018](0018-cell-native-portability-and-runtime-profiles.md), [ADR-0030](0030-license-model-and-copyleft-boundary.md), [ADR-0033](0033-x86-intel-igpu-native-compute.md)
- Plan [261104-0802-x86-tier2-gpu-runtime](../../.agents/261104-0802-x86-tier2-gpu-runtime/plan.md): phases R2, R3, R4
- [Large-memory CP1](../../.agents/261104-0801-x86-igpu-compute/cp1-tier2-large-memory.md); [port gaps](../../.agents/261104-0801-x86-igpu-compute/research/mesa-anv-port-gaps.md)
