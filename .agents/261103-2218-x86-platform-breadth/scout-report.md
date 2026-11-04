# Scout report — x86 platform breadth (KVM, VMware, Intel PC/server)

Recon date 2026-11-03, branch `main`. Facts are `file:line`; anything not read is marked.

## Platform core

| Area | Fact |
|---|---|
| Timer gate | `kernel/src/main.rs:512` `x86_timer_ready = lapic && ioapic && hpet`; `main.rs:596-656` binds LAPIC/IOAPIC/HPET, else `[x86-gate] timer CLOSED` |
| HPET as clocksource | `hal::hpet::now_ns`/`checked_sample` consumed by `kernel/src/task/syscall.rs:8120,8143` (GetTime), `task/telemetry.rs:41`, `hal/arch/x86/src/x86_64/svm_vcpu.rs:126`, `hal/arch/x86/src/x86_64.rs:346`; G1 private lane only (`#[cfg(cellos_g1_cpu)]`): `task/cpu_accounting.rs:258-268,593,683,1447`, `main.rs:604-649` |
| LAPIC calibration | `hal/arch/x86/src/x86_64/hpet.rs:207` HPET 10 ms window, PIT ch2 fallback `hpet.rs:171` (unreachable today because the gate needs HPET) |
| No TSC/kvmclock | no `rdtsc`, CPUID 0x15/0x16, invariant-TSC or 0x4000_00xx use in `hal/arch/x86`, `kernel/src` |
| x2APIC refused | `hal/arch/x86/src/x86_64/apic.rs:22,72,93,233`; Limine MP request `kernel/src/boot/limine.rs:229` `flags: 0` |
| MADT | `kernel/src/acpi.rs:419-510`: one IOAPIC kept (last wins), override flags dropped, types 0/9/0xA ignored |
| MCFG | `acpi.rs:524-566`: first segment-0 allocation only |
| DMAR | `acpi.rs:638-712`: one DRHD selected, RMRR/ATSR skipped; VT-d driver single unit `task/drivers/iommu_x86.rs:60,66,546`; no interrupt remapping |
| ACPI tables parsed | only `APIC`, `MCFG`, `HPET`, `DMAR` (`acpi.rs:395-398`); no FADT/SPCR/DSDT |
| Power | no reset/poweroff path in `kernel/src` or `hal/arch/x86`; `cells/services/power/src/lib.rs` is a 35-line trait stub |
| SMP | `kernel/src/task/smp.rs:16` `MAX_HARTS = 2`, `HART_RT = 1` (49 uses), `boot ^ 1` mapping `smp.rs:282-297`, `x86_secondary_main` asserts `logical == 1` (`smp.rs:966+`); x86 `MAX_CPUS = 2` `hal/arch/x86/src/x86_64/syscall.rs:58` used by `apic.rs`, `gdt.rs`, `ist.rs`; fixed-size arrays `task/hart_local.rs:43`, `hart_local/ready.rs:116`, `cpu_accounting.rs:19`, `memory/heap/p01_owner.rs:16` |

## PCIe, IRQ, DMA ABI

| Area | Fact |
|---|---|
| Cell IRQ wait | `WaitIrq = 234` (`libs/api/src/abi/syscall.rs:95`), `a0 = irq_num (u8)`; only virtio-MMIO/GPIO use it; PCIe cells never call it |
| MSI/MSI-X | `task/drivers/pcie_ecam.rs:162-168,196-203,547-597` capture capability; `:768-776` quiesce only; never programmed |
| Device query | `FindPcieDevice = 418`, `FindPcieDeviceByVendor = 424`; `PcieDeviceInfo` (`libs/ostd/src/syscall.rs:2747`) exposes BAR0 + first memory BAR only, no capability list |
| DMA | `GrantDma = 233` maps into IOMMU per BDF (`syscall.rs:85-89`) |
| Highest syscall | `QueryMachineFacts = 433`; allowlist bitmap full (phase 03b record in `.agents/261004-1957-x86-pc-lane/plan.md:250-254`) |
| ECAM | single base `pcie_ecam.rs:84-108` |

## Drivers

