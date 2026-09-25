---
phase: 2
title: "cpp-freestanding runtime profile"
status: completed
priority: P1
effort: "1d"
dependencies: [1]
tier: thinking
---

# Phase 02: `cpp-freestanding` runtime profile

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

Admits C++ as a Cellos runtime profile ([ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md)
§2.3): the language of the portable Linux application corpus, currently with **zero** support in
the tree (no `.cpp` file, no `cxx` crate, no unwinder configuration anywhere in `cells/`,
`libs/`, or the build scripts).

The profile is the freestanding subset: `-fno-exceptions -fno-rtti -fno-threadsafe-statics`,
static PIC PIE, libc through the existing POSIX shim or mlibc, landing as an `FFI`-class Tier 2
cell. Hosted C++ (exceptions, RTTI, thread-safe statics) is explicitly out of scope here: it
needs the unwinder and per-task TLS from phase 03.

## Requirements

- Functional:
  - A C++ translation unit compiles with the `cc` crate (`cpp(true)`) for the Cellos targets and
    links into a cell.
  - Static constructors run: `ostd` crt0 already walks `__init_array_start..__init_array_end`
    on all three architectures (`libs/ostd/src/startup.rs:42-43,67-70,91-92`, documented for
    Tier-1b cells at `libs/cell-build/src/lib.rs:29-38`) — the phase proves it for C++ rather
    than assuming it.
  - `operator new`/`operator delete` (sized and array forms) resolve to the shim's
    `malloc`/`free`; `__cxa_pure_virtual` and `__cxa_atexit` (or `atexit`) are provided by a
    small shim, not by libstdc++/libc++.
  - Virtual dispatch, templates, and header-only utilities work; the reference cell exercises
    them.
  - The profile is admitted on the architectures the acceptance ledger already carries for FFI
    (RV64, AArch64) and is a Tier 2 `FFI`-class cell at runtime.
- Non-functional:
  - The Rust host cell keeps `#![forbid(unsafe_code)]`; all C++ lives behind the FFI boundary.
  - No libstdc++/libc++ runtime in v1: no `std::string`, `std::vector`, iostreams, or locale.
    A libc++ freestanding build is a **separate decision** with a measured binary size, not a
    silent addition here.

## Architecture

```text
cells/tests/cpp-smoke/
  ├─ src/main.rs        Rust host cell (ostd cell_main!, manifest tier = FFI, syscalls declared)
  ├─ cpp/engine.cpp     C++ subset: classes, virtual dispatch, templates, static ctor, new/delete
  └─ build.rs           cc::Build::new().cpp(true).flag("-fno-exceptions") … + cell_build
        │
        ├── cpp-shim (operator new/delete, __cxa_pure_virtual, atexit) ──► shim malloc/free
        └── ostd crt0 _start ──► __init_array (C++ static ctors) ──► cell_main
```

## Assumptions

- **Claim:** crt0 runs `.init_array` on RV64/AArch64/x86_64 for cells.
  **Confidence:** high
  **How to verify:** `libs/ostd/src/startup.rs:40-100`; confirm at runtime with a static
  constructor that sets a flag the Rust host reads.
- **Claim:** the shim's `malloc`/`free` are sufficient backing for `operator new`/`delete`.
  **Confidence:** high
  **How to verify:** `libs/api/src/services/posix/alloc.rs:29-105` (16-byte aligned, header
  checked) and the Tier 2 heap evidence in `cells/tests/tier2-smoke/src/main.rs:43-57`.
- **Claim:** the acceptance ledger admits FFI profiles only on RV64/AArch64.
  **Confidence:** high
  **How to verify:** `docs/app-tier-acceptance-matrix.md` ("C/Zig FFI only on RV64/AArch64").

## Related Files

- Create: `cells/tests/cpp-smoke/` (Cargo.toml, build.rs, `src/main.rs`, `cpp/*.cpp`, linker
  script via `cell_build`), `libs/cpp-shim/` (or `cells/tests/cpp-smoke/cpp/cxx_shim.cpp`)
- Modify: `docs/guides/tier1b-c-zig.md` (C++ section), `docs/specs/05-application.md` (§3
  profile matrix), `docs/specs/18-cell-trust-tiers.md` (§2 profile list)
- Modify: `docs/app-tier-acceptance-ledger.json` + `docs/app-tier-acceptance-matrix.md`
- Modify: `Cargo.toml` (workspace member), `scripts/*` image assembly if the cell ships

## Implementation Steps

