# Hardware Tracks

**Last updated**: 2026-10-04

This page collects the hardware qualification lanes that matter for roadmap
reading. For the architecture split between board descriptors, SoC facts, and
shared drivers, see [system-architecture.md](../system-architecture.md).

## Board and SoC Ownership

- Root `boards/` owns board descriptors and audited fallback assets.
- `hal/soc/*` owns immutable SoC facts.
- Shared drivers stay single-copy in `cells/drivers/` or the relevant kernel
  integration path; boards do not fork UART, SDHCI, GIC/PLIC, PCIe, or similar
  mechanism code.

## Current Qualification Lanes

- Prior RPi3 physical smoke is merged as real evidence for the exact captured
  device; it is not evidence for either current Model B+ board until identity
  reconciliation.
- VF2, Pioneer, and RPi4 remain physical-only qualification lanes unless a log
  explicitly records PASS/FAIL/BLOCKED evidence.
- QEMU and compile-only checks are regression evidence, not board qualification.

## x86_64 PC/Server Lane (planned — prerequisite inventory, no physical qualification)

There is no supported physical x86 target today: no machine is qualified and no
machine-specific descriptor exists. `boards/` carries `qemu/q35-x86_64`, the
placeholder entries (`q35-x86_32` is README-only), and — since phase 01 of
`.agents/261004-1957-x86-pc-lane/` — a **generic** `pc/x86_64-pc` descriptor
(selected by `--features board-x86-pc`) that declares the PC-class compatibility
baseline and only drivers whose cells exist — not a claim that every PC exposes
that wiring. Every x86 Capability Lane row still carries a
`qemu` ceiling, and QEMU/compile results never qualify a board. The table below
records the missing prerequisites; the owner is
`.agents/261004-1957-x86-pc-lane/` (phases 01–07, QEMU-first; phase 07 is the
hardware gate), and the machine-level list lives in
[hardware-compatibility-list.md](../hardware-compatibility-list.md). Nothing
outside that plan is authorized by this inventory.

**Boot-critical requirement (current code).** On x86 the only working log and
input path is a 16550-compatible UART at COM1 (`0x3F8`, IRQ 4)
(`hal/soc/x86/src/lib.rs`, `kernel/src/main.rs:157`,
`kernel/src/task/drivers/console_drv.rs:139-145`). Without it the boot is
silent: there is no xHCI (USB-serial), no PS/2, no framebuffer console driver
(`cells/apps/fb-console` needs a real display driver), and no netconsole. Polled
RX keeps debug alive when the MADT/HPET gate closes the IRQ
(`kernel/src/main.rs:160,711-716`). Treat "COM1 or an equivalent 16550 (including
BMC serial-over-LAN) at the configured base" as a must-have HCL row.

**Firmware requirement — Secure Boot must be disable-able.** Cellos has no
signed or measured x86 boot path: code-signing/secure-boot is a separate
Security-track item (`.agents/260605-2107-full-reliability-track/plan.md:76`,
out of scope there as "load-bearing for trust model, not for reliability"), and
secure/measured boot appears only as a production-release-gate requirement
(`docs/specs/12-reliability.md:72`, `docs/roadmap/open-risk-register.md`).
A board whose firmware offers no way to disable Secure Boot therefore **cannot
be made compatible** — the Cellos ISO will not boot and there is no exemption
path to design around. Record "Secure Boot can be disabled, or ships off" as a
hard HCL row, not a preference, and record the BIOS option/version for each
qualified machine.

