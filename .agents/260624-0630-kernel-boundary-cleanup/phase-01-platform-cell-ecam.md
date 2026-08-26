# Phase 01 — Platform Cell (PCIe ECAM Enumeration)

## Context Links
- Plan: [plan.md](plan.md) · Prereq: [phase-00-prerequisites.md](phase-00-prerequisites.md)
- Source to migrate: `kernel/src/task/drivers/pcie_ecam.rs` (499 LOC)
- Blacklist: `docs/specs/15-kernel-boundary.md` — "PCIe ECAM enumeration → Platform Cell (G2)" (Fuchsia + Genode do this in userspace)
- ECAM bases: x86_64 q35 `0xB000_0000` (from ACPI MCFG), RISC-V virt `0x3000_0000`, ARM64 virt `0x3F00_0000` (`pcie_ecam.rs:22-60`)

## Overview
- **Priority:** P1 — gates the x86_64 VirtIO path (Phase 05/06) since on q35 VirtIO devices live on PCIe.
- **Status:** **complete** (2026-06-24)
- **Risk:** HIGH — on x86_64 the kernel currently relies on `pcie_ecam::init()` at boot; moving it to a Cell changes boot ordering.
- **Description:** Move PCIe ECAM enumeration out of the kernel into a new `cells/services/platform/` Cell. It scans the ECAM config space (MMIO it claims), discovers devices, and registers each device's BARs via `sys_register_pcie_bar` (Phase 00). The kernel's `pcie_ecam.rs` shrinks to a passive BAR table (`find_class`, `register_bar`) — no scanning.

## Key Insights (verified)
- `pcie_ecam.rs` populates `static PCI_DEVICES: Spinlock<Vec<PciDevice>>` (`:150`) with `bars: [Bar;6]`, msix, pm. Consumers call `find_class(class,sub,progif)` via `sys_find_pcie_device` (418).
- After migration: Platform Cell does the scan and calls `register_bar()`; kernel keeps only `PCI_DEVICES` + `find_class` + the new `register_bar`. `init()` is deleted (Phase 08) / no longer called at boot.
- ECAM is plain MMIO — Platform Cell claims it via `sys_request_mmio(ecam_base, window)`. The Cell needs the **per-arch ECAM base**: pass it via a kernel query or hardcode per-arch (the base is fixed per platform; x86_64 needs the ACPI MCFG value — keep MADT/MCFG parse in kernel `acpi.rs` for now and expose the ECAM base via a small query, or have init pass it as a spawn arg).
- Platform Cell lives in **BootFS** (embedded), spawned early — after VirtIO blk (for any disk need it has none, so actually it can spawn before disk) but **before** any Driver Cell that calls `sys_find_pcie_device`. Order: kernel → platform → virtio-blk → vfs → rest.
- `PlatformCap` singleton (Phase 00) gates `sys_register_pcie_bar`.

## Requirements
### Functional
1. New Cell `cells/services/platform/` that: claims ECAM MMIO, walks bus 0 config space, finds all functions, reads BARs, calls `sys_register_pcie_bar(bdf, bar_base, bar_len)` for each relevant BAR.
2. Kernel `pcie_ecam.rs` reduced to passive store + `find_class` + `register_bar`; `init()` scan removed from boot.
3. Boot ordering: Platform Cell spawned (from BootFS) before VirtIO/NVMe/e1000 Cells call `sys_find_pcie_device`.

### Non-Functional
- x86_64 q35 must still discover VirtIO + NVMe + e1000 devices identically to today.
- Cell is `#![forbid(unsafe_code)]` except the ECAM MMIO reads (via `ostd::mmio::MmioRegion`, already safe-wrapped — so possibly *zero* unsafe in the Cell).

## Architecture

### Data flow
```
Kernel boot ──► spawn /bin/platform (BootFS), grant PlatformCap
Platform Cell Init:
  ecam_base = <per-arch const or spawn-arg>
  region = sys_request_mmio(ecam_base, ECAM_WINDOW)        // claim ECAM
  for bus 0, dev 0..32, fn 0..8:
     cfg = region @ (dev<<15)|(fn<<12)
     vendor = cfg.read_u16(0); if 0xFFFF: skip
     for bar in 0..6:
        (base,len) = probe_bar(cfg, bar)                    // size via write-0xFFFFFFFF/read-back
        if base != 0: sys_register_pcie_bar(bdf, base, len)
  sys_register_service(service::PLATFORM, self)            // optional discovery
Driver Cells later: sys_find_pcie_device(class,sub,progif) → reads kernel PCI_DEVICES → bar0_base
```

