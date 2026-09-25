# Porting C applications to Cellos

Cellos C ports are statically linked native cells. Select the lane before editing source:

| Class | Lane | Use when | Stop condition |
|---|---|---|---|
| A | Relink | Single-process C app with an explicit platform layer | Uses only the POSIX-shim contract and hooks below |
| B | Adapt | App needs a small host/UI/input rewrite | Replace Linux/SDL/platform code with the Cellos hooks |
| C | mlibc | Source needs broader libc/POSIX than the shim | Build mlibc; do not link the shim too |
| D | Tier 3 guest | Needs `fork`, dynamic loading, JIT, or Linux process semantics | Run the application in a Linux guest |

## Choose the runtime

- **POSIX shim:** `api = { features = ["posix"] }`. Its exact exported ABI is generated at
  [`posix-shim-contract.generated.md`](posix-shim-contract.generated.md). Any symbol absent from
  that table is unsupported; do not discover this at final link time.
- **mlibc:** follow [the mlibc build guide](../mlibc-build.md). It replaces—not supplements—the
  shim. Enabling both profiles is an error due to duplicate C symbols.

Named SAS refusals are deliberate: process creation (`fork`/`execve`), dynamic loading (`dlopen`),
page-permission mutation (`mprotect`), and file-backed shared mappings cannot be granted by an
in-process native cell. Use a Tier 3 guest when they are requirements.

Starting a *fixed, reviewed* child is the one process-shaped operation a port may use, and it is not
any of the above. `cellos_spawn.h` launches one exact reviewed target with a bounded command line
and explicit endpoint grants; the kernel authorizes an exact `(caller identity, route, target)` row
and derives the child's capability ceiling from it, so a port cannot turn a string into authority.
A child inherits nothing: it holds no capability and has no launch edge of its own.

## Platform hook contract

Include `cellos_platform.h`. A Rust host owns the Cellos services and invokes the C application's
entry point only after creating its compositor surface. C owns application logic; it receives no
kernel/service handles.

| Header API | Host service | Contract |
|---|---|---|
| `cellos_surface_create`, `cellos_surface_present` | compositor | BGRA8888 surface; pixels live until host destruction |
| `cellos_input_poll` | input | one decoded input event or `CELLOS_INPUT_NONE` |
| `cellos_time_ms`, `cellos_sleep_ms` | clock + scheduler | monotonic milliseconds; sleep yields rather than busy-spins |
| `cellos_vfs_read`, `cellos_vfs_write` | VFS | only paths granted by the cell's manifest/capabilities |
| `cellos_tcp_connect` | Net service | TCP capability only; no ambient networking |

The declarations in this table are exactly those in
[`libs/port-platform/include/cellos_platform.h`](../../libs/port-platform/include/cellos_platform.h).
`cells/demos/tetris-c` is the in-tree precedent: its Rust host supplies surface, input, and time
callbacks while its C game remains platform-oriented.

## Reference-port evidence

| Phase-07 class | Candidate | Result | Measured cost / blocker |
|---|---|---|---|
| A | MIT Tetris-C | RV64 QEMU `TETRIS-C-PORT-QEMU: PASS`; Tier 2 admission and ready marker witnessed | 48,384 B RV64 release ELF. The port now obtains its C timer callback from `PlatformHost`; its build no longer requires unavailable cross `libc.a`/`libm.a`. Historical engineering hours were not recorded. |
| B | pthread C workload | RV64 QEMU `C-PTHREAD-QEMU: PASS`; Tier 2 admission, mutex/condition hand-off, one-shot join, and 32 immediate create/join reuse cycles witnessed | Use `cellos_pthread.h` only for its narrow documented subset. C `__thread`, cancellation, detachment, and a full POSIX thread runtime remain unsupported. |
| C | C child-process utility | Class B witness (`c-spawn`) | `cellos_spawn.h` + `cellos_spawn.c` compose an exact reviewed launch edge with the staged command line and explicit `PipeShare` endpoint grants; RV64 QEMU `C-SPAWN-QEMU: PASS`. There is no `fork`, `exec`, `posix_spawn`, or ambient path authority, and a child holds no capability of its own. |

This is a development/QEMU witness only. It neither measures physical performance nor qualifies
hardware, fleet posture, or production release.

## Before building

1. Add a reviewed launch target and a least-privilege manifest/syscall declaration. A port that
   starts a child also needs its exact `(caller, route, target)` row in
   [`launch_profile`](../../kernel/src/loader/launch_profile), plus the target in the shell's
   reviewed list if the shell must launch it.
2. Compile C with PIC and freestanding flags; do not silently link host libc.
3. Compare undefined C symbols against the generated shim contract.
4. Exercise the cell in QEMU. A successful cross compile alone does not prove VFS, input, or
   compositor capability wiring.
