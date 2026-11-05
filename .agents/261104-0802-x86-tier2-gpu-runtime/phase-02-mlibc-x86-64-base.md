---
phase: 2
title: "mlibc x86_64 base sysdeps: pin, CRT, syscall thunk, TLS"
status: done-at-link-ceiling (owner approved 2026-11-04, [record](r2-deviations-approval.md); runtime witnesses open under H-1 and R3)
priority: P1
effort: "3-5d"
dependencies: ["R1"]
tier: thinking
checkpoint: false
---

# Phase R2: mlibc on x86_64 (base)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [plan.md](plan.md) D-R8; [port gaps](../261104-0801-x86-igpu-compute/research/mesa-anv-port-gaps.md) §3 row "mlibc on x86_64", §6 G-b
- `docs/posix-ledger.md` §Tier B (`:261-294`); `docs/guides/tier1b-c-zig.md` §TLS (`:298-337`), §x86_64 Support (`:483-485`)

## Overview

R2 makes Tier-B mlibc build and start on x86_64 with its current option set (`posix` still off). It also pins upstream and ports the Cellos sysdeps to the API at that pin. The result is a Tier-2 x86 Cell that runs `printf`, `malloc`, `clock_gettime` and `errno`, with a working static TLS block.

## Key Insights

**Cellos side today:**
- The vicell sysdeps enable only `ansi` (`third_party/mlibc/sysdeps/vicell/meson.build:10-15`).
- They ship CRTs for riscv64 and aarch64 and `error()` on any other CPU family (`:52-59`).
- `sys_tcb_set` is a no-op off RV64/AArch64 (`third_party/mlibc/sysdeps/vicell/generic/generic.cpp:61-70`).
- `scripts/build-mlibc.sh` builds riscv64 and aarch64 (`:2`). It clones upstream unpinned with `--depth 1` (`:39`) and only logs the commit (`:41`).
- `libs/mlibc-shim/build.rs` selects `third_party/mlibc/build-aarch64` or `third_party/mlibc/build` (`:24-28`), so there is no x86 directory.
- The guide states Level B is unavailable on x86_64 (`docs/guides/tier1b-c-zig.md:483-485`).

**Upstream supports x86_64.**
- The local clone `third_party/mlibc-src` is at `e45676e5` (2026-09-04), MIT (`LICENSE`).
- It has `options/internal/x86_64/` (`setjmp.S`, `fenv.S`, …) and x86_64 sysdeps for linux, managarm, keyronex, ethereal and obos.

**API drift** (confirmed 2026-11-04: the old sysdeps fail at `e45676e5` with `'sysdep_return_t' does not name a type`). At that commit, mlibc dispatches sysdeps through `mlibc::sysdep<Tag>`:
- `options/rtld/generic/main.cpp:358`;
- `all-sysdeps.hpp:9,17` includes a sysdeps-provided `mlibc/sysdeps.hpp` and requires `Sysdeps<Tag>`;
- `SysdepImpl<Tag>` specialisations are declared in `sysdep-signatures.hpp:28-42`.

Vicell defines `sys_*` free functions (`generic.cpp:61`), and its `include/` tree has `abi-bits/` but no `mlibc/sysdeps.hpp`. It therefore most likely does not build at the clone's commit, and the RV64/AArch64 Tier-B lane is affected too. No `libc.a` build directory exists under `third_party/mlibc/`. Managarm's `sysdeps/managarm/include/mlibc/sysdeps.hpp` is the model to follow.

**Kernel interfaces reused (no new syscall):**
- x86 `syscall` with `rax` = number and `rdi`, `rsi`, … as arguments (`libs/ostd/src/syscall.rs:131-136`).
- `SetTlsBase = 9`: self-only, writes `FS_BASE`, reinstalled on every resume (`libs/api/src/abi/syscall.rs:161-172`; `tier1b-c-zig.md:325`).
- `GetTime = 120` (`syscall.rs:254`): x86 op 0 is nanoseconds (`tier1b-c-zig.md:457`).
- `Exit = 60` (`syscall.rs:33`), `Log = 11` (`syscall.rs:226`).

**TLS.** The guide says C `__thread` is unsupported because no runtime allocates `PT_TLS` blocks (`tier1b-c-zig.md:332-337`). mlibc's static start-up allocates the TCB and static TLS block itself, then calls `TcbSet` (`options/rtld/generic/main.cpp:358`, `:673`). With `TcbSet` mapped to `SetTlsBase`, mlibc becomes that runtime for mlibc Cells `[inference: verify the static-link path in step 4]`.

## Requirements