| Gate | Missing capability | Blocking evidence | Notes |
|---|---|---|---|
| X86-PC-0 | Machine-specific descriptor + a published HCL row per machine + physical lane (one Intel, one AMD) separate from QEMU | **Generic descriptor + HCL model landed in phase 01** (`boards/pc/x86_64-pc`, `--features board-x86-pc`, `boards/pc/x86_64-pc/README.md`); no machine-specific descriptor and no HCL row exist yet | Descriptor + `SocId::GenericX86Pc` profile are facts and contract only; the physical half still needs hardware (phase 07) |
| X86-PC-1 | AHCI/SATA storage | No AHCI driver in the Cellos source (`kernel/src/task`, `cells/drivers`, `hal/soc`, `boards`); the only `ahci` strings in the tree are inside the embedded guest Linux artifacts (`kernel/src/embedded-hv*/kernel_fs.img`). `cells/drivers/` ships `nvme`, `virtio-blk`, and `disk` only | Highest-value gap for this class: industrial PCs are SATA/mSATA/SATA-DOM. Blocks `/data`, a persistent guest disk, and storage throughput. It does **not** block booting: Cellos boots from the embedded VIFS1 (`kernel/src/embedded-hv-x86/kernel_fs.img`, 60 MiB) and the Tier 3 guest takes `/vmlinux` + `/initrd.gz` from the hypervisor cell's filesystem (`scripts/make-hypervisor-fs-x86.sh:9,118-119`); `virtio-blk` keeps a 4 MiB volatile fallback (`cells/services/hypervisor/src/virtio_blk.rs:4`) |
| X86-PC-2 | xHCI + HID | No x86 USB host controller driver; `cells/drivers/dwc2-usb` is BCM2837-only | Previously frozen out of the G1–G3 common-driver plan by decision (`.agents/260819-1416-port-common-drivers-g1-g2-g3/phase-01-evidence-and-provenance-gate.md` item 5, `reports/driver-source-license-bom.md:32`), so reopening it is a scope decision, not an oversight |
| X86-PC-3 | Real NIC — first family Intel `igb` (i210/i211) | `cells/drivers/e1000` implements 82540EM only and `kernel/src/task/drivers/pcie_ecam.rs:894` fail-closes every other Ethernet-class binding; `RTL8125/i225` are research-only in the driver plan | i210/i211 have public datasheets and an upstream reference (`igb`); the q35 e1000 DHCP gate (`.agents/260903-x86-e1000-dhcp/`) is QEMU-only |
| X86-PC-4 | ACPI DMAR discovery → real IOMMU | `kernel/src/task/drivers/iommu_x86.rs:59-60` hardcodes the q35 base | DMA isolation on real hardware; requires board VT-d (present on Whiskey-Lake-class parts, absent on Haswell-ULT U-series) |
| X86-PC-5 | Multi-port COM / RS232-485 with DE/RE control | `cells/drivers/serial` is PL011 (ARM); x86 exposes the COM1 console only | Industrial deployment requirement, not boot-critical |
| X86-PC-6 | Intel VMX guest execution (Tier 3 on Intel) | `docs/guides/tier3b-linux-vm.md` Platform Support: "Intel VT-x guest execution is not implemented"; `hal/arch/x86/src/hypervisor.rs` returns `NotSupported`; `.agents/260711-1917-tier3b-x86-vtx/phase-09-vtx-backend-apic.md` is pending | Needs a real-Intel/KVM lane. AMD SVM remains the only implemented x86 Tier 3 backend, itself QEMU-qualified only |
| X86-PC-7 | x86 CPU-inference kernels (AVX2/FMA) + kernel vector-state decision | AVX2/NEON/RVV kernels are an explicit open item, not a hidden defect: `.agents/260914-cpu-engine-optimization/plan.md:51-53` keeps them out of the default profile because a Cell also runs on CPUs without them and on a kernel that does not save vector state | Independent of X86-PC-0..6; track it with Spec 24, not with the storage/network gates |

Already working on x86 and therefore not gaps: Limine BIOS+UEFI boot, ACPI
MADT/HPET/MCFG discovery (`kernel/src/main.rs:514-516,725-739`), the COM1 console
with polled fallback, NVMe, the virtio device stack, and the AMD SVM Tier 3
backend under QEMU-TCG.

## Available RPi3 Inventory

- **Current inventory — `2 × Raspberry Pi 3 Model B+`, owner-reported;
  reconciliation pending.** No provisional board labels are assigned. Record
  each board's exact serial, revision, and current condition before binding
  runtime evidence to it.
- **Prior exact-device run — current-inventory mapping unresolved.** Firmware
  and U-Boot reported board revision `a22082`, `RPI 3 Model B`, 948 MiB DRAM,
  and unique serial `000000003d042795`. This record is not assigned to either
  current Model B+ board. The reviewed 2026-08-28 HDMI image has SHA-256
  `566b73a2e4b3499d564a8e40da41ff895cc1ff61f7388d1894881522c8e8e202`.
  The repository TFTP log independently records the final 9,642,048-byte
  transfer at 2026-08-28 11:14:54. Separately,
  `.agents/debug/rpi3-b-hdmi-reviewed-20260828.raw` contains an earlier boot at
  lines 37–210 and a later reviewed-image boot beginning around line 253. The
  later block records one 4,096-byte mailbox page, accepted cache begin/exact
  completion, framebuffer base `0x3e876000`, size 3,686,400, 1280x720, pitch
  5,120, BCM registration, fb-console damage, and a completed first scanout
  flush without a cell fault. The UART file has no host timestamp or image hash,
  so it does not itself prove the TFTP event's 11:14:54 timestamp. The user
  separately observed the cold-connected display remain lit for more than 10
  minutes with fb-console and cursor movement. This closes the HDMI visual gate
  at exact-device development evidence only; it does not establish production
  qualification. Connecting HDMI after firmware startup previously produced
  black / `No Signal`; that remains a reproduction condition, not an isolated
  root cause.
- **Historical capture — exact board unassigned.**
  `.agents/debug/rpi3-hdmi-data-path-long-capture.raw` reports revision
  `a22082`, reaches the Cellos shell, mounts FAT16/FAT32/littlefs, registers the
  BCM display driver, and completes its first scanout flush. It contains no
  unique serial, so it cannot be attributed to either current Model B+ board.
- **Current access state.** COM4 and the direct 100-Mbps Ethernet link were
  usable for the prior exact-device run. The COM4 recorder and verified
  repository TFTP process were stopped after the final capture.
- **Available peripherals.** One HDMI cable is retained for regression testing.
  A camera is available but its model/interface is still unrecorded and sensor
  integration is deferred in the current session order.

## Placeholder-Only Board Entries

- `q35-x86_32`
- `virt-riscv32`
- `virt-aarch32`

These entries exist for documentation and future expansion, not as active
hardware claims.

## Shared-Lane Rule

If a hardware change affects a common peripheral mechanism, update the shared
driver or HAL layer once rather than cloning the behavior per board.
