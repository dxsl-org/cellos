# Plan — AI inference oracle on the ARM64 leg

**Created**: 2026-09-14
**Ceiling**: qemu (aarch64), same class as the existing riscv64 leg; no hardware claim
**Track**: G2 Level-A AI inference (Spec 24), follow-on to `.agents/260914-q8-integer-activations/`

## Goal

Spec 24's CP-3 governance gate reads "QEMU RV64/ARM64 and RPi3 memory budget validation". The lane
had only ever run its oracle on **riscv64** — `scripts/run-ai-inference-oracle-qemu.sh` hardcoded the
target, the QEMU binary, and the RISC-V `objcopy`, and the CI job ran that one command. So the ARM64
half of the gate was named but unexercised, and the engine's portability across ISAs and float ABIs
was assumed rather than measured.

This slice makes the oracle run on both architectures and closes the QEMU-ARM64 half:

- `--arch riscv64|aarch64` (or `CELLOS_AI_ARCH`), default riscv64 so existing callers and the CI job
  keep the same behaviour.
- The oracle evidence artifact records its own architecture, target, and model. A serial log that
  cannot say which ISA produced it is not evidence of anything, and both architectures now write into
  the same directory.
- The CI job becomes a two-leg matrix, so both gates are continuous instead of one being a manual
  recollection.

## Why this is worth more than a second throughput slice

The aarch64 cell target is `aarch64-unknown-none-softfloat`: its f32/f64 arithmetic is the compiler's
**software** float routines, not FP instructions. A fixture whose golden token ids and embedding
reproduce there is therefore a genuine numerics cross-check of the integer kernel path on a second
ISA *and* a second float ABI — two implementations of the same contract agreeing where a silent
miscompile, an endianness slip, or an `unsafe`/aliasing assumption would show up. It also exercises
the whole service stack (typed IPC, VIFS1 image, cell signing, virtio-blk, W^X cell pages) on a
second architecture, which is what a G2 server cohort would eventually run on.

## Phases

- [phase-01-arm64-leg.md](phase-01-arm64-leg.md) — the parameterization, the two-arch run, and the
  remaining gap (RPi3 memory budget, which needs the physical board).

## Non-goals

- No new kernel or driver work. If the aarch64 image had needed a kernel change this slice would have
  stopped and said so; the probe showed the existing kernel, drivers, and signing path carry the AI
  service unchanged.
- No physical board claim. The RPi3 leg of CP-3 stays open and named: it needs the board, and this
  plan does not substitute a QEMU result for it.
- No change to the frozen AI interface (Law 1): the wire contract, opcodes, and SDK surface are
  untouched, and `scripts/check-ai-law1-digests.sh` stays green.
