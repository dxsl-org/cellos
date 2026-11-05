# x86 Tier-2 runtime — R2 report and deviations approved

Recorded 2026-11-04. The owner (the user) answered **"duyệt"** ("approve") to the R2 report, which covered [phase-02 Deviation Log](phase-02-mlibc-x86-64-base.md#deviation-log) items 1–9:

| Item | Approved value |
|---|---|
| 1 | mlibc pin `ae1092eb211e42e80d2d1c73881c8c25fde80fe6` (2026-03-26), the newest upstream commit GCC 13 builds. Bumping past `5c81d6fc` needs GCC ≥ 14 on every build host |
| 2 | x86_64 mlibc uses `scripts/mlibc-x86_64.cross` (host GCC, freestanding, SSE2, PIE), not R1's `meson/cellos-x86_64.ini` |
| 3 | Sysdep surface follows upstream: one `Stat` tag (18 Tier-B rows), `Isatty` → `ENOTTY` above fd 2, upstream `TcbSet` offsets |
| 4 | `abi-bits/` copies upstream `abis/linux/` at the pin except `fcntl.h`, `seek-whence.h`, `stat.h`, `vm-flags.h`; `AT_FDCWD`, C++ `-D_GNU_SOURCE`, rtld include/source lines |
| 5 | Each build row ships its compiler's `libgcc.a`; `libs/mlibc-shim` links it after `libc.a` |
| 6 | `__thread`/`errno` and every runtime witness stay open: mlibc static start-up moves to R3; launch rows wait for H-1 |
| 7 | Variadic soft-float → SSE2 calls are a known risk until R5 preserves Cell SSE state |
| 8 | Out-of-set edits accepted: `scripts/check-posix-ledger.py`, its test, one `.gitignore` line |
| 9 | aarch64 mlibc stays unbuilt until a host has `aarch64-linux-gnu-g++` (or GCC ≥ 13 for aarch64) |

**Effect:** R2 closes at the link-proof ceiling. R3a (threads, futex, time, environment, ELF, plus the mlibc static start-up from item 6) opens on the H-5 shared files. R3b still waits for the large-memory CP2.

| Path | SHA-256 at approval (HEAD `fb79d0fa`, uncommitted tree) |
|---|---|
| `.agents/261104-0802-x86-tier2-gpu-runtime/phase-02-mlibc-x86-64-base.md` | `175d4621333fe413021266bf782a6f64c283b0404f6261f06c1958595fe7ed59` |
| `scripts/build-mlibc.sh` | `8368bbe848b7f89cc046c77e13b5d62357399e6ce3218816c193dbb00b6da93c` |