| Driver | Admission | Interrupts |
|---|---|---|
| `cells/drivers/e1000` | `8086:100E` only (`pcie_ecam.rs:37,60`) | polling |
| `cells/drivers/igb` | `8086:10C9`, `8086:1533` (`igb/src/identity.rs:35,61-69`) | polling |
| `cells/drivers/nvme`, `ahci` | class triple | polling |
| `cells/drivers/xhci` | class `0C:03:30`; one boot keyboard on a root port; no hubs (`xhci/src/controller.rs:3-14`); no USBLEGSUP BIOS handoff (grep empty) | polling (`controller.rs:303`) |
| `cells/drivers/virtio-{blk,net,gpu}` | MMIO slots only; x86 branch `core::iter::empty()` (`virtio-blk/src/device.rs:175-178`) | — |
| virtio-drivers fork | `kernel/third_party/virtio_drivers` (root `Cargo.toml:245`), has `transport/pci.rs` + `pci/bus.rs`; `PciRoot::bar_info` sizes BARs by writing config space (`pci/bus.rs:267`), so it needs config-write access a cell does not have |
| Stale test | `tests/integration/tests/virtio-x86.rs` targets kernel `virtio_pci.rs`, deleted in `543621013` (Kernel Boundary Law: no device drivers in kernel) |
| Console | COM1 only (`hal/arch/x86/src/x86_64/uart_16550.rs:183,224`); PS/2 kernel driver feeds `console_drv.rs:160-171`; Limine framebuffer mapped for the display cell (`main.rs:1461-1482`), no text console |
| Profiles | `hal/soc/x86/src/lib.rs:274` `X86_PROFILES = [QEMU_Q35, GENERIC_X86_PC]`; `boards/pc/x86_64-pc/board.rs:16-26` drivers; `dma_isolation: Required` |

## Test harness and lanes

- `tests/integration/src/lib.rs:823-1490` x86 `QemuRunner` constructors (bios, smp, accel kvm, nvme, sata, xhci, nic, igb, serial ports, vtd, machine pin).
- x86 suites: `ahci-x86`, `igb-x86`, `iommu-dmar-x86`, `machine-pin-x86`, `nic-x86`, `nvme-x86`, `pcie-multibus-x86`, `serial-x86`, `virtio-x86` (stale), `x86_64-boot`, `xhci-x86`, `desktop-x86`, `local-service-lifecycle-x86`.
- CI job `qemu-x86_64-boot` at `.github/workflows/ci.yml:909`.
- VMware: `scripts/x86/vmware-boot-test.py`, `test-native-vmware-matrix.py` — local Workstation Pro, BIOS boot, 2 vCPU; NIC/SATA/SCSI disabled in `vmware-boot-test.py:109-113`; the matrix keeps ethernet off and SATA optional (`test-native-vmware-matrix.py:73-83`).
- Device lookup: `find_class`/`find_vendor_device` return the first match; if a live driver owns it, `offer_pcie_device` returns 0 (`kernel/src/task/syscall.rs:2428-2470,8722-8743`), so a second identical device is unreachable today.
- The CPU-accounting capture binds to HPET identity (`kernel/src/task/cpu_accounting.rs:258-268`), but the module is `#[cfg(cellos_g1_cpu)]` (`kernel/src/task.rs:104`) and in-memory only (`G1-*` log lines).
- Host: `/dev/kvm` present; QEMU 8.2.2 has `e1000`, `e1000e`, `igb`, `vmxnet3`, `pvscsi`, `virtio-*-pci`, `virtio-iommu-pci`, `intel-iommu`, `usb-hub`, `qemu-xhci`, `megasas`, `mptsas1068`; **no** `igc`, `r8169`, `tg3`, `ixgbe`, `i40e` models.

## Governance

- ADR-0022: Intel-only, "keep one NIC/storage path; do not broaden device support" (§Hardware item 2) — this plan needs an amendment.
- ADR-0029: Optional DMA only via explicit `cellos.profile=` under a hypervisor.
- `CELL-CONTRACT-0024` lock covers device-authority/Cell-contract changes; handoff format: `.agents/261031-1241-x86-runtime-board-selection/lock-handoff-approval.md`.
- Portfolio registration: `.agents/plan-portfolio.md:162-179` table format.