> NOTE: BAR sizing requires writing all-ones to the BAR then reading back the mask. This is a
> **config-space write**, which the Cell does through its ECAM MMIO region. The kernel must allow
> the Platform Cell to write ECAM (it owns the region via `sys_request_mmio`). Confirm no kernel
> code touches ECAM after handoff.

### Kernel-side reduction (`pcie_ecam.rs`)
- KEEP: `PCI_DEVICES`, `PciDevice`, `Bar`, `find_class()`, NEW `register_bar(bdf,base,len)` (adds a `PciDevice` entry or BAR to existing). The `register_bar` arm added in Phase 00 calls this.
- REMOVE (Phase 08, after Cell proven): the `init()` scanning loop, the per-arch ECAM-base self-discovery, BAR probing in kernel.
- x86_64 ACPI MCFG: `acpi.rs` keeps MADT; the MCFG base needed by the Platform Cell is read by kernel `acpi.rs` and handed to the Cell as a spawn arg (avoids the Cell parsing ACPI — that stays kernel per whitelist "bootstrap root of trust"... actually ACPI parse is blacklisted to Platform Cell. **Decision:** keep MCFG-base extraction minimal in kernel for now, pass base to Cell; full ACPI→Platform Cell is a later G2 item, out of scope here).

## Related Code Files
**Create:**
- `cells/services/platform/Cargo.toml`, `build.rs` (`cell_build::emit_linker_script()`), `src/main.rs`, `src/scan.rs` (ECAM walk + BAR probe), `src/bar.rs` (BAR sizing helper).
- BootFS embed: `kernel/src/main.rs` — `static PLATFORM_ELF = include_bytes!(.../platform)` + spawn + `PlatformCap` grant (loader path-match already added in P00).

