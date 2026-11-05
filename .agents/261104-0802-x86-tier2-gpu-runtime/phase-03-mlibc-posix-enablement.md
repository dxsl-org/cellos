---
phase: 3
title: "mlibc posix on x86_64: threads, futex, time, environment, memory, memfd, ELF introspection"
status: in-progress (R3a; opened 2026-11-04 after the R2 approval)
priority: P1
effort: "9-16d"
dependencies: ["R2", "large-memory CP1 implemented (R3b only)"]
tier: thinking
checkpoint: false
---

# Phase R3: mlibc `posix` enablement

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [plan.md](plan.md) D-R8; [port gaps](../261104-0801-x86-igpu-compute/research/mesa-anv-port-gaps.md) §2 (Mesa OS surface), §3 rows "Threads" to "dladdr", §6 G-c, G-d, G-g
- Dependency: [cp1-tier2-large-memory.md](../261104-0801-x86-igpu-compute/cp1-tier2-large-memory.md) (draft by a parallel session, not approved)
- [ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md) §2.1 (substitution policy), §2.5 (fail loudly)

## Overview

R3 turns on mlibc's `posix` option for vicell and implements the sysdeps that Mesa's compute path and llama.cpp reach. It has two parts:
- **R3a** (no kernel dependency): threads, futex, time, sleep, sysconf, environment, ELF introspection.
- **R3b** (after the large-memory CP1 is implemented): real `mmap`/`munmap`/`mprotect`, a thread-safe heap, and `memfd` emulation.

## Key Insights

**What exists today:**
- `posix`, `linux`, `glibc` and `bsd` are off (`third_party/mlibc/sysdeps/vicell/meson.build:10-15`).
- Tier-B futex wait is a yield loop that ignores the timeout, and wake is a no-op (`third_party/mlibc/sysdeps/vicell/generic/generic.cpp:72-86`; `docs/posix-ledger.md:276-277`).
- The heap is a 4 MiB bump arena that is never freed, and `vm_map` supports only `MAP_ANONYMOUS` (`docs/posix-ledger.md:284-287`).

**Kernel primitives to reuse (no new syscall):**
- `FutexWait = 17`: keyed by address space, generation and address. Its timeout is in 10 ms ticks, and `0` means forever (`libs/api/src/abi/syscall.rs:173-186`).
- `FutexWake = 18` (`:187-191`).
- `Spawn = 5` creates a thread that inherits its creator's authority under one scheduler lock (`kernel/src/task/syscall.rs:6069-6079`). The port-platform pthread subset already uses it (`libs/port-platform/cellos_pthread.c:113-115`).
- A worker's `Exit` ends only that thread (`docs/guides/tier1b-c-zig.md:327-330`).
- `Yield = 104`, `SetTimer = 35` (`syscall.rs:220-221`); `MemInfo = 243` (`:148`); `GetTime = 120` (`:254`).

**mlibc at the local clone `e45676e5` already contains:**
- `posix`: `sysconf` (`options/posix/generic/unistd.cpp`), `secure_getenv` (`posix_stdlib.cpp`), `memfd_create` (`sys-mman.cpp`), `dladdr` (`dlfcn.cpp`), `open_memstream` (`posix_stdio.cpp`), `newlocale` (`posix_locale.cpp`), `pthread_setaffinity_np` (`pthread.cpp`).
- `ansi`: `asprintf`, `strtok_r`, `qsort`, `clock_nanosleep`.
- `elf`, built unconditionally (`meson.build:580`): `dl_iterate_phdr` (`options/elf/generic/phdr.cpp`).
- `sched_getaffinity` lives in `linux` (`options/linux/generic/sched.cpp`). Mesa treats it as optional (port gaps §2), so `linux` stays off.

**Sysdep tags to implement** (`options/internal/include/mlibc/sysdep-tags.hpp`):
- `ThreadExit` (`:27`), `PrepareStack` (`:32`), `Clone` (`:35`), `FutexWait` (`:38`) and wake, `TcbSet` (`:106`);
- `VmMap`/`VmUnmap`/`VmProtect` (`:130-134`), `Ftruncate` (`:242`), `MemfdCreate` (`:454`), `Sysconf` (`:512`);
- `Ioctl` (`:398`) belongs to R6.

**Large memory.** The parallel CP1 draft recommends one op-multiplexed `AnonMemory = 440`:
- ops MAP / UNMAP / PROTECT; `FIXED` inside one's own reservation; reserve-only `PROT_NONE`; sub-range commit;
- no `PROT_EXEC` and no two-VA aliasing in v1;
- `440-442` is listed as the alternative shape.

