# Phase 04 — VMM one-core + feature-flag preset design note

- **Track:** B (G5 Lite foundations) · **Label:** **now-able** (design/spec, 0 LOC, no HW) · **Tier:** thinking · **Effort:** M

## Context Links
- G5 memory `project-g5-dual-profile-vm`; roadmap §Stage G5. Precedent: rust-vmm core → Firecracker (lite) + Cloud-Hypervisor (wide).
- Shared core inventory from scout: `kernel/src/hypervisor/registry.rs`, `cells/services/hypervisor/src/` (15 files).

## Overview
- **Priority:** P2 · **Status:** pending
- Design note (NOT code): refactor plan for Lite/Wide as **composable feature flags** over one VMM core, NOT two codebases. Identify precisely which shipped files become the shared core vs profile-specific presets.

## Key Insights
- **Profile = host/VMM config; guest image = orthogonal axis.** Alpine (lite guest) and glibc (wide guest, P02) load INTO a VM; Lite/Wide select the hypervisor's device model + boot path + snapshot flags. Re-architecting to presets does NOT require a new distro.
- Model as flags: `{device-model: min|full} × {boot: direct|firmware} × {snapshot/CoW: on|off} × {confidential: none|TDX/SEV/CCA}`. Lite/Wide = 2 curated presets NOW; add a 3rd (e.g. confidential) only when a workload needs an uncovered combo (YAGNI). `VmHandle`/ABI stays CC-neutral.
- **Shared core (already arch-generic):** syscalls 220-227, `registry.rs` VM/vCPU lifecycle, `virtqueue.rs`/`virtio_mmio.rs` framing, `run_loop.rs` dispatch skeleton. **Profile-specific:** which device backends are instantiated (`run_loop.rs:33-40` builds ALL of them today), boot path (initramfs vs root-on-blk vs PVH-firmware), snapshot hooks.
- **CoW/snapshot is an ARCH-SPECIFIC mechanism, NOT shared core (red-team C1/C4).** The `snapshot/CoW: on` flag toggles arch-specific implementations: ARM64 uses stage-2 permission-fault + `tlbi ipas2e1` + VMID (P05/P06); x86 uses EPT/NPT write-violation + `INVEPT`/`INVVPID` + VPID (P05b/P06b). The flag-matrix must record that enabling CoW pulls in the arch backend, and that the two backends share only the *provenance/refcount model* (defined in P05), not the fault/TLB mechanics. Do NOT model CoW as a single shared-core feature.

## Requirements
- **Functional (of the design):** a table mapping each of the 15 cell files + registry to {shared-core | lite-preset | wide-preset | future}; a flag-matrix spec; the exact seam where the run loop selects a device-model set.
- **Non-functional:** DRY (no forked VMM); presets are `Cargo` features + a runtime config struct, not `#[cfg]`-duplicated modules.

## Architecture (proposed, for approval)
Introduce a `ViVmProfile` config struct (device-model set, boot path, snapshot on/off) passed to the cell run loop; `run_loop::run` selects which backends to construct from the profile instead of unconditionally building all. Kernel core is profile-agnostic — profile lives entirely cell-side (Kernel Boundary: no profile logic enters kernel).

## Related Code Files (design targets, no edits this phase)
- Would touch: `cells/services/hypervisor/src/run_loop.rs` (profile-driven backend construction), a new `profile.rs` (config struct). Kernel `registry.rs` unchanged.

## Implementation Steps (design deliverables)
1. Produce the file-ownership table (shared-core vs preset).
2. Specify the `{device-model × boot × snapshot × confidential}` flag matrix + the 2 curated presets.
3. Specify the run-loop seam that consumes a profile.
4. Note the guest-pairing: Lite preset ↔ minimal Alpine; Wide preset ↔ glibc (P02). A Lite VMM booting a full distro is still slow (guest userspace bottleneck).
5. State YAGNI gate for the confidential preset (HW + paying customer).

## Todo
- [ ] file-ownership table (core vs preset)
- [ ] flag-matrix + 2-preset spec
- [ ] run-loop profile seam design
- [ ] guest-pairing + speed-reality note
- [ ] confidential-preset YAGNI gate

## Success Criteria
- A design note complete enough that a future coding phase can implement the profile struct + run-loop seam without re-deriving which files are shared. No code lands.

## Risk Assessment
- **Med:** over-abstraction — building a flag matrix before a 2nd real preset need. Mitigation: ship 2 presets only; matrix is documentation, not a plugin framework.
- **Low:** Kernel Boundary drift — pressure to push profile logic into kernel for speed. Mitigation: explicit rule — profile is cell-only.

## Security Considerations
- Confidential preset is a distinct security posture (TDX/SEV/CCA), not "more compat" — do not conflate. Keep ABI CC-neutral so it can be added later without a break.

## Next Steps
- P05 (CoW-golden) is the `snapshot/CoW: on` flag's mechanism; this phase defines where that flag lives.