**Modify:**
- `kernel/src/task/drivers/pcie_ecam.rs` — add `register_bar()`; gate `init()` behind a fallback (call only if Platform Cell absent during transition).
- `kernel/src/main.rs` — spawn Platform Cell before disk Cells; stop calling `pcie_ecam::init()` once Cell registers (transition: call init() only as fallback if no PlatformCap holder appears within boot window).
- root `Cargo.toml` — add `cells/services/platform` member.
- `cells/tools/init/src/main.rs` — IF platform is spawned from BootFS by kernel, init does NOT spawn it; otherwise add to init order. **Decision: BootFS + kernel-spawn** (needs to run before vfs which init spawns first).
- `gen_disk.ps1` + embedded copy — sign + embed platform like init.
- `libs/api/src/syscall.rs` — add `service::PLATFORM` constant (Law 1 — service-id table; bundle with P00's Law 1 review if possible).

## Implementation Steps
1. **Spike: x86_64 ECAM base handoff.** Decide how the Cell learns `ecam_base` on q35 (spawn-arg from kernel ACPI MCFG read is the chosen path). Confirm `set_spawn_args`/`get_spawn_args` can carry a `usize` to a BootFS-spawned cell.
2. Scaffold `cells/services/platform/` from the nvme Cell template (Cargo.toml, build.rs, run_app! handler, declare_manifest! with no privileged flags — PlatformCap is path-granted).
3. Port the BAR-probe + bus walk from `pcie_ecam.rs` scanning loop into `src/scan.rs` using `ostd::mmio` reads/writes (no kernel `read32`).
4. Implement Init handler: `request_region(ecam_base, window)` → walk → `sys_register_pcie_bar` per BAR. Handle "ECAM claim failed" by exiting cleanly (kernel fallback `init()` runs).
5. Kernel: add `pcie_ecam::register_bar()`; wire the Phase-00 `RegisterPcieBar` syscall arm to it.
6. Kernel `main.rs`: embed + spawn Platform Cell from BootFS *before* the disk/driver cells; grant PlatformCap; pass ecam_base spawn-arg on x86_64.
7. Transition guard: keep `pcie_ecam::init()` callable; in `main.rs` call it ONLY if no Platform Cell registered after spawn (so a missing/crashed Platform Cell still boots). Phase 08 removes this guard + `init()`.
8. `gen_disk.ps1`: build `-p service-platform`, sign, embed.
9. Boot test x86_64 q35: confirm `sys_find_pcie_device` for NVMe/e1000/VirtIO returns the same BARs as before (compare boot logs).
10. Boot test RISC-V virt + ARM64 virt: ECAM base differs; confirm scan still finds VirtIO-PCI if present (RISC-V virt uses VirtIO-MMIO not PCI by default — Platform Cell finds nothing, exits cleanly, MMIO path unaffected).

## Todo List
- [ ] Spike ECAM-base spawn-arg handoff (x86_64)
- [ ] Scaffold platform cell
- [ ] Port BAR probe + bus walk to scan.rs (ostd::mmio)
- [ ] Init handler: claim ECAM → register BARs
- [ ] kernel register_bar() + wire RegisterPcieBar arm
- [ ] kernel main.rs BootFS spawn + PlatformCap + ecam-base arg
- [ ] transition fallback guard for pcie_ecam::init()
- [ ] gen_disk sign + embed
- [ ] x86_64 q35 boot parity test
- [ ] RISC-V + ARM64 boot regression test

## Success Criteria
- [ ] x86_64 q35: Platform Cell registers all BARs; `sys_find_pcie_device(NVMe/e1000/VirtIO)` returns identical bar0_base to pre-migration boot log.
- [ ] NVMe + e1000 Cells (existing) still init via the Cell-fed PCI_DEVICES table.
- [ ] RISC-V/ARM64 virt boot unaffected (Platform Cell finds 0 PCI devices, exits clean, MMIO path serves disk).
- [ ] Spawning 2nd Platform Cell rejected (PlatformCap singleton).
- [ ] `pcie_ecam::init()` no longer the primary scan path on x86_64 (only fallback).

## Risk Assessment
| Risk | L | I | Mitigation |
|------|---|---|-----------|
| Platform Cell boots too late → driver Cell's `find_pcie_device` returns empty | Med | High | Kernel spawns it from BootFS *first*; driver Cells spawn after; add boot-window wait |
| ECAM base wrong on x86_64 (ACPI MCFG variance) | Med | High | Read MCFG in kernel acpi.rs, pass as spawn-arg; log + compare to old value |
| BAR sizing write corrupts a live device | Low | High | Standard probe (save→write 0xFFFFFFFF→read→restore); only bus 0; skip already-claimed |
| Cell crash leaves no PCI table | Med | High | Transition fallback: kernel `init()` runs if no PlatformCap registers in boot window |

## Security Considerations

> **⚠️ Red-team fix (H3):** The Platform Cell must RELINQUISH the ECAM write region after
> enumeration. Holding the ECAM MMIO claim for the Cell's lifetime means a compromised or
> exploited Platform Cell can reprogram ANY device's BAR at any time — including the IOMMU's
> own PCI config registers if IOMMU is PCI-attached, potentially subverting DMA isolation.

- ECAM MMIO is the keys-to-the-kingdom (config-space write can reprogram any device's BAR). Only the singleton PlatformCap Cell may claim it; recorded in resource_registry.
- **REQUIRED:** After completing the ECAM scan and calling `sys_register_pcie_bar` for all devices, Platform Cell MUST release the ECAM MMIO claim via `sys_release_mmio` (or the `Drop` impl on its `MmioRegion`). The `resource_registry` then marks the ECAM range unclaimed. A later Cell cannot re-claim it without PlatformCap (singleton already consumed). This gives one-shot scan semantics: enumerate → register → drop.
- If `sys_release_mmio` doesn't exist, add it (or document that `MmioRegion::drop` already calls it — check `kernel/src/resource_registry.rs`).
- `sys_register_pcie_bar` validated against PlatformCap; no other Cell can inject fake BARs.
- BDF ownership recorded in resource_registry prevents two Driver Cells claiming the same device.

## Next Steps
- Unblocks Phase 05/06 x86_64 PCI VirtIO path.
- Phase 08 deletes `pcie_ecam::init()` scanning + fallback guard.