None of this is approved. R3b codes against whichever shape is approved at that CP's CP2 (coordination message to `Tier2LargeMemoryCp1`, 2026-11-04).

**Mesa fails hard without a build-id.** `vkEnumeratePhysicalDevices` fails without `dl_iterate_phdr` + `dladdr` + a build-id note (port gaps §2 row "dladdr + dl_iterate_phdr"). libunwind needs `dl_iterate_phdr` for `.eh_frame_hdr` (R4).

## Requirements

**R3a:**
- `Clone`/`PrepareStack`/`ThreadExit` over `Spawn`/`Exit`, each thread getting its own TCB and TLS via `SetTlsBase`.
- Futex wait/wake mapped to 17/18:
  - timeouts are rounded **up** to ticks;
  - a zero-length timeout never reaches the kernel, because `0` means forever there. It returns `ETIMEDOUT` after one value check.
- `clock_gettime` (`MONOTONIC`/`REALTIME`); `nanosleep`/`clock_nanosleep` via a timed futex wait on a private word.
- `sysconf`:
  - `_SC_PAGESIZE`;
  - `_SC_PHYS_PAGES` from `MemInfo`;
  - `_SC_NPROCESSORS_*` from an existing query `[inference: confirm which syscall reports the CPU count]`, else 1.
- `getenv`/`secure_getenv` over a static environment (empty by default).
- `getpid` returns a fixed documented value. A Cell is one process, and the value is recorded in the ledger as a semantic subset.
- `sched_yield`.
- `dl_iterate_phdr`/`dladdr` for the single static image (`__ehdr_start`), plus `-Wl,--build-id=sha1` in the Rust host link.
- Every unsupported call returns `ENOSYS`/`EINVAL` and sets `errno` (ADR-0018 §2.5).

**R3b:**
- `VmMap`/`VmUnmap`/`VmProtect` on the approved large-memory primitive, and the mlibc slab allocator over it.
- `memfd`: one reserve-only range per memfd. `ftruncate` commits up to the new size, and every `mmap(memfd, off)` returns `base + off`. Two distinct VAs for the same pages are refused with `ENOSYS`.

## Architecture

```text
pthread_create ─> Sysdeps<Clone> ─> Spawn(5, trampoline, tcb) ─> trampoline: SetTlsBase, switch to prepared stack
pthread_cond_timedwait ─> Sysdeps<FutexWait> ─> FutexWait(17, ticks = ceil(ns / 10ms))
mmap(ANON) / mprotect / munmap ─> Sysdeps<VmMap|VmProtect|VmUnmap> ─> large-memory CP1 syscall (R3b)
memfd_create + ftruncate + mmap(fd) ─> reserve once, commit on growth, base + off (R3b)
```

## Related Code Files

- Modify: `third_party/mlibc/sysdeps/vicell/{meson.build,generic/*.cpp,include/**}`, `docs/posix-ledger.md` (Tier-B rows), `docs/guides/tier1b-c-zig.md` (Level B), `libs/cell-build` linker arguments for `--build-id` (if the linker script lives there)
- Create: `cells/tests/mlibc-posix-x86/**` (witness Cell + runner)
- Not touched: `libs/api/**`, `kernel/**`. The large-memory syscall arrives through its own CP.

## Implementation Steps

1. Verify `Spawn` from an x86 private-root task:
   - the thread shares the root;
   - who supplies the user stack;
   - TLS inheritance (`tier1b-c-zig.md:327-328`).
   Record the result before writing `Clone`.
2. R3a sysdeps in the order: futex → threads → time and sleep → sysconf and env → ELF.
3. Witness R3a:
   - 8 threads exercising mutex, condvar (timed), `call_once`, `thread_local` and `errno` isolation;
   - `clock_nanosleep` never returns early;
   - `dl_iterate_phdr` finds `PT_GNU_EH_FRAME` and the build-id note.
4. After the large-memory CP2: R3b sysdeps. Witness:
   - 256 MiB anonymous map, then `mprotect` `PROT_NONE` → RW, then `munmap`, then reuse;
   - 1 GiB memfd reserve, grown in 64 MiB steps, re-mapped at the same base;
   - slab under 8-thread contention.

## Todo List

