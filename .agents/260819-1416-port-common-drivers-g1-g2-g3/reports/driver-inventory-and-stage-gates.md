# Driver Inventory and Stage Gates

Status legend: `promoted` = explicit active lane with board/spec evidence; `present` = code/selection exists but not yet the promoted lane for this stage; `prototype` = partial/deferred/fallback only; `missing` = no in-tree implementation for that lane. Evidence legend: `compile`, `qemu`, `synthetic`, `physical`.

## DriverId matrix

| DriverId | G1 | G2 | G3 | Evidence | Basis |
|---|---|---|---|---|---|
| `ConsoleSbiDbcn` | missing | present | missing | compile | SG2042/Pioneer descriptor only; not a G1/G3 baseline. |
| `UartNs16550a` | present | missing | missing | qemu | QEMU virt baseline, shared legacy console path. |
| `UartDwApb` | missing | missing | missing | compile | `DriverId` exists; no Cell/driver promotion yet. |
| `UartPl011` | promoted | missing | missing | qemu | ARM virt baseline; spec-13 marks QEMU peripheral lane shipped. |
| `UartBcmMini` | present | missing | missing | compile, physical | RPi3 mini-UART RX/AUX IRQ/scheduler lane is physically proven; this does not promote new GPIO/I2C controllers. |
| `PlicSifive` | present | present | missing | qemu, compile | Current RV64 shared interrupt substrate. |
| `ClintSifive` | present | present | missing | qemu, compile | Current RV64 timer substrate. |
| `GicV2` | promoted | missing | missing | qemu | ARM virt G1 baseline. |
| `IrqBcm2836Local` | present | missing | missing | compile, physical-blocked | Descriptor-selected; full BCM local-controller qualification beyond the proven mini-UART lane remains blocked. |
| `IrqBcm2835Legacy` | present | missing | missing | compile, physical | AUX legacy IRQ29 for the RPi3 mini-UART RX path is physically proven; other BCM2835 legacy IRQ sources/paths remain unqualified. |
| `TimerBcm2835System` | present | missing | missing | compile, physical-blocked | RPi3 descriptor-selected only. |
| `RtcGoldfish` | present | missing | missing | qemu | ARM virt/QEMU RTC lane. |
| `RtcPl031` | present | missing | missing | qemu | ARM virt/QEMU RTC lane. |
| `VirtioMmio` | promoted | prototype | missing | qemu | Strong QEMU baseline; not the promoted real-controller G2 lane. |
| `PcieEcam` | missing | promoted | present | qemu, compile | q35 G2 baseline; G3 depends on the same host substrate. |
| `SdhciArasan` | present | missing | missing | compile, physical | RPi3 MMC/SDHCI boot, sector discovery, FAT16/FAT32, `/mnt/sd`, and `/bin` are physically proven in current repo docs. |
| `SdhciDwCqe` | missing | present | missing | compile | `DriverId` exists for JH7110-class storage path; no promoted execution lane yet. |
| `GpioBcm2837` | present | missing | missing | compile | Real BCM cell exists; physical graduation still blocked. |
| `GpioBcm2711` | present | missing | missing | compile | `DriverId` exists; no dedicated Cell crate yet. |
| `Uart16550PortIo` | missing | promoted | present | qemu | q35/x86_64 COM1 baseline. |
| `IoApic` | missing | promoted | present | qemu | q35/x86_64 interrupt baseline. |
| `Hpet` | missing | promoted | present | qemu | q35/x86_64 timer baseline. |
| `NvmePci` | missing | present | present | qemu, compile | q35 descriptor + Driver Cell exist; still below “real storage first” promotion target. |
| `EthernetE1000` | missing | present | present | qemu, compile | q35 descriptor + Driver Cell exist; still below “real NIC after IOMMU” gate. |

## `cells/drivers/*` crate matrix

| Crate | G1 | G2 | G3 | Evidence | Notes |
|---|---|---|---|---|---|
| `adc-sim` | prototype | missing | missing | synthetic | Spec 13 marks simulation only. |
| `can-loopback` | prototype | missing | missing | synthetic | Loopback only; no hardware controller. |
| `disk` | present | present | prototype | qemu | Generic block helper/fallback path. |
| `e1000` | missing | present | present | qemu, compile | Real q35 lane exists, but not yet promoted past Phase 05 gate. |
| `gpio` | promoted | missing | missing | qemu | PL061/QEMU ARM virt baseline. |
| `gpio-bcm` | present | missing | missing | compile | Real BCM2837 controller crate exists. |
| `gpio-sifive` | present | present | missing | compile | Useful RV64 board lane; not selected as current promoted baseline. |
| `gpu` | present | present | prototype | qemu | Shared framebuffer path; G3 only telemetry/support for now. |
| `i2c-gpio` | prototype | missing | missing | synthetic, qemu | Bit-bang only per Spec 13. |
| `nvme` | missing | present | present | qemu, compile | Exists for q35/G3 host substrate; still pre-promotion. |
| `pwm-gpio` | prototype | missing | missing | synthetic, qemu | Bit-bang only per Spec 13. |
| `serial` | promoted | present | present | qemu, compile | Shared serial substrate spans G1 and G2 console lanes. |
| `spi-gpio` | prototype | missing | missing | synthetic, qemu | Bit-bang only per Spec 13. |
| `virtio-blk` | promoted | prototype | missing | qemu | Promoted for QEMU storage baseline; fallback-only for G2. |
| `virtio-gpu` | present | present | prototype | qemu | QEMU UI/display baseline; not a G3 accelerator lane. |
| `virtio-net` | present | prototype | missing | qemu | QEMU network baseline; not the promoted G2 NIC lane. |
| `wasm` | missing | missing | missing | compile | Not part of driver-port scope; exclude from stage promotion. |

## Stage gates frozen by Phase 01

| Stage | Gate |
|---|---|
| G1 | Promote only after real-controller proof; Spec 13 bit-bang/sim crates stay `prototype`. |
| G2 | Keep ordering `PCIe ECAM -> IOMMU -> NVMe -> real NIC`; q35/QEMU proof does not equal physical server proof. |
| G3 | Reuse G2 host substrate only; no `ViAccelerator` or vendor NPU driver promotion before hardware. |

## Evidence anchors

- `boards/src/descriptor.rs:20-44` defines the complete `DriverId` set.
- `boards/raspberry-pi/3-model-b/board.rs:6-12` selects `UartBcmMini`, BCM IRQ/timer, `SdhciArasan`, `GpioBcm2837`.
- `docs/baremetal/load-cellos.md:107-114` records RPi3 physical mini-UART RX/AUX IRQ/scheduler/100-command burst PASS.
- `docs/baremetal/load-cellos.md:114-119` records AUX legacy IRQ29 enable/drain as the proven RPi3 mini-UART RX interrupt route.
- `docs/project-changelog.md:230-234` records RPi3 physical MMC/SDHCI boot, sector discovery, FAT16/FAT32, `/mnt/sd`, and `/bin` PASS.
- `docs/project-changelog.md:266-271` records the same AUX legacy IRQ29 real-board RX fix and burst validation.
- `boards/qemu/q35-x86_64/board.rs:6-13` selects COM1/IOAPIC/HPET/ECAM/NVMe/e1000 for the G2 x86 lane.
- `docs/specs/13-peripherals.md:4,160-176` freezes bit-bang/sim peripheral crates as partial, not hardware-complete.
- `docs/specs/04-hardware.md:80-110` freezes the G2 storage/network ordering and blocks NIC promotion before IOMMU.