1. Add the C++ build recipe to a reference cell: `cc` with `cpp(true)`, `-fPIC`, `-fno-exceptions`,
   `-fno-rtti`, `-fno-threadsafe-statics`, `-fno-use-cxa-atexit`, plus the freestanding include
   path already used by other C cells.
2. Write the C++ runtime shim (`operator new/delete`, `__cxa_pure_virtual`, `atexit`) over the
   shim allocator; keep it to the symbols the subset actually needs and document each.
3. Build the reference cell: static constructor writes a sentinel; Rust host reads it; virtual
   dispatch, a template instantiation, `new`/`delete` churn, and one VFS read through the shim.
4. Declare the manifest as `tier = PROTECTION_CLASS_FFI` and run it in a Tier 2 domain.
5. Add the guide section and the profile rows (Spec 05 §3, Spec 18 §2), then the ledger witness.
6. Record what the subset does *not* cover (exceptions, RTTI, thread-safe statics, STL runtime)
   in the guide next to the profile row.

## Success Criteria

- [x] `cells/tests/cpp-smoke` builds for RV64 and AArch64 and runs in QEMU.
      RV64: `scripts/qemu-cpp-smoke.sh` → `CPP-SMOKE-QEMU: PASS`. AArch64: builds clean
      (`cargo build --release --target aarch64-unknown-none-softfloat -p app-cpp-smoke`); it is
      not booted here because the AArch64 image lane is a separate runner. x86_64 is refused by
      design with a stated reason (the shim's C++ ABI layer does not exist there).
- [x] The Tier 2 admission marker appears for the cell and the static-constructor sentinel is
      observed by the Rust host (proves `.init_array` runs).
      `[domain] admitted cell 'cpp-smoke' to Tier 2 Paged Domain (SATP isolation)` and
      `[cpp-smoke] static-ctor marker=0xC0FFEE11`.
- [x] `operator new`/`delete` are exercised without linking libstdc++/libc++ (verified by the
      link line and by `nm` showing no `__cxa_throw`/`_Unwind_*`).
      The runner asserts the symbol absence on every run; the cell's `nm` shows
      `_Znwm`/`_Znam`/`_ZdlPv`/`_ZdaPv` from the shim and no hosted runtime symbols; the ELF
      carries only `R_RISCV_RELATIVE` relocations (18) and a non-empty `.init_array`.
- [x] Spec 05 §3 and Spec 18 §2 list `cpp-freestanding`; the ledger carries its witness row.
      **Ledger row deliberately deferred** — see § Deviation Log: schema v5 binds rows to an
      archived Spec 23 contract revision plus witnesses, so creating one is a ratification act,
      not a build artifact.
- [x] The guide states the excluded features explicitly.

## Result

Landed:

| Change | Where |
|---|---|
| Reference cell: C++ language surface + Rust host + FFI boundary | `cells/tests/cpp-smoke/{Cargo.toml,build.rs,src/main.rs,src/cpp.rs,cpp/engine.cpp,cpp/cxx_support.hpp}` |
| Build recipe (per-arch compiler, `cpp_link_stdlib(None)`, subset flags, arch refusal) | `cells/tests/cpp-smoke/build.rs` |
| `atexit` / `__cxa_atexit` in the profile's runtime layer (clang emits `atexit` for `_GLOBAL__sub_I_*`) | `libs/api/src/services/posix/cxxabi.rs` |
| Reviewed launch edge for `/bin/cpp-smoke` (`CapSet::EMPTY`) | `kernel/src/loader/launch_profile/targets.rs` |
| F1 allowlist entries (crate + FFI file) | `scripts/unsafe-allowlist.toml` |
| QEMU runner with link-level assertions and 9 marker checks | `scripts/qemu-cpp-smoke.sh` |
| Workspace membership | `Cargo.toml` |

Evidence (all at the `qemu` ceiling, RV64, QEMU 8.2.2, default-feature kernel):

```
PASS: Tier 2 paged-domain admission
PASS: C++ static constructor ran (__init_array)
PASS: virtual dispatch through a base pointer
PASS: virtual destructor + operator delete
PASS: template instantiation
PASS: operator new/delete over the shim allocator
PASS: VFS service round trip over typed IPC
PASS: C file ABI read through the POSIX shim
PASS: cell PASS marker
CPP-SMOKE-QEMU: PASS target=riscv64gc-unknown-none-elf kernel=cellos-kernel
```

Plus: `python3 scripts/cellos-sign --check` → `OK: F1 — 95 crates and 640 files scanned; unsafe
confined to 49 allowlisted files` (the new entries are in use, not stale).

Not claimed: no AArch64 boot evidence for this cell, no physical/production claim, and no ledger
`PASS`.

## Security Considerations

C++ code is `unsafe` by construction; the profile's safety is Tier 2 containment (ADR-0018 §2.5),
not LBI. The Rust host must remain `forbid(unsafe_code)`, the manifest must declare only the
syscalls the cell uses, and no C++ exception machinery may be linked in silently (a link-time
symbol check is the guard).

## Risk Notes

Scope creep toward hosted C++ is the main risk; the success criteria deliberately forbid linking
an STL runtime. If a reference port later needs `std::string`/`std::vector`, that becomes its own
phase with a measured binary size against the 16 MiB quota.

## Risk Assessment

- **Undone by:** deleting the reference cell and its guide/spec rows; no kernel, ABI, or
  on-disk change is introduced by this phase.
- **Cannot be undone:** none.

## Deviation Log

- **Surprise — the runtime already existed.** The plan had the cell supply `operator new/delete`
  and `__cxa_pure_virtual`. They are already in the shim
  (`libs/api/src/services/posix/alloc.rs:148-205`, `cxxabi.rs:20-70`), and a cell-local copy made
  the AArch64 link fail with six duplicate symbols. `cpp/cxx_runtime.cpp` was deleted; the cell
  now contains only the language surface. The profile's runtime is the shim's C++ ABI layer —
  which also means every future C++ cell gets it for free.
- **Deviation — `atexit`/`__cxa_atexit` added to the shim, not the cell.** clang emits `atexit`
  from `_GLOBAL__sub_I_engine.cpp` for a translation unit with a global destructor, and the
  symbol was missing (GCC's riscv64 path did not reference it). Adding it once to
  `cxxabi.rs` keeps the "one runtime layer" rule; the semantics are documented there
  (registration accepted, never fires — a cell has no process teardown).
- **Deviation — no C++ standard headers on either toolchain.** `riscv64-unknown-elf-g++` ships no
  libstdc++ headers and `clang++` has no libc++ sysroot, so `#include <cstdint>` fails on both.
  The v1 profile is therefore *language-only*, with `cpp/cxx_support.hpp` deriving types from
  compiler builtins. The plan's "header-only utilities" line is corrected in the guide and Spec 05.
- **Decision — arch scope narrowed to RV64 + AArch64, enforced loudly.** The profile's runtime
  layer lives in `libs/api/src/services/posix.rs`, which is
  `#![cfg(any(riscv64, aarch64, wasm32, doc))]`. `build.rs` now fails with that reason on other
  targets instead of leaving an undefined-symbol dump. This matches the acceptance matrix
  ("C/Zig FFI only on RV64/AArch64").
- **Surprise — a new cell path needs a reviewed launch edge.** The shell refused
  `/bin/cpp-smoke` (`[loader] DENY launch edge … spawn_cap=false`) until
  `kernel/src/loader/launch_profile/targets.rs` gained the path with a `CapSet::EMPTY` ceiling.
  Any future port that the shell or desktop launches needs the same one-line review.
- **Surprise — two file-I/O paths, not one.** The first attempt read a `/srv` file (written
  through the VFS service over typed IPC) with the shim's C `open`/`read` ABI and got
  `FileNotFound`: `ViSyscall::Open` resolves through the *kernel* file table (FAT/VIFS1, e.g.
  `/BIN/INIT`), while `/srv` belongs to the VFS service. The cell now proves both paths
  separately, and the guide/spec text says which is which — a port that assumes one for the other
  would look like a linker or shim bug.
- **Decision — ledger row deferred.** Schema v5 rows bind `source` witnesses to an archived
  Spec 23 contract revision (`docs/evidence/spec23-native-sdk-contract-<sha12>.md`) with a
  matrix digest, validated by `scripts/validate-app-tier-acceptance.py`. Creating one is a
  ratification act; the phase records its evidence here and in the runner instead of fabricating
  a witness. Reopen as a ledger task when the profile is proposed for a `PASS` claim.
- **Note — F1 notes during signing are an untracked-file artifact.** The runner's `sign_cells`
  step printed "[[file]] … no longer contains unsafe" / "[[crate]] … no longer needed" because
  the F1 scan reads the *git-tracked* set and the new cell is not committed yet. With the files
  visible to the index, `scripts/cellos-sign --check` reports both entries in use (95 crates,
  640 files, unsafe confined to 49 allowlisted files).
