# Phase 10 — Run scripts (SVM-TCG CI smoke + KVM note) + CI job + x86 trait finalize + docs

> ⚠️ **Law 1 (light): finalizes the multi-arch `ViHypervisor` trait shape** when the x86 impl replaces
> the ENOSYS stub (`hal/arch/x86/src/hypervisor.rs:9-21`). No new syscalls, no manifest-flag bump, no
> `ViVmExit` change beyond P04. Confirm the trait shape once more before locking.

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-09](phase-09-vtx-backend-apic.md), [phase-05](phase-05-cell-pvh-boot-alpine.md)
- Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-10-run-ci-stubs-docs.md`
- Verified: `run-hypervisor-arm.ps1:72-82` (ARM QEMU launch to parallel), `run-x86.ps1` (existing x86
  runner), `docs/specs/05-application.md:313` (`x86_64 VT-x ... ENOSYS stub ⏳ G2`), `:315` (registry
  cfg note), `:341` (x86 pending section); `hal/arch/x86/src/hypervisor.rs:9-21` (stub replaced in P03).

## Overview
- **Priority:** P2 · **Status:** pending · **Depends on:** 05 (boot smoke) + 07,08 (full matrix)
- Ship the operational + docs layer: a dedicated x86 hypervisor run script (**SVM under TCG** for CI,
  **KVM/VMX** note for real hardware), a CI smoke job that boots Alpine x86_64 to a shell and asserts
  blk+net, replacement of the x86 `ViHypervisor` ENOSYS stub with the real impl, and doc updates to
  `docs/specs/05-application.md` + roadmap/changelog.

## Key Insights
- **CI-accelerator = SVM under TCG (plan Validation Log):** the run script uses
  `qemu-system-x86_64 -cpu qemu64,+svm -accel tcg -m 1G` — the ONLY path that runs hardware-virt guests
  on the Windows+QEMU dev host and the x86 CI runners (TCG has zero VMX; WHPX no nested virt). This
  preserves the ARM track's "TCG makes CI cheap" property. A `-accel kvm -cpu host` block documents the
  real-hardware VMX/SVM path (P09 lane).
- **CI smoke (mirror ARM P10):** boot, grep for busybox `/ #` within a **180s** timeout (TCG boot is
  slow), assert blk mount + apk (P07/P08), then trigger guest shutdown for a clean exit. Reuse ViCell's
  QEMU-boot CI pattern — **TCP probe / serial capture, NOT `-serial file:`** (known footgun in memory).
- **Trait finalize (Law 1 light):** `hal/arch/x86/src/hypervisor.rs` moves from all-NotSupported to the
  SVM-backed impl (VMX arm per P09). The `ViHypervisor` trait shape (`hal/traits/hypervisor/src/lib.rs`)
  is now committed across aarch64 (real) + x86_64 (real) + riscv64 (ENOSYS) — confirm once.
- **Docs:** `docs/specs/05-application.md:313/341` currently says x86 VT-x is an ENOSYS stub / pending
  G2. Update to describe the shipped SVM-first + VMX-second architecture, the PVH boot path, the
  no-LAPIC MVP, and the SVM/TCG-vs-KVM/VMX CI split. Update roadmap/changelog (delegate to
  docs-manager per docs rules).
- **vmlinux artifact:** the CI fetch script must produce the PVH `vmlinux` (P05) — extract from Alpine
  bzImage + verify `readelf -n` note; pin + checksum.

## Requirements
**Functional**
- `run-hypervisor-x86.ps1`: `-cpu qemu64,+svm -accel tcg` + guest artifacts + KVM/VMX comment block.
- CI job: build x86_64 kernel + hypervisor cell, boot Alpine, assert `/ #` within 180s, assert blk mount
  + apk (gated on P07/P08), clean shutdown.
- `hal/arch/x86/src/hypervisor.rs` = real SVM-backed impl (VMX per P09); riscv64 stays ENOSYS.
- `scripts/fetch-alpine-x86.*`: download/cache + extract PVH `vmlinux` + initramfs; verify note.
- Docs: `05-application.md` + roadmap + changelog updated.

**Non-functional**
- Don't regress the ARM64 hypervisor CI or existing x86 suites (`tests/integration/tests/*-x86.rs`).
- Law 1 (light): trait shape finalized across three arches — confirm once.

