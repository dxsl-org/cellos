# ADR-0029 — Platform image families and boot-time hardware detection

**Date:** 2026-10-31
**Status:** Accepted. x86-64 runtime selection is in scope under [ADR-0022](0022-intel-x86-64-c2c-only-direction.md). The behaviour-preserving aarch64 core/backend refactor is admitted by the ADR-0022 amendment of the same date. ARM groups 2 and 3 are design-only and unscheduled.
**Decider:** Cellos maintainer, approving the image-family split and the aarch64 three-group model in the planning conversation.
**Implementation authority:** No ABI, manifest field or driver-admission change is approved here. Each concrete change goes through its own plan.

## Context and drivers

Each board currently gets its own image. The kernel picks a `BoardDescriptor` at compile time with cargo features (`board-x86-pc`, `board-vf2`, `board-pioneer`, `board-rpi3`, `board-rpi4`) through `const SELECTED_*_BOARD` in `kernel/src/board.rs`. `has_driver()` then gates driver init.

On some architectures, boot-time discovery already exists:

- **x86-64:** ACPI MADT/HPET/MCFG/DMAR parsing and PCIe ECAM enumeration (`kernel/src/main.rs`). NICs are chosen by `ethernet_id_has_driver(vendor, device)`. `QEMU_Q35` and `GENERIC_X86_PC` (`hal/soc/x86`) share the same COM1 wiring and firmware windows. The descriptors differ only in the driver list and `dma_isolation`.
- **riscv64:** `platform::from_dtb` reads UART/PLIC/CLINT from the firmware DTB. Compiled defaults are only a fallback.
- **aarch64:** about 190 `cfg(feature = "board-rpi*")` sites run through core files (`boot.rs`, `vcpu.rs`, `trap.rs`, `timer.rs`, `hypervisor/registry.rs`). This is board-specific code mixed into the architecture core.

Drivers:

- Fewer images per architecture.
- No silent weakening of the DMA isolation invariant (`DmaIsolation::Required`).
- The HCL (`docs/hardware-compatibility-list.md`) stays authoritative.
- Small kernel trusted code base.
- Stay aligned with ADR-0022's single x86-64 focus.

## Decision

1. **One image per architecture family where firmware describes the machine, not one image per board.** The image detects at boot from ACPI, DTB, SMBIOS and PCI, and binds one profile from a compiled catalog of approved profiles.
2. **Detected facts never set policy.**
   - The profile supplies `dma_isolation` and the driver allow-list.
   - Detection may select only the strictest applicable profile, or validate and reject an operator-selected one (for example a boot-option slug). A weaker profile, such as `DmaIsolation::Optional`, needs explicit operator selection plus a consistent detected fact (for example the CPUID hypervisor bit for QEMU).
   - No acceptable profile means the machine is rejected or boots a minimal safe mode, with the reason logged.
   - "No DMAR found" never downgrades `Required` to `Optional`.
3. **A one-time setup pins the configuration instead of skipping detection.** The first boot records a fingerprint: SMBIOS/DMI identity or DTB root `compatible`, the PCI ID set, the matched profile slug and, where relevant, the firmware version.
   - Every later boot detects again and compares against the record.
   - A mismatch (changed device, firmware update, VT-d disabled) stops boot for re-confirmation. It is never accepted automatically.
   - The record grants no capability and carries no authority.
4. **Driver families are Cells matched by device ID, not kernel code.** A broad image ships many driver Cells in its initrd or store. Only matched ones are spawned. The kernel does not grow per supported device.
5. **Architecture families:**
   - **x86-64:** one image covering QEMU q35 and generic PCs. Scheduled first under ADR-0022.
   - **riscv64:** one image with the SoC profile chosen from the DTB root `compatible`. Built as PIC. Boards without a DTB still need an explicit selection. Unscheduled.
   - **aarch64 is split into three groups over a shared core** (paging, traps, EL1/EL2, architected timer, vCPU):
     - **Group 1, embedded (Raspberry Pi and similar):** per-board images with compile-time selection, kept for size and determinism.
     - **Group 2, SystemReady PC/server:** one image using UEFI with ACPI or DTB, GICv3 and SMMUv3. Design-only until a target board exists.
     - **Group 3, Apple Silicon:** a separate family, one image per chip generation. DTB comes from the m1n1 bootloader. Needs AIC, DART and the RTKit coprocessor mailbox. Reuses the core, DTB parsing, PCIe enumeration and PCIe-attached drivers. DART is a real IOMMU, so `DmaIsolation::Required` applies. Design-only, outside the roadmap.