- [x] Spawn/stack/TLS fact check (Deviation Log 1)
- [x] Futex + threads — `generic/thread.cpp`, `thread-entry/<arch>.S`; link-proven riscv64 + x86_64
- [x] Time, sleep, sysconf, env, getpid — `generic/posix.cpp`; environment comes from mlibc's start-up (empty in CP1 v1)
- [x] `dl_iterate_phdr`, `dladdr`, build-id — mlibc's own (posix/elf options); build-id from the witness `build.rs` (H-6), note present in a mapped `PT_NOTE`
- [x] R3a witness — `cells/tests/mlibc-posix-x86` 7/7 on x86 QEMU TCG and KVM via `scripts/x86/qemu-mlibc-witness.sh` (slice RE, [evidence](../../docs/evidence/mlibc-runtime-x86.txt))
- [ ] R3b memory + memfd (after the large-memory CP2)
- [x] Ledger rows — 27 Tier-B rows; checker scans every `generic/*.cpp` and rejects duplicate tags
- [x] mlibc start-up wiring — kernel initial stack + auxv, ostd hook, `mlibc-shim` anchor, mlibc pin patch 0001 ([slice RE](slice-runtime-enablement.md); CP2 pending)

## Success Criteria

- [x] The R3a witness passes 100 runs under QEMU TCG and KVM (x86 development profile) with no hang. Timed waits are ≥ the requested time. — 2026-11-04: 100/100 in one boot on TCG and on KVM (`qemu64,+pdpe1gb`), every domain released, after [slice TL](slice-thread-exit-leak.md) (thread-exit domain leak) and [slice VL](slice-vfs-launch-log-retirement.md) (VFS 64-launch cap). Timed waits met their minimums.
- [ ] The R3b witness passes, and a Cell exit frees every mapping (checked by `MemInfo` before and after).
- [ ] `nm -u` of a static Mesa-like link (the G3 step-1 inventory) leaves no libc symbol unresolved except those listed as refused.

## Risk Assessment

- **10 ms tick granularity.** It makes `pthread_cond_timedwait` and Vulkan wait deadlines coarse `[inference: acceptable; port gaps §3 "Futex" row]`. Mitigation: round up, never early. A finer timer would be a separate kernel decision.
- **`Spawn` stack semantics may not fit mlibc's `Clone`.** Mitigation: a trampoline that switches to the mlibc-prepared stack. Step 1 settles it before any code.
- **The R3b schedule depends on another CP.** Mitigation: R3a, R4's no-thread parts and R6 do not wait for it.

## Security Considerations

- Futex keys are per address space and generation, so two domains never wake each other (`syscall.rs:182-185`).
- Memory operations act only on the caller's own private root. Authority stays with the large-memory CP1.
- No `PROT_EXEC` anonymous memory keeps W^X.

## Next Steps

R4 (C++ runtime on threads and TLS), R6 (fd provider and `Ioctl`).

## Deviation Log

