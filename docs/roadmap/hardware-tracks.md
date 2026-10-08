# Hardware Tracks

**Last updated**: 2026-10-08

This page collects the hardware qualification lanes that matter for roadmap
reading. For the architecture split between board descriptors, SoC facts, and
shared drivers, see [system-architecture.md](../system-architecture.md).

## Sole Current Direction — Intel x86-64 C2C

[ADR-0022](../decisions/0022-intel-x86-64-c2c-only-direction.md) makes
Cell-to-Cell Anywhere on one fixed, headless Intel x86-64 configuration the
sole program. Every hardware task must name its direct C2C deliverable,
dependency, or regression obligation. Select and qualify one exact Intel
machine first; only then consider a second independently qualified Intel node.
Matching configuration is preferred, not mandatory. This is not purchase authorization:
phase 07 and an explicit procurement decision remain mandatory.

AMD, new ARM/RISC-V ports and expansion, GUI/browser, AI, robotics, and
general-purpose OS programs are paused. Existing implementations, inventory,
historical evidence, and necessary shared-code cross-architecture regressions
are retained, not converted into new platform work. Strategy approval grants
no ABI/security exception, remote activation, or production readiness.

## Board and SoC Ownership

- Root `boards/` owns board descriptors and audited fallback assets.
- `hal/soc/*` owns immutable SoC facts.
- Shared drivers stay single-copy in `cells/drivers/` or the relevant kernel
  integration path; boards do not fork UART, SDHCI, GIC/PLIC, PCIe, or similar
  mechanism code.

## Retained Qualification Evidence (Non-Intel Expansion Paused)

- Prior RPi3 physical smoke is merged as real evidence for the exact captured
  device; it is not evidence for either current Model B+ board until identity
  reconciliation.
- VF2, Pioneer, and RPi4 are retained inventory/evidence lanes, not active
  qualification objectives. New captures or ports are not independently queued.
- QEMU and compile-only checks are regression evidence, not board qualification.

## Intel x86-64 C2C Lane (QEMU prerequisites landed; no physical qualification)

There is no supported physical x86 target today: no machine is qualified and no
machine-specific descriptor exists. `boards/` carries `qemu/q35-x86_64`, the
placeholder entries (`q35-x86_32` is README-only), and — since phase 01 of
`.agents/261004-1957-x86-pc-lane/` — a **generic** `pc/x86_64-pc` descriptor
(selected by `--features board-x86-pc`) that declares the PC-class compatibility
baseline and only drivers whose cells exist — not a claim that every PC exposes
that wiring. Every x86 Capability Lane row still carries a
`qemu` ceiling, and QEMU/compile results never qualify a board. The table below
records prerequisites and their evidence ceilings; the owner is
`.agents/261004-1957-x86-pc-lane/` (phases 01–06 completed at QEMU only; phase 07
is the separately authorized hardware gate), and the machine-level list lives in
[hardware-compatibility-list.md](../hardware-compatibility-list.md). Driver
completion is not procurement authorization or physical qualification.

**Boot-critical requirement.** The fixed headless target requires a
16550-compatible UART at COM1 (`0x3F8`, IRQ 4), including a correctly mapped
BMC serial-over-LAN option. xHCI/HID has QEMU evidence, but that is neither a
USB-serial console nor a physical-board qualification; no framebuffer,
PS/2, or netconsole alternative is implied. Polled RX preserves debugging
when the MADT/HPET IRQ gate closes, so a shell prompt alone does not prove
the address/IRQ contract. Require the exact-resource and exercised-RX HCL
capture, and exposed HPET.

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
| X86-PC-0 | Exact Intel configurations, machine-specific descriptor and HCL capture | Generic `pc/x86_64-pc` descriptor and HCL model landed in phase 01; no machine-specific descriptor or physical HCL row | First exact Intel machine, then a separately qualified second Intel node; matching preferred, not required; phase 07 and procurement approval remain gates |
| X86-PC-1 | AHCI/SATA storage | Phases 02a/02b completed at `qemu` only, including two-boot FAT32 persistence | Exact physical storage controller still needs capture |
| X86-PC-2 | xHCI + HID | Phases 03/03b completed at `qemu` only, including shell key delivery | Retain as a substrate/regression dependency; no GUI program or physical USB claim |
| X86-PC-3 | Ethernet with a shipped exact device ID | Phases 04a/04b completed at `qemu` only; `igb` supports `8086:10c9` (QEMU 82576) and `8086:1533` (flash-backed i210) | i210/i211 was the family research target, not broad SKU support; neither i211 nor flashless i210 is admitted by that claim; no actual NIC is qualified |
| X86-PC-4 | ACPI DMAR → VT-d DMA isolation | Phase 05 completed at `qemu` only; DMAR discovery and `DmaIsolation` profile contract landed | Physical profile requires VT-d; no generic-PC q35-base fallback or untranslated-DMA waiver |
| X86-PC-5 | Multi-port COM / RS232-485 | Phase 06 completed at `qemu` only; RS485 DE/RE timing remains unclaimed | Retained evidence; further serial work requires a named C2C dependency, not an industrial side program |
| X86-PC-6 | Intel VMX/VT-x + EPT guest execution | Intel guest execution remains incomplete; `.agents/260711-1917-tier3b-x86-vtx/phase-09-vtx-backend-apic.md` owns the prerequisite | Required for Intel Tier 3 and the explicit C2C guest adapter; Intel hardware or a suitable nested-KVM lane is needed; AMD SVM QEMU evidence never qualifies VMX |
| X86-PC-7 | CPU-inference kernels and vector-state work | Historical inventory only, parked | AI/SIMD is not an independent objective; reopen only for an explicit direct Intel C2C dependency |

Retained x86 substrate evidence includes Limine BIOS+UEFI boot, ACPI
MADT/HPET/MCFG discovery, COM1 with polled fallback, NVMe, and virtio.
The AMD SVM Tier 3 backend under QEMU-TCG remains regression/reference evidence
only, not Intel guest readiness or an AMD hardware objective.

## Retained RPi3 Inventory (No New Platform Program)

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
- **Available peripherals.** One HDMI cable is retained with the historical
  inventory. A camera is available but its model/interface is still unrecorded;
  camera and display expansion are paused, not next-session objectives.

## Placeholder-Only Board Entries

- `q35-x86_32`
- `virt-riscv32`
- `virt-aarch32`

These entries are retained documentation only. New ports and expansion are
paused; their presence is not an active hardware claim or scheduling authority.

## Shared-Lane Rule

If a hardware change affects a common peripheral mechanism, update the shared
driver or HAL layer once rather than cloning the behavior per board.
