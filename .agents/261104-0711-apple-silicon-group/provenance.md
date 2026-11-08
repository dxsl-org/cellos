# Apple Silicon provenance ledger

**Rule:** [ADR-0032](../../docs/decisions/0032-apple-silicon-secondary-lane.md) §7 and [ADR-0030](../../docs/decisions/0030-license-model-and-copyleft-boundary.md). Add a row **before** review for every Cellos file whose behaviour comes from an external Apple Silicon source. A file with no row is not reviewable.

## Source classes

| Class | Sources | Allowed use |
|---|---|---|
| P — permissive code | OpenBSD `sys/arch/arm64/dev/apl*.c` (ISC); m1n1 tag `v1.9.9` and earlier (MIT); Linux/Asahi files whose own SPDX offers MIT (`drm/asahi/*.rs`, `soc/apple/{rtkit,mailbox,sart}.c`; check every file); OpenBSD/FreeBSD `bge`, Aquantia drivers (ISC/BSD) | Read and adapt, with attribution in `NOTICE` when code is adapted |
| D — hardware data | Linux `arch/arm64/boot/dts/apple/*` (`GPL-2.0+ OR MIT`, used under MIT); DT binding YAML (BSD-2-Clause option); captured ADT/FDT/PCI IDs from our own machines | Facts, register maps, test fixtures |
| T — external tools | m1n1 (any version) proxy/hypervisor, Asahi installer, U-Boot | Run as separate, unmodified programs; never vendored into Cellos artifacts |
| X — forbidden | Linux/Asahi files without a permissive option (e.g. `irq-apple-aic.c`, `apple-dart.c`, `pcie-apple.c`, `nvme/host/apple.c`) and the Linux `kernel::` abstractions; m1n1 ≥ `v2.0.0` source; U-Boot source; disassembly or decompilation of Apple binaries/firmware | Not read by anyone who writes the matching Cellos component |

## Tainted sessions

A human or AI-agent session that has loaded class-X material for a component is recorded here. It may write a behavioural spec for that component, but not the code.

| Date | Session / person | Component | Material |
|---|---|---|---|
| _(none)_ | | | |

Note (2026-11-04, planning session): the planning session read the m1n1 `README.md` licence section, the first three lines of `src/aic.c` (SPDX header only), the OpenBSD `apldart.c` header and register defines (class P), and Linux Apple DT sources (class D). No class-X implementation was read.

## Ledger

| Cellos file | Component | Sources (class, exact path + revision) | Notes |
|---|---|---|---|
| `hal/arch/arm/src/aarch64/translation_config.rs` | Translation config | Class D: Arm Architecture Reference Manual ARMv8 (DDI 0487) | PA/VA translation derivation |
| `libs/tensor-math/src/neon.rs` | NEON SIMD kernels | Class D: Arm Architecture Reference Manual ARMv8 NEON intrinsics | `vmlaq_f32`, `vmull_s8`, `vpadalq_s16` |
| `boards/apple/macmini-m1/board.rs` | Mac mini M1 descriptor | Class D: Linux `arch/arm64/boot/dts/apple/t8103-j274.dts` (MIT option) | DRAM base, peripherals, compatible strings |
| `boards/apple/macmini-m2/board.rs` | Mac mini M2 descriptor | Class D: Linux `arch/arm64/boot/dts/apple/t8112-j473.dts` (MIT option) | DRAM base, peripherals, compatible strings |
| `kernel/src/task/drivers/iommu_pt.rs` | Apple DART page table builder | Class P: OpenBSD `sys/arch/arm64/dev/apldart.c` (ISC) | 16 KiB granule, 2-level mapping, bypass refusal |
| `libs/apple-rtkit/src/lib.rs` | Apple RTKit, SART, PartitionGuard | Class P: OpenBSD `aplrtk.c`, `aplmbox.c`, `aplsart.c` (ISC); Linux `soc/apple/{rtkit,mailbox,sart}.c` (MIT option) | 128-bit ASC mailbox, SART filtering, partition protection |
| `docs/baremetal/apple-silicon.md` | Installation guide | Class D: m1n1 user guide (MIT tag `v1.9.9`), Asahi UEFI documentation | Partition layout, 1TR bputil, Limine EFI |
