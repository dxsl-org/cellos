# QEMU + cross-toolchain: the "cannot build/boot here" blocker is wrong

**Date**: 2026-07-31 · **Why this matters**: phases 09, 10 and 11 all closed with
"runtime UNVERIFIED — no QEMU / cross toolchain on this machine". That premise is false
on this host. Two real but small problems were mistaken for a missing environment, and
they left W^X, the `NoEntry` gate and `cellos-sign` merged without ever booting.

## What is actually installed

`qemu-system-riscv64`, `qemu-system-aarch64`, `qemu-system-x86_64`, `pwsh`, and
`riscv64-unknown-elf-gcc` are all present and working. A full RV64 image was built and
booted to the shell prompt during this session.

## Problem 1 — toolchain is installed under a different triple

Every C-dependent `build.rs` hardcodes the xpack name when the per-target `CC` env var is
unset, e.g. `cells/runtimes/lua/build.rs:95-96` and `cells/demos/doom/build.rs:73-74`:

```rust
if std::env::var("CC_riscv64gc_unknown_none_elf").is_err() {
    build.compiler("riscv-none-elf-gcc");
}
```

Ubuntu installs `riscv64-unknown-elf-*`. `gen_disk.ps1` knows this and has
`Resolve-CrossTool` to fall back — but the fallback did not reach cargo (problem 2), so
cc-rs failed with `failed to find tool "riscv-none-elf-gcc"`.

**Workaround used**: a directory of symlinks named `riscv-none-elf-<tool>` pointing at the
installed `riscv64-unknown-elf-<tool>`, prepended to `PATH`. Covers gcc, ar, objcopy, ld,
as, g++, ranlib, strip, readelf, nm, objdump.

## Problem 2 — `gen_disk.ps1` sets CFLAGS that never reach cargo

`gen_disk.ps1:55-60` composes `CFLAGS_riscv64gc_unknown_none_elf` including
`-I third_party/freestanding-include`, which exists because bare-metal gcc ships no libc
headers and littlefs includes `<string.h>`. In the failing build the compiler line
contained every other flag from that variable but **not** the `-I`, and littlefs2-sys
failed with:

```
lfs_util.h:26:10: fatal error: string.h: No such file or directory
```

Reproduced directly: compiling `lfs.c` without the include fails, with it succeeds. So the
variable is being composed but not propagated intact to the cargo child process.

**Workaround used**: export the four variables from bash before invoking the script —
`CC_riscv64gc_unknown_none_elf`, `AR_riscv64gc_unknown_none_elf`,
`CFLAGS_riscv64gc_unknown_none_elf` (with an absolute `-I`), `OBJCOPY`. With these set,
`pwsh ./gen_disk.ps1` completes and produces `disk_v3.img` plus the kernel.

## Boot recipe that works

```
qemu-system-riscv64 -machine virt -m 256M -nographic -bios default \
  -kernel target/riscv64gc-unknown-none-elf/release/vicell-kernel \
  -drive file=disk_v3.img,format=raw,if=none,id=hd0 \
  -device virtio-blk-device,drive=hd0
```

Reaches `=== ViCell shell ready ===` on a clean `main` worktree.

## Driving the shell from a script — one trap

The prompt `ViCell > ` is written **without a trailing newline**, so a harness that reads
line-by-line blocks forever waiting for it and never sends its command. Trigger on the
`=== ViCell shell ready ===` line instead, then pause briefly before writing: the input
service only delivers to a cell already parked in `Recv`.

## Recommended follow-ups

1. Make `gen_disk.ps1` fail loudly if `CFLAGS_*` does not survive into the child — or drop
   the PowerShell layer for a POSIX script on Linux CI parity.
2. Have the build scripts accept `riscv64-unknown-elf-*` directly rather than relying on
   `Resolve-CrossTool` output reaching cargo, since the `build.rs` fallbacks bypass it.
3. Re-run the runtime gates left open by phases 09 and 11 — the reason they were skipped does
   not hold. **Phase 10 done 2026-07-31**: `tests/integration/tests/wx-text-write.rs` 2/2 PASS
   and the `boot` suite 54/54 PASS, from a detached worktree at the branch head. Tracked as
   A4 in `decision-docket-260730.md` Part 0.
4. Integration tests need an explicit `--target x86_64-unknown-linux-gnu` on Linux: the
   checked-in `.cargo/config.toml` defaults to a Windows host target, so a bare `cargo test`
   fails to find `core` for `x86_64-pc-windows-msvc` before it ever boots QEMU.
