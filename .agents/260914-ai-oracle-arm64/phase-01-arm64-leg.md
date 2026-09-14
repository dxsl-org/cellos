# Phase 01 — ARM64 leg of the CP-3 gate

**Status**: completed
**Ceiling**: qemu (aarch64), same class as the existing riscv64 leg

## What changed

`scripts/run-ai-inference-oracle-qemu.sh` now takes `--arch riscv64|aarch64` (`CELLOS_AI_ARCH` also
works; riscv64 stays the default so existing callers, the CI job's command line, and every recorded
log keep their meaning). Per architecture it selects the Rust target, the QEMU binary, the QEMU
machine/-cpu arguments, and the cross `objcopy` the signing helper needs:

| | riscv64 | aarch64 |
|---|---|---|
| target | `riscv64gc-unknown-none-elf` | `aarch64-unknown-none-softfloat` |
| QEMU | `qemu-system-riscv64 -machine virt -bios default` | `qemu-system-aarch64 -machine virt -cpu cortex-a57` |
| `objcopy` for cell signing | riscv candidates (auto-probed) | `aarch64-linux-gnu-objcopy` |
| rustc flags | `relocation-model=pic` from the script | from `.cargo/config.toml` (`pic +bti,+paca,+pacg`) |

Two details that were not obvious and are now encoded: the signing helper resolves a *rv64* `objcopy`
unless `OBJCOPY` is pre-set (a host `objcopy` refuses a foreign ELF), and an aarch64 run must not set
a bare `RUSTFLAGS`, because that replaces — rather than merges with — the target features the config
file supplies.

The evidence artifact became self-describing: it is the QEMU serial log with a header naming the
architecture, target, model, and boot timeout, and its filename carries the architecture. A log that
cannot say which ISA produced it is not evidence.

The CI job `ai-inference-oracle` is now a two-leg matrix (`riscv64`, `aarch64`) with per-arch caches
and per-arch artifact names, so both halves of CP-3's QEMU requirement are continuously gated instead
of one being a manual recollection.

## What the leg proves

- **Numerics on a second ISA and a second float ABI.** The aarch64 cell target is softfloat: its
  f32/f64 arithmetic is the compiler's software routines. The fixture's eight golden token ids and
  its 64-component embedding reproduce there exactly (`greedy ids matched`, `embedding matched`), so
  the integer Q8_0 path agrees with an independent reference across two ISAs — a silent miscompile, an
  endianness slip, or an aliasing assumption in the kernels would not survive that.
- **The whole service stack on a second architecture**: VIFS1 image, dev-key cell signing and
  loader verification, VirtIO-BLK, W^X cell pages, typed IPC, sessions, streaming, cancellation.
- **A real checkpoint, not just the fixture**: `stories15M-q8_0` (26.7 MB, 32000 vocab) generates
  coherent prose on both ISAs.

| Same fixture and flow | riscv64 TCG | aarch64 TCG |
|---|---|---|
| fixture, 8 tokens | 109 ms | 122 ms |
| `stories15M`, 24 tokens | 1595 / 1601 ms | 3355 ms |

The ~2× difference is an emulator observation (TCG translation, softfloat, cortex-a57 model); it is
not a hardware claim and no board was run.

## Not closed by this slice

CP-3's gate also names "RPi3 memory budget validation". That is the physical-board leg: the model
load path already reports `resident bytes` (28 087 038 for a 26.7 MB checkpoint), but the budget must
be observed on the board. This slice does not substitute a QEMU result for it, and the roadmap row
keeps it open.
