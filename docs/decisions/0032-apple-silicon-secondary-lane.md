# ADR-0032 — Apple Silicon (aarch64 group 3) as an active secondary lane

**Date:** 2026-11-04
**Status:** Accepted for direction. The owner decided this on 2026-11-04 (quoted below). The lane rules in §2–§8 are the recorded reading of that decision; the owner may amend them at the plan's phase 00 checkpoint. This ADR amends [ADR-0022](0022-intel-x86-64-c2c-only-direction.md) and [ADR-0029](0029-platform-image-families-and-runtime-detection.md) §5 group 3, and corrects one example in [ADR-0030](0030-license-model-and-copyleft-boundary.md).
**Decider:** Cellos maintainer (owner), planning conversation of 2026-11-04: "Nhóm này không ưu tiên bằng x86 nhưng cũng sẽ là nhóm active để triển khai từ từ vì máy apple silicon bây giờ quá phổ biến để chạy AI agent local" ("This group has lower priority than x86, but it will also be an active group, rolled out gradually, because Apple Silicon machines are now very common for running local AI agents").
**Implementation authority:** direction and scheduling only. Every slice still needs its CELL-CONTRACT-0024 lock handoff, and Law-1 CP1/CP2 where it changes ABI. No hardware purchase is approved.
**Plan:** [`.agents/261104-0711-apple-silicon-group/plan.md`](../../.agents/261104-0711-apple-silicon-group/plan.md). **Research:** [Asahi reference analysis](../../.agents/261104-0711-apple-silicon-group/research/asahi-linux-reference.md).

## Context

- ADR-0029 defined aarch64 group 3 (Apple Silicon) as design-only and outside the roadmap. Under ADR-0022 any work on it first needed an amendment.
- Haily is the planned flagship application, and its users increasingly run local agents on Apple Silicon Macs.
- Asahi Linux shows the hardware path. Coverage on 2026-11-04: M1/M2 mature and largely upstream; M3 officially supported since 2026-09 (not M3 Ultra); M4 has no PCIe, DART or NVMe support.
- The current shared aarch64 core cannot run on Apple CPUs:
  - non-VHE EL2 host, while Apple CPUs force `HCR_EL2.E2H`;
  - FIQ treated as sync, while Apple delivers the timer and IPIs as FIQ;
  - GIC only, while Apple has AIC;
  - PSCI-only secondary start, while Apple uses spin-table;
  - 4 KiB granule with a 39-bit identity map, while DART uses 16 KiB pages and t600x DRAM starts at 1 TiB;
  - `MAX_HARTS = 2`;
  - no Linux arm64 Image entry.
- Licensing:
  - m1n1 is GPL-2.0-or-later from v2.0.0 (MIT only up to v1.9.9).
  - OpenBSD's `apl*` drivers are ISC and cover AIC, DART, PCIe, ANS/NVMe, RTKit, mailbox, SART and PMGR.
  - Linux/Asahi drivers differ per file. AIC, DART, Apple PCIe and Apple NVMe are GPL-only. `drm/asahi`, RTKit, the mailbox and SART carry a `GPL-2.0-only OR MIT` option.

## Decision

1. **Status of the group.** Apple Silicon becomes an **active secondary lane**. Intel x86-64 C2C (ADR-0022, ADR-0031) remains the primary direction and keeps precedence everywhere below.
2. **Lane rules.**
   - At most one Apple implementation slice is in progress at a time. This is the Apple lane's own WIP slot, beside the Intel single-WIP slot.
   - An Apple slice never takes file ownership from an active or admitted Intel slice. On conflict the Apple slice waits.
   - When both lanes need owner review or a checkpoint, Intel goes first.
   - A shared-core change (`hal/arch/arm/**`, arch-generic `kernel/src/**`) must keep every existing aarch64 lane (QEMU `virt`, `raspi3b`, RPi4 compile) and the x86 lanes green.
3. **Admission rule for Apple tasks.** Each slice records:
   - its Apple milestone (AS-0…AS-9 in the plan);
   - its consumer: a Haily/local-agent node, or a shared-core prerequisite;
   - its acceptance scenario and its evidence ceiling.

   It does not need an Intel C2C link. "Might help Apple later" does not pass.
4. **Targets, in order.**
   1. Mac mini M1 (t8103).
   2. M2 (t8112).
   3. M1/M2 Pro/Max/Ultra (t600x/t602x), for memory capacity.
   4. M3 only after a permissive reference exists for the blocks Cellos needs.
   5. M4 and later are parked until PCIe, DART and NVMe are publicly documented.

   ADR-0029 §5's "one image per chip generation" stays.
