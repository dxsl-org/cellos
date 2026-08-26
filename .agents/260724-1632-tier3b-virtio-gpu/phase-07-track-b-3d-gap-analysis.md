# Phase 07 — Track B: 3D / virgl Gap Analysis (decision-only)

## Context Links
- Plan: [plan.md](plan.md)
- Scope Doctrine + 4-gate test: `CLAUDE.md` → "Scope Doctrine — SAS/LBI-first".
- Compositor is CPU-only software blend: `docs/specs/06-graphics.md`.
- G3 NPU/GPU-board gating (project memory): G3-class = after a real accelerator board exists.

## Overview
- **Priority:** P3 — **analysis + verdict only. No implementation planned.**
- **Status:** pending (decision artifact)
- **Description:** Enumerate exactly what Cellos lacks for host-accelerated 3D (virgl/venus),
  classify each missing piece against the Scope Doctrine 4-gate test, and record the verdict:
  host-accelerated 3D is G3-class, gated on real GPU hardware — NOT current work. Near-term
  graphical-app need is fully met by Track A (2D scanout) + guest-side Mesa llvmpipe.

## What Cellos lacks for host-accelerated 3D (research-grounded)
virglrenderer is **not a renderer** — it decodes the guest's Gallium/virgl command stream and
**replays it onto a real host GL/GLES context** (translating TGSI→GLSL at runtime). venus is a
thinner Vulkan pass-through but requires a genuine host Vulkan ICD. Either way the host needs a full
graphics stack. The five missing layers:

| Layer | What it is | Size | Cellos has it? |
|-------|-----------|------|----------------|
| 1. Host GPU kernel driver (DRM/KMS, GEM/TTM, command submit) | manages real GPU hardware | **large** (multi-year) | No — and no GPU hardware |
| 2. Userspace GL/Vulkan driver (Mesa radeonsi/RADV/ANV or vendor) | shader compiler + command builder | **large** (most of "Mesa") | No |
| 3. Windowing/context layer (EGL + GBM + libdrm) | buffer alloc, context/surface, render-node ioctls | medium | No — Cellos has no DRM concept |
| 4. virglrenderer (or rutabaga_gfx) | TGSI→GLSL replay engine, resource/fence model | medium-large | No |
| 5. Display-path GPU-buffer import (dma-buf → scanout) | hand GPU buffers to the compositor | medium | No — compositor is CPU memcpy/blend only |

Notes from research: a "software-only" host (Mesa llvmpipe as the host GL driver, or lavapipe for
venus) does **not** remove the dependency — it still requires the entire Mesa+EGL/GBM+libdrm stack,
just with a CPU rasterizer swapped in for layer 2; and it is strictly slower than guest-side
llvmpipe (double translation). venus additionally needs guest kernel ≥5.16 blob support and host
`/dev/udmabuf`. Porting virglrenderer is inseparable from first porting Mesa OpenGL/EGL — one of the
largest open-source C/C++ codebases; a precise LOC figure could not be obtained (source host blocked
anti-bot) and is stated qualitatively.

## Scope Doctrine verdict (4-gate test)
1. *Leverages/showcases SAS+LBI?* No — virgl is foreign-GPU-command replay; it neither uses nor
   showcases zero-copy IPC / type-isolation / capability model.
2. *A library a Tier-1/1b cell needs to function?* No — it is host infrastructure, and it would drag
   in fork/mmap/full-POSIX-scale dependencies → the explicit "thin-shim creep" anti-pattern.
3. *General ecosystem "Linux already has it"?* Yes — this is exactly ecosystem-chasing, which the
   doctrine confines to Tier 3.
4. *Replicates Linux into native/kernel OR erodes the SAS/LBI bargain?* Yes — it requires standing
   up most of the Linux graphics stack (layers 1-3) that Cellos deliberately does not have.

**Verdict: G3-class, deferred.** Host-accelerated 3D (virgl or venus) requires a real host GPU
kernel driver + Mesa/Vulkan userspace + EGL/GBM context layer + a GPU-buffer-import display path —
none of which exist in Cellos's software-blend compositor, and which cannot be cheaply acquired. It
is gated on (a) a real GPU-bearing board and (b) a multi-year Mesa/DRM port, aligning with the
existing G3 accelerator-board gate. The correct near-term answer is Track A (2D scanout) + guest-side
Mesa **llvmpipe**: pure-CPU OpenGL inside the guest, blitted to a dumb framebuffer, needs **zero**
host 3D support and works on the existing CPU compositor today.

## Requirements
- This phase produces documentation only. Deliverable: fold the verdict into
  `docs/guides/tier3b-linux-vm.md` and `docs/specs/06-graphics.md` as the documented graphics
  boundary (2D-scanout host support; 3D = guest software only, host-accel = G3-gated).

## Related Code Files
- **Modify (docs only):** `docs/guides/tier3b-linux-vm.md`, `docs/specs/06-graphics.md`.
- **Do NOT create** any virgl/GL/Mesa code.

## Todo List
- [ ] Land the 2D/llvmpipe boundary statement in `tier3b-linux-vm.md`
- [ ] Land the G3-gated host-accel-3D note in `06-graphics.md`
- [ ] Cross-link to G3 accelerator-board roadmap item

## Success Criteria
- Docs state, unambiguously: (1) graphical apps work today via 2D + guest llvmpipe; (2) host 3D
  acceleration is out of scope until a real GPU board + Mesa/DRM port exist (G3).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Future contributor attempts a virgl port without a GPU board | L×H | This doc is the explicit gate; reference it in code review. |
| "software llvmpipe on host" mistaken for a cheap 3D win | M×M | Doc states it still needs the full Mesa/EGL stack and is slower than guest llvmpipe. |

## Security Considerations
Not applicable (no code). Note only that a future 3D path would add a large new host attack surface
(shader compiler, GL state replay) — a further reason to gate it.

## Next Steps
None — decision recorded. Track A remains the deliverable.