- Pin one mlibc commit, recorded in the script and in `third_party/README.md`. Re-clone deterministically.
- Port the vicell sysdeps to the `Sysdeps<Tag>` API for all three architectures, preserving every Tier-B ledger row's behaviour.
- `crt-x86_64/crt1.S`: an empty stub like the others, since `ostd` provides `_start`.
- x86 syscall thunk; `TcbSet` → `SetTlsBase`; x86 build row → `third_party/mlibc/build-x86_64/libc.a`; `mlibc-shim` picks the x86 directory.
- Build with the R1 meson cross file: SSE2 hard-float, `-fno-exceptions -fno-rtti` for mlibc internals (`third_party/mlibc/sysdeps/vicell/meson.build:18-30`).

## Architecture

```text
ostd _start ─> Rust host main ─> __libc_start_main (mlibc)
  mlibc static init: TCB + PT_TLS block ─> Sysdeps<TcbSet> ─> SetTlsBase(9) ─> FS_BASE
  stdio/malloc/clock ─> Sysdeps<Write|AnonAllocate|ClockGet> ─> existing syscalls
```

## Related Code Files

- Create: `third_party/mlibc/sysdeps/vicell/crt-x86_64/crt1.S`, `third_party/mlibc/sysdeps/vicell/include/mlibc/sysdeps.hpp`, x86 syscall header under `third_party/mlibc/sysdeps/vicell/include/`, an x86 runner for `cells/tests/mlibc-smoke`
- Modify: `third_party/mlibc/sysdeps/vicell/{meson.build,generic/*.cpp}`, `scripts/build-mlibc.sh`, `scripts/setup-mlibc.ps1`, `libs/mlibc-shim/build.rs`, `third_party/README.md`, `docs/posix-ledger.md` (Tier-B rows), `docs/guides/tier1b-c-zig.md` (`:483-485`)
- Not touched: `libs/api/**` (if the `mlibc` feature needs a change on x86, stop and open a Law-1 CP1), `kernel/**`, `hal/**`

## Implementation Steps

1. Pin the commit, then build RV64 and AArch64 at the pin. Record whether the current sysdeps compile; this confirms or refutes the API-drift finding.
2. Port the sysdeps to `Sysdeps<Tag>` with no behaviour change. Re-run `cells/tests/mlibc-smoke` on RV64 (`docs/posix-ledger.md:292-294`) and add a marker runner.
3. Add the x86 CRT, syscall thunk, `TcbSet` and build row.
4. Run mlibc-smoke on x86 Tier-2 (QEMU TCG/KVM, development profile). Extend it with a `__thread` variable and `errno` after a failing `open`.

## Todo List

- [x] Pin mlibc — `ae1092eb211e42e80d2d1c73881c8c25fde80fe6` (2026-03-26) in `scripts/build-mlibc.sh`, `scripts/setup-mlibc.ps1`, `third_party/README.md`
- [x] Port to `Sysdeps<Tag>` (all arches) — riscv64 and x86_64 built; aarch64 unbuilt (no compiler, see log)
- [x] x86 CRT, thunk, TLS — `crt-x86_64/crt1.S`, `include/vicell/syscall.h` x86_64 branch, `TcbSet` → `SetTlsBase`; static TLS start-up still missing (see log)
- [x] Build row + shim directory — `scripts/mlibc-x86_64.cross`, `third_party/mlibc/build-x86_64/`, `libs/mlibc-shim/build.rs`
- [ ] x86 smoke + RV64 regression runner — link proofs pass on both; runtime blocked (see log)
- [x] Ledger and guide rows — 18 Tier-B rows, checker recognises `Sysdeps<Tag>`

## Success Criteria

- [x] mlibc-smoke prints its markers on x86 Tier-2 under TCG and KVM. A `__thread` variable reads back its initial image, and `errno` equals `ENOENT` after a failing `open`. — Met 2026-11-04 by slice RE: `mlibc-smoke` 3/3 and `mlibc-posix-x86` (`tls-errno`, `errno`) under TCG and KVM ([evidence](../../docs/evidence/mlibc-runtime-x86.txt)).
- [ ] The RV64 smoke still passes at the pinned commit. — Links at the pin; the RV64 run did not resolve `/bin/mlibc-smoke` from the disk image ([slice RE](slice-runtime-enablement.md) item 8). Still open.
- [x] Two clean builds produce the same `libc.a` hash. — riscv64 `d7a7a045…6333`, x86_64 `72078752…88d0`, identical across two `git clean -ffdx` rebuilds.

## Risk Assessment

- **The upstream port touches RV64/AArch64 Tier-B too.** Mitigation: run step 2 before any x86 work. RV64 regression is the gate.
- **mlibc may expect `rtld`-style auxv/phdr discovery in static builds.** Mitigation: `ostd` crt0 already walks `__init_array` (`libs/ostd/src/startup.rs`, per `tier1b-c-zig.md:275-276`). Provide `__ehdr_start`-based phdr discovery in R3.