1. **Step 1 fact check (code-read, 2026-11-04).** `Spawn{entry, arg}` → `scheduler.rs` `spawn_thread` (`kernel/src/task/scheduler.rs:1062-1201`): the thread shares the creator's address space (native domains), capabilities, allowlist, PKU and **TLS base**; the kernel allocates its own user stack of `stack_pages_for("thread")` = 16 pages = 64 KiB (`kernel/src/task.rs:703-713`), charged to the Cell quota; at most 32 threads per Cell (`scheduler.rs:22`); `arg` arrives as the first argument register. No syscall reports the caller's own tid or the hart count. A worker's `Exit` ends only that thread.
2. **Decision — thread stacks.** mlibc's default stack is 2 MiB (`threads.cpp:213`); the Tier-B heap is a 4 MiB static arena until R3b. Without a caller stack a thread therefore runs on the kernel's 64 KiB Spawn stack (`PrepareStack` reports 64 KiB); a caller-supplied stack (`pthread_attr_setstack`) is switched to. R3b moves the default to an anonymous mapping of the requested size.
3. **Decision — identities.** `getpid()` = 1; the root thread's tid = `0x3fffffff` (inside mlibc's 30-bit owner mask, above any realistic Spawn tid; tids are not reused); `sysconf(_SC_NPROCESSORS_*)` = 1; `_SC_PHYS_PAGES`/`_SC_AVPHYS_PAGES` from `MemInfo` or −1/`ENOSYS` without that allowlist bit.
4. **Fixes found while porting (R2 code).** `TcbSet` on riscv64/aarch64 wrote `tp`/`TPIDR_EL0` directly, which the kernel overwrites on the next resume — now `SetTlsBase` on every arch. `ClockGet(MONOTONIC)` multiplied op-0 ticks by 100 on every arch; x86_64 op 0 is already nanoseconds and aarch64's frequency is per board — now riscv64 op 0 × 100, aarch64 op 1 (ms) × 10⁶, x86_64 op 0 × 1. The arena bump pointer is now a CAS (threads).
5. **Deviation — headers.** Enabling `posix` broke the four Cellos-specific `abi-bits` (`stat.h` redefined `ino_t`; `fcntl.h` lacked `O_TMPFILE`). Their numeric values already equal Linux's, and `struct stat` never crosses into Rust (the `Stat` sysdep fills it in C++), so all four are now upstream copies too: every `abi-bits` header matches `abis/linux` at the pin.
6. **Deviation — sysdep objects.** Listing sysdep sources in `rtld_sources` put each one in the static `libc.a` twice (`link_whole` of `ld.a`); they now go to `rtld_dso_sources`, as upstream's Linux sysdeps do. `scripts/build-mlibc.sh`/`setup-mlibc.ps1` (R2 files) flip `-Dposix_option=enabled`.
7. **Surprise — start-up cannot be wired inside the R3 file set.** (a) Cell ELFs do not map their ELF/program headers (`cell.ld.in` starts `.text` at VA 0, no `PT_PHDR`; `__ehdr_start` names unmapped bytes), and the loader passes no auxv, so `PT_TLS` is not discoverable at run time. (b) ostd's `_start` runs `.init_array` before `main` (`libs/ostd/src/startup.rs:89-105`), and mlibc's own constructors (`init_libc`, `_GLOBAL__sub_I_*`; 7 in `mlibc-smoke` today) need mlibc's start-up first, while mlibc's start-up runs `.init_array` itself. `generic/startup.cpp` (`__vicell_mlibc_enter(load_base, phdrs, phnum)`) does the mlibc side; who calls it, and where the headers come from, is an owner decision (escalated 2026-11-04).
8. **Owner decision — option B (2026-11-04).** ostd hook + loader auxv: the kernel writes a Linux-shaped initial stack (argc, argv, envp, auxv with `AT_PHDR` copy and `AT_BASE` = load base) for every Cell, and ostd's `_start` calls a weak `__cellos_libc_start(entry_sp)` in place of its `.init_array` walk. Package: [cp1-cell-initial-stack-auxv.md](cp1-cell-initial-stack-auxv.md) (AX-1…AX-8, design check pending). `generic/startup.cpp` now implements `__cellos_libc_start` (filters `PT_DYNAMIC`/`PT_INTERP`, synthesises `PT_PHDR`, calls `__dlapi_enter`); it links on riscv64 and x86_64 and stays unexercised until the CP is implemented (lane + S1 release). Rejected: A (opt-in linker profile + init-array takeover), C (ostd hook + linker profile).
9. **Deviation — files outside the R3 set (none S1-held).** `scripts/build-mlibc.sh` and `scripts/setup-mlibc.ps1` (posix on; `--prefix=/` and a per-row `sysroot/include` from `meson install`, so C witnesses compile against mlibc's headers; vicell `meson.build` installs `abi-bits/`); `scripts/check-posix-ledger.py` + test (Tier-B discovery over every `generic/*.cpp`, duplicate tags rejected; 15 tests OK); `Cargo.toml` member line for `cells/tests/mlibc-posix-x86` (H-2, append-only; `Cargo.lock` gained its package entry).
10. **Witness build facts (x86_64, 2026-11-04).** `app-mlibc-posix-x86` links as a PIE with only relative relocations; it carries `PT_TLS` (`.tdata` 4 B for the `__thread` initial image), `PT_GNU_EH_FRAME` and a `PT_NOTE` holding `NT_GNU_BUILD_ID` inside a mapped `R` segment. Without `third_party/mlibc/build-x86_64/sysroot` (CI) the build script warns and the binary prints `MLIBC-POSIX: NOT-BUILT` and exits 1; riscv64/aarch64 builds take the same path, so `cargo build --workspace` stays green on every leg. Clippy `-D warnings` clean on riscv64, aarch64 and x86_64. Correction to item 7: `mlibc-smoke`'s 7 constructors are mlibc `_GLOBAL__sub_I_*` objects; `init_libc` is pulled only by Cells that use `environ`/argv.
11. **Owner approval (2026-11-04).** CP1 AX-1…AX-8 approved as recommended ([record](cp1-cell-initial-stack-auxv-approval.md)). The same day S1 closed and released the Intel WIP lane (`b3b5d51e`); implementing the CP needs that lane assigned to it.