## Architecture
```
CI smoke (TCG, x86 runner):
  build kernel(x86_64) + hypervisor cell → qemu -cpu qemu64,+svm -accel tcg -m 1G
    + PVH vmlinux + initramfs + rootfs image
  serial/TCP probe: wait "/ #" (180s) → PASS
    [matrix] guest: mount /dev/vda; apk add → assert OK
  guest shutdown → qemu exits 0
real-HW lane (P09): -accel kvm -cpu host  → VMX or SVM native
```

## Related Code Files
**Create**
- `run-hypervisor-x86.ps1` — SVM/TCG QEMU launch + KVM/VMX note + guest artifacts
- `scripts/fetch-alpine-x86.*` — fetch/cache/extract PVH vmlinux + initramfs + verify note
- `.github/workflows/` job (or extend x86 workflow) — hypervisor smoke + matrix
**Modify**
- `hal/arch/x86/src/hypervisor.rs:9-21` — replace ENOSYS stub with real SVM-backed impl (⚠️ trait shape)
- `docs/specs/05-application.md:313,341` — shipped x86 architecture (SVM-first/VMX-second, PVH, no-LAPIC MVP, CI split)
- `docs/project-roadmap.md`, `docs/project-changelog.md` — Tier 3b x86 milestone status

## Implementation Steps
1. Write `run-hypervisor-x86.ps1` (`-cpu qemu64,+svm -accel tcg`) + KVM/VMX comment block + artifacts.
2. `fetch-alpine-x86`: pin version; extract PVH `vmlinux`; `readelf -n` assert; cache initramfs + rootfs.
3. CI job: build + boot + TCP/serial probe for `/ #` (180s); on P07/P08 add blk mount + apk asserts;
   clean-shutdown exit-0 check.
4. Replace the x86 ENOSYS `ViHypervisor` stub with the SVM-backed impl; confirm all 3 targets compile.
   ⚠️ confirm trait shape (Law 1 light).
5. Update `05-application.md` + roadmap + changelog (delegate to docs-manager).
6. Run full CI matrix; confirm no ARM64/x86 regression.

## Todo List
- [ ] `run-hypervisor-x86.ps1` (SVM/TCG + KVM/VMX note + artifacts)
- [ ] Alpine x86 fetch/extract script (PVH vmlinux + note verify + rootfs cache)
- [ ] CI smoke: boot → `/ #` within 180s → clean shutdown exit-0
- [ ] CI matrix: blk mount + apk asserts (gated on P07/P08)
- [ ] ⚠️ x86 ViHypervisor real impl replaces ENOSYS stub (confirm trait shape)
- [ ] docs/specs/05-application.md + roadmap + changelog updated

## Success Criteria
- CI green: a fresh checkout builds the x86_64 kernel + hypervisor cell and boots Alpine x86_64 to `/ #`
  within 180s on a TCG (`-cpu qemu64,+svm`) runner, then exits 0.
- Full matrix (with P07/P08): guest mounts `/dev/vda` and completes `apk add <pkg>`.
- `cargo build` succeeds for aarch64 + riscv64 + x86_64 (x86 real impl, riscv ENOSYS — no missing-trait
  errors).
- `docs/specs/05-application.md` describes the shipped x86 SVM-first/VMX-second architecture (no stale
  "ENOSYS stub" text).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| TCG SVM boot exceeds CI timeout | Med×Med | 180s timeout; cache artifacts; minimal initramfs; KVM lane for dev |
| `-serial file:` capture footgun (memory) | Med×Med | TCP probe / serial stdio capture, not `-serial file:` |
| `+svm` not exposed under the runner's QEMU build | Low×High | Pin QEMU version known to emulate SVM under TCG; assert in job preflight |
| ENOSYS stub signature drift vs real impl | Low×Med | Single trait def in HAL; confirm shape (Law 1 light) |
| Flaky mirror (apk fetch) | Med×Med | Pin/cache local mirror; mark net-matrix non-blocking |

## Security Considerations
- CI runs untrusted guest images — guest stays fully EPT/NPT-isolated (P02 invariant holds in CI).
- Pin + checksum Alpine `vmlinux` + initramfs + rootfs to avoid supply-chain drift.

## Next Steps
- Milestone complete: x86_64 Tier-3b VMM boots Alpine with console + blk + net on the SVM/TCG CI lane;
  VMX validated on the KVM/HW lane (P09). Future: APICv/virtual-APIC throughput, multi-VM, ACPI/PCI for
  broader guest support.