## Security Considerations

The TLS base carries no authority (`syscall.rs:164-166`). A bad pointer faults inside the Tier-2 domain. Nothing new is exposed to other Cells.

## Next Steps

R3 enables `posix` on this base.

## Deviation Log

1. **Surprise — compiler floor.** From upstream `5c81d6fc` (2026-03-31) mlibc uses C++23 explicit object parameters (GCC ≥ 14, clang ≥ 19). The host has GCC 13.2 and clang 18.1 (also Zig 0.13 = clang 18); clang 18 additionally fails on alias-template CTAD and libstdc++ `<expected>`. **Decision:** pin `ae1092eb` (2026-03-26), found by `git bisect` over `dbe26313..e45676e` with a GCC-13 riscv64 build; it already has the `Sysdeps<Tag>` API (`dbe26313`, 2026-03-17). Bumping past it needs GCC 14 on every build host.
2. **Deviation — x86 compiler/cross file.** The R1 `meson/cellos-x86_64.ini` (`system = 'cellos'`, clang via `cellos-cc`) cannot build mlibc: mlibc dispatches on `host_machine.system() == 'vicell'`, and clang 18 fails (item 1). New `scripts/mlibc-x86_64.cross` uses the host `g++` freestanding with the same SSE2 float contract (`-msse2 -mfpmath=sse`) plus `-fPIE -mno-red-zone -mcmodel=small`.
3. **Deviation — sysdep surface.** Upstream merged `fstat`/`stat` into one `Stat` tag (`fsfd_target`), so the ledger has 18 rows, not 19. `Isatty` now returns `ENOTTY` for fds > 2 (mlibc's contract) instead of a "not a terminal" success. `TcbSet` adopts upstream Linux offsets (RV64 `tp` = Tcb + `sizeof(Tcb)`; AArch64 `TPIDR_EL0` = Tcb + `sizeof(Tcb)` − 0x10); the old code wrote the raw Tcb pointer, which mismatches mlibc's TLS layout. Nothing calls `TcbSet` yet (item 6), so no running Cell changes.
4. **Deviation — headers and build flags.** `abi-bits/` now copies upstream `abis/linux/` at the pin for every header except the Cellos-specific `fcntl.h`, `seek-whence.h`, `stat.h`, `vm-flags.h`; `aio.h`/`sched_param.h` (absent upstream at the pin) are removed; `errno.h` gains the full Linux set (`ENOTSUP` 134 → `EOPNOTSUPP` 95). Added `AT_FDCWD` to `fcntl.h`, `-D_GNU_SOURCE` for C++ (Linux g++ predefines it; bare-metal toolchains do not), and the rtld include/source lines (`ld.a` is built for static programs too).
5. **Surprise — riscv64 link.** Rust's prebuilt `compiler_builtins` for `riscv64gc-unknown-none-elf` carries soft-float C objects (`bswapsi2.o`); lld refuses them next to lp64d code once mlibc needs `__bswapsi2`. **Decision:** each build row ships the building compiler's `libgcc.a`; `mlibc-shim` links it right after `libc.a` (upstream's `libgcc_dependency` idea). x86_64 linked without it but uses the same contract.
6. **Blocked — TLS and runtime witnesses.** ostd's `_start` never enters mlibc's static start-up, so no Tcb or `PT_TLS` block is allocated and `TcbSet` is never called; `__thread` and `errno` (thread-local) would fault on x86 (`FS_BASE` = 0). The smoke Cell therefore gets no `__thread`/`errno` markers yet; R3 owns the start-up path. Running any mlibc Cell needs a launch row (H-1, S1-held `kernel/src/loader/launch_profile/*`) and, on RV64, policy/disk rows (`scripts/sign-policy.py` is S1-held).
7. **Risk — variadic calls.** Soft-float Rust calling SSE2 `printf` leaves `%al` unset, so the callee may spill `xmm0-7`. Harmless only while the kernel preserves Cell SSE state (R5); `scripts/check-float-ffi-boundary.py` does not flag variadics. Recorded in the guide.
8. **Deviation — files outside the R2 set.** `scripts/check-posix-ledger.py` and its test (Tier-B discovery now matches `Sysdeps<Tag>::operator()`), and one `.gitignore` line for `build-x86_64/`. Neither is S1-held. The checker passes all Tier-B rows; its 20 remaining Tier-A violations (`docs/posix-ledger.md:254-259` anchors) predate R2.
9. **Unverified — aarch64.** No `aarch64-linux-gnu-g++` on the host and clang 18 cannot build the pin; the row prints `SKIP`. The AArch64 `TcbSet` change is compiled only by inspection.
