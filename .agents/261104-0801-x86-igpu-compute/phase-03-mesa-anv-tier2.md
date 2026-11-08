---
phase: 3
title: "Vulkan userspace: Mesa anv as a Tier-2 Cell"
status: pending
priority: P1
effort: "40d"
dependencies: ["G2"]
tier: thinking
checkpoint: true
---

# Phase G3: Mesa `anv` as a Tier-2 Cell

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- Mesa `src/intel/vulkan/**`, `src/intel/compiler/**` (MIT); Tier-2 runtime and POSIX shim (`docs/guides/posix-shim-contract.generated.md`, `docs/posix-ledger.md`); `meson/` cross files

## Overview

Owner decisions 2026-11-04 (Q1, Q3 per the [port-gap](research/mesa-anv-port-gaps.md) recommendations):
- The deliverable is **a Tier-2 inference Cell that links `anv`**: the inference engine plus Mesa's Intel Vulkan driver, in one Cell. It talks to the G2 driver through a transport shim that carries the unchanged `xe` requests and structures over Cellos IPC instead of `ioctl`. Only Vulkan compute is exposed.
- Mesa may carry the small OS-identity patch (`DETECT_OS_CELLOS`, the two meson system lists, a static ICD target), following the upstream Managarm precedent. No other Mesa change is pre-approved.
- The runtime prerequisites (Tier-2 large memory, mlibc `posix` on x86_64, C++17 runtime, hard-float C, XSAVE) are owned by separate plans, not by this phase.

## Key Insights

- Corrected 2026-11-04 ([port gaps](research/mesa-anv-port-gaps.md)):
  - No LLVM on the **target**: `-Dmesa-clc=system`. The **build host** does need LLVM/clang/SPIRV-LLVM-Translator for `mesa_clc` and `vtn_bindgen2`.
  - `anv` is a library inside the application, so the Tier-2 Cell is "inference engine + `anv`". There is no separate `anv` Cell.
  - `anv` is linked directly through `vk_icdGetInstanceProcAddr`, with no loader.
- The shim covers exactly the requests in [xe-uapi-subset.md](xe-uapi-subset.md), plus `mmap`/`munmap` on the pseudo-fd and the libdrm device enumeration. Mesa's `intel_ioctl` virtio reroute is the single seam.
- Tier-2 is untrusted. A shader bug or malicious shader is contained by per-client GPU VMs plus G2's hang reset, not by trusting Mesa.
- **Prerequisites outside this plan** (each needs its own owner decision; none is unlocked by ADR-0033):
  - x86 Tier-2 admission, and large Tier-2 memory (CP1 draft D-G0; today a 4 MiB arena);
  - mlibc with `posix` plus x86_64 support;
  - a C++17 runtime (C++ with exceptions for llama.cpp);
  - hard-float SSE2 C on a soft-float Cell target;
  - XSAVE/AVX state.
- [inference] Estimated user-space effort is 34–58 days, which can exceed this phase's 40 days.

## Requirements

- Meson cross build for `x86_64-unknown-cellos`, minimal options (Vulkan, Intel only, no X11/Wayland/display, `-Dmesa-clc=system`, `zlib` off). Host-side build tools are `mesa_clc`, `vtn_bindgen2` and `glslangValidator`.
- DRM shim with tests against the G1 fake backend.
- A Vulkan compute smoke: vector add and a GEMM checked against CPU results.
- CP2 for the ABI items as implemented.

## Related Code Files

- Create: `cells/runtimes/mesa-anv/**` (build + shim), `meson/cellos-x86_64.ini`, `third_party/mesa` (pinned, MIT)
- Modify: POSIX shim only for gaps found in step 1 (each gap recorded in the POSIX ledger)

## Implementation Steps

1. Inventory the ioctls and POSIX calls `anv` needs for compute.
2. Cross build; stub nothing silently — missing calls fail loudly.
3. DRM shim on the fake backend (host), then on G2 (S2).
4. Compute smokes.

## Todo List

- [x] ioctl/POSIX inventory: documented in research/mesa-anv-port-gaps.md and xe-uapi-subset.md
- [ ] Cross build (blocked: x86 Tier-2 large memory and toolchain prerequisites)
- [x] Shim: implemented in cells/runtimes/mesa-anv with host tests against XeSession and Syncobjs
- [ ] Smokes
- [ ] CP2

## Success Criteria

- [ ] S2: vector add and GEMM (e.g. 1024×1024 F16/F32) match CPU results within tolerance; repeated runs leave no leaked GPU memory.

## Risk Assessment

- The POSIX gap may be larger than expected (threads, mmap semantics, file descriptors). Mitigation: step 1 measures it before any port work.

## Security Considerations

Mesa runs as an untrusted Tier-2 Cell; its authority is only its own GPU client capability.

## Next Steps

G4.

## Deviation Log

None.