6. **The aarch64 core/backend boundary is designed now.** Interrupt controller, IOMMU and platform discovery sit behind traits, and the RPi `cfg` sites move into the embedded backend. The behaviour-preserving refactor is admitted by the ADR-0022 amendment of 2026-10-31. New backends are not.
7. **Apple Silicon work is clean-room only.** Asahi Linux drivers are GPL-2.0 and must not be copied or ported into this Apache-2.0 OR MIT tree. Published hardware documentation and permissively licensed m1n1 code (licence to be verified per file) may be used as references, and the provenance is recorded.

## Considered options

| Option | Outcome | Reason |
|---|---|---|
| Keep one image per board everywhere | Rejected for x86/riscv64/server ARM | Duplicates images whose only difference firmware already reports. Keeps the redundant `board-x86-pc` feature. |
| One universal image per ISA, including embedded and Apple | Rejected | RPi has no complete firmware description and needs compile-time size trimming. Apple lacks ACPI/GIC/SMMU, so a SystemReady image cannot run on it. Forcing a single image means untested branches in the core. |
| Detect from firmware and let detected facts decide policy (for example no DMAR ⇒ allow untranslated DMA) | Rejected | Turns firmware omissions and lies into a silent isolation downgrade, which contradicts the `DmaIsolation` contract. |
| Cache detection results and skip detection on later boots | Rejected | Saves only milliseconds, since device init dominates boot time. Hides hardware and firmware changes that matter for security. |
| Merge Apple Silicon into group 2 | Rejected | Different boot chain, interrupt controller and IOMMU. Only the core and PCIe pieces are shared. |
| Port Asahi drivers | Rejected | GPL-2.0 is incompatible with the project licence. |
| Catalog-matched detection with a pinned fingerprint, three aarch64 groups over a shared core | **Chosen** | Fewer images, policy stays explicit, hardware changes are visible, embedded optimisation is kept. |

## Consequences

- x86-64: `board::selected()` becomes a runtime catalog match and the `board-x86-pc` feature is removed. HCL entries map to catalog profiles plus pinned fingerprints.
- A persistent store for the fingerprint record and a re-confirmation boot path are needed. Their format is defined by the implementing plan.
- `bring-up-drivers` stays as a separate minimal-image axis.
- aarch64 groups 2 and 3 have no code, tests or HCL entries until scheduled. Any work on them must first amend ADR-0022's scope.
- Apple Silicon support, if ever scheduled, would follow Asahi's hardware coverage (realistically M1/M2) and requires a documented clean-room process.

### Amendment 2026-11-02 — fingerprint store, safe mode and trust (plan 261031-1241 phase 03)

- **Hand-off.** No boot-time record could carry the PCI set, which is complete only after the Platform Cell runs. The kernel answers a SpawnCap-gated syscall 433 `QueryMachineFacts` with one `ViMachineFactsV1` (profile slug and source, ACPI root OEM, SMBIOS type 1, DMAR/VT-d/hypervisor flags, sorted PCI multiset, SHA-256). Approved as Law-1 (CP1 2026-11-02).
- **Store.** `/srv/.cellos-machine/pin` on CellosFS P5, not `/etc/cellos` (synthetic and read-only). The VFS admits only the live `MACHINE_PIN` provider (service id 20), which the kernel accepts only from trusted init naming itself. No durability is claimed.
- **Safe mode.** Init gates every configured service. A mismatch, an unreadable or unknown-version record, or missing facts withhold everything except `input` and the `cellbox` shell; boot-critical storage keeps running under catalog policy. A record is never re-pinned automatically: only definitive absence on a healthy store is a first boot. No store is safe mode under `Required` profiles and a warning under the `Optional` q35 profile.
- **Re-confirmation.** `machine pin --confirm` from the console shell init started, in safe mode, for the digest init reports; it takes effect at the next boot.
- **Trust.** The pin carries no authority and changes no profile or DMA decision. It is change detection, not a boot-integrity control: `limine.conf` (which selects the profile) and the kernel are unmeasured today, and an offline writer can delete the pin. Phase 02's note that firmware "cannot inject the command line" holds only on a measured or Secure Boot setup that does not exist yet (HCL R3).

## Related

- [ADR-0022](0022-intel-x86-64-c2c-only-direction.md): sole x86-64 direction and scheduling.
- [ADR-0024](0024-cell-kernel-authority-contract.md): driver Cells, least authority.
- `boards/pc/x86_64-pc/README.md`, `docs/hardware-compatibility-list.md`.
