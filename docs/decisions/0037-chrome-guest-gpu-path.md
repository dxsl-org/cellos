# ADR-0037 — The Chrome guest renders in software

**Date:** 2026-11-26
**Status:** Accepted
**Decider:** Cellos maintainer (owner), plan `.agents/261125-1100-chrome-kiosk-guest-resources` phase 12.
**Related:** [ADR-0017](0017-dual-browser-strategy-ocel-and-tier3-chrome.md) §5d/§5e, [ADR-0033](0033-x86-intel-igpu-native-compute.md).

## Context

The maintainer asked for more CPU, memory, GPU, files, sound and clipboard for the Chrome
guest. CellOS has no graphics GPU stack: the compositor draws into a linear framebuffer, the
hypervisor's virtio-gpu is 2D only, and ADR-0033 admits compute on one Intel iGPU with nothing
built. A guest cannot be given a GPU the host does not drive. Chromium runs with
`--disable-gpu`; cage composites with pixman.

Phases 03-07 of the plan made the software path faster. Bench (`scripts/tier3_chrome_bench.py`,
1024x680 window, frames in 5 s, one vCPU unless noted, nested KVM on an i7-14700):

| metric | phase 02 CellOS | after phase 07 | plain QEMU/KVM reference (phase 02) |
|---|---|---|---|
| `paint` frames | 80 | 311 | 371 |
| `scroll` frames | 83.5 | 297 | 369 |
| `raf` frames | 267 | 302 | 377 |
| `js1` ms | 1272 | 382 | 504 |
| `jsN` ms (4 workers) | 2385 | 950 (517 with 4 vCPUs) | 1387 |
| timer lateness p95 ms | 21.9 | 1.6 | 0.2 |

Sources: `evidence/phase02-baseline.md`, `evidence/phase05-vcpus.md`, `evidence/phase07-avx.md`.
The reference column was measured on a loaded host in phase 02 and the CellOS columns after it
on a quiet one, so they show the order of magnitude, not a ratio; the plan's assumption that
`paint` and `scroll` would be within a factor of two of the reference holds (0.84x and 0.80x).

## Options

- **A. Software only.** Keep `--disable-gpu`; speed comes from the hypervisor (in-kernel
  APIC timer, entry fast path, more memory, optional vCPUs, AVX2). Works on every machine
  CellOS runs on, physical included. Cost: nothing more.
- **B. virgl proxy.** When CellOS itself runs under QEMU with `virtio-vga-gl`, forward the
  guest's 3D command stream to the outer device and read each frame back into the launcher's
  Grant. Needs a 3D-capable CellOS virtio-gpu driver (`cells/drivers/virtio-gpu` is 2D: no
  context, no 3D resource or submit commands), a guest command-stream validation layer in the
  hypervisor, and a CPU readback per frame. Helps only VM-hosted CellOS; the gain over A after
  readback at 60 Hz for a 1024x680 window is unknown and would need a one-week spike to learn.
- **C. Native graphics.** Extend the ADR-0033 Intel iGPU work from compute to display and
  rendering, then give the guest a paravirtual GPU on top. The only route that helps a
  physical machine; the largest by far, and it starts from a compute-only driver
  (`cells/drivers/intel-gpu`, `cells/runtimes/mesa-anv`).

## Decision

**A.** The maintainer chose software rendering (2026-11-25, "gpu = software"), and the
numbers above show the software path now within about 20 % of a plain KVM guest for paint and
scroll. B is rejected because it optimises the configuration CellOS runs in today (nested
under QEMU), not the one it is aimed at (physical Intel), and puts a guest-driven 3D command
stream in front of a host driver with no validation layer. C stays the long-term route and is
not started by this decision.

## Consequences

- No document may claim GPU acceleration for the Chrome guest until an option that provides
  it is built and measured.
- Remaining software headroom: one vCPU by default (four help parallel JavaScript but cost
  scroll frames on this host, `evidence/phase05-vcpus.md`), the 30-40 µs per `RunVcpu` added by
  AVX under nesting (`evidence/phase07-avx.md`), and frame copies host side.
- Revisit when ADR-0033's iGPU driver reaches display, or if B's spike is asked for; either
  needs its own ADR, including the validation layer a guest 3D stream would require.