5. **Two tracks inside the group.**
   - **Hosted:** Cellos as a guest of macOS on Apple Silicon (Virtualization.framework or QEMU-HVF; GICv3, virtio-pci). It reuses the generic virt backend, so it is cheap and keeps macOS on the machine. It is not Apple SoC evidence.
   - **Bare metal:** the Asahi-referenced path (m1n1, AIC, DART, ANS, Apple PCIe).
   - Both depend on the same shared-core prerequisites.
6. **Boot and GPL separation.**
   - Development uses m1n1 proxy mode and the m1n1 hypervisor (USB vUART, MMIO tracing) as external, unmodified tools.
   - Cellos artifacts contain no m1n1, U-Boot or Linux code. The installed boot path (U-Boot + Limine EFI, or an m1n1 payload) is chosen in plan phase 07. If a combined m1n1 + Cellos blob is ever distributed, it needs its own licence review before release.
7. **Provenance.**
   - **Permitted sources:** OpenBSD `apl*` drivers (ISC); m1n1 ≤ v1.9.9 (MIT, tag pinned); Linux/Asahi files whose own SPDX line offers MIT (for example `drm/asahi/*.rs`, `soc/apple/{rtkit,mailbox,sart}.c`), used under MIT with attribution; Linux Apple DT sources (`GPL-2.0+ OR MIT`, used under MIT); DT bindings (BSD-2-Clause option); FreeBSD/OpenBSD NIC drivers.
   - **Forbidden sources:** Linux/Asahi files without a permissive option (for example `irq-apple-aic.c`, `apple-dart.c`, `pcie-apple.c`, `nvme/host/apple.c`), the GPL Linux `kernel::` abstractions, m1n1 ≥ v2.0.0 source, U-Boot source, and disassembly of Apple binaries.
   - **Tainted-session rule:** a human or AI-agent session that has loaded forbidden material for a component does not write that component; it may write a behavioural spec for someone else.
   - Every Apple file records its references in the plan's provenance ledger.
8. **DMA isolation.**
   - Apple profiles are `DmaIsolation::Required`. DART translation must be enabled, and bypass is refused.
   - Whether SART-filtered ANS storage satisfies `Required` is decided in phase 06. Until then storage DMA fails closed.
   - The hosted track follows ADR-0031 D-VM: an explicit, hypervisor-only profile.
9. **Evidence ceilings for this lane.**
   - `S1-qemu` (QEMU virt);
   - `S1-hvf` (guest on an Apple host);
   - `S2-hv` (bare metal under the m1n1 hypervisor);
   - `S2` (native, one exact Mac);
   - `S3` (qualified).

   None is promoted to a higher level. HCL §7 rows need S2 or S3.
10. **Out of scope.** Apple GPU (AGX/DCP), the Neural Engine, AMX, Thunderbolt/USB4, the Type-C PHY (ATC), Wi-Fi/Bluetooth, audio, SEP/Touch ID, and laptop peripherals. Each needs a new decision.

## Considered options

| Option | Outcome | Reason |
|---|---|---|
| Keep group 3 design-only (ADR-0029 as written) | Rejected | Owner decision 2026-11-04 |
| Make Apple equal to Intel | Rejected | Owner: lower priority than x86 |
| Apple only after Intel C2C completes | Rejected | Owner wants the lane active now. A separate WIP slot with Intel precedence keeps Intel unblocked |
| Bare metal only, no hosted track | Rejected | Most Mac users will not replace macOS. The hosted track reaches them early and reuses shared-core work |
| Start with M4 (most popular for local AI) | Rejected for now | Asahi has no PCIe/DART/NVMe on M4, and the only progress is an LLM-reverse-engineered fork whose provenance Cellos cannot adopt |
| Port Asahi drivers / relicense | Rejected | ADR-0030 |

## Consequences

- ADR-0022's "single WIP" becomes one WIP slot per lane (Intel primary, Apple secondary). Intel precedence is absolute.
- The aarch64 core gains real work that also serves ADR-0029 group 2 and QEMU: VHE host, FIQ, GICv3, 16 KiB/wide-PA paging, spin-table, N harts, and the Linux Image entry. The RPi group-1 backends must not regress.
- x86 breadth phase 07a (N-CPU per-CPU structures) and phase 17 (`bcm57xx`) gain an Apple consumer. The Intel slice keeps ownership; Apple adds device IDs or backends after it.
- The HCL gains an Apple section with its own mandatory requirements and an empty machine table.
- Phase 08 must report CPU-only inference against Asahi Linux and macOS on the same Mac, including an unfavourable result.

## Related

- [ADR-0022](0022-intel-x86-64-c2c-only-direction.md), [ADR-0029](0029-platform-image-families-and-runtime-detection.md), [ADR-0030](0030-license-model-and-copyleft-boundary.md), [ADR-0031](0031-x86-platform-breadth-kvm-vmware-intel.md)
- [HCL §7](../hardware-compatibility-list.md#7-apple-silicon-aarch64-group-3), [Haily roadmap](../roadmap/haily-personal-assistant.md)
