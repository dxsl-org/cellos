# Reference OS Driver Map

Status: planner-synthesized, read-only. User named `D:\Cellos.references`; observed correction is `D:\Cellos\.references`.

## Inventory and licenses

- Redox local tree license is MIT: `D:\Cellos\.references\Redox\LICENSE:3`. Local tree mostly contains recipes/OS tree, not the standalone Redox drivers repo named in old spec.
- Tock local tree is dual Apache-2.0/MIT: `D:\Cellos\.references\Tock\LICENSE-APACHE:2`, `D:\Cellos\.references\Tock\LICENSE-MIT:1`.
- Theseus local tree is MIT: `D:\Cellos\.references\Theseus\LICENSE-MIT:1`.
- Nanvix local tree is MIT: `D:\Cellos\.references\nanvix\LICENSE.txt:1`.
- seL4 uses per-file SPDX; kernel code generally GPLv2 and user-level code BSD-2-Clause, so default to concept-only unless file SPDX is checked: `D:\Cellos\.references\seL4\LICENSE.md:9`.

## Useful reference areas

- G1 peripheral shape: Tock capsules for GPIO/I2C/SPI/PWM and virtualizers, especially buffer/ownership/error-state design. Use these to design real controller Driver Cells, not to promote Cellos bit-bang/sim baselines as hardware-complete: `D:\Cellos\.references\Tock\capsules\core\src\gpio.rs:70`, `D:\Cellos\.references\Tock\capsules\core\src\i2c_master.rs:40`, `D:\Cellos\.references\Tock\capsules\core\src\spi_controller.rs:58`.
- G1 BCM/PL011/generic timer bring-up: seL4 serial/timer files are useful for register sequencing only unless SPDX permits direct reuse: `D:\Cellos\.references\seL4\src\drivers\serial\bcm2835-aux-uart.c:14`, `D:\Cellos\.references\seL4\src\drivers\serial\pl011.c:13`, `D:\Cellos\.references\seL4\src\drivers\timer\generic_timer.c:9`.
- G2 PCI enumeration: Theseus PCI bus/device iteration and Tock pci-x86 typed device access are useful patterns: `D:\Cellos\.references\Theseus\kernel\pci\src\lib.rs:209`, `D:\Cellos\.references\Theseus\kernel\pci\src\lib.rs:313`, `D:\Cellos\.references\Tock\chips\pci-x86\src\device.rs:119`.
- G2 e1000 and VT-d: Theseus has MIT e1000 and Intel IOMMU references; map first to q35 x86_64, while Pioneer waits for SG2042 substrate/storage/network facts: `D:\Cellos\.references\Theseus\kernel\e1000\src\lib.rs:126`, `D:\Cellos\.references\Theseus\kernel\e1000\src\lib.rs:153`, `D:\Cellos\.references\Theseus\kernel\iommu\src\lib.rs:1`.
- G2 ARM SMMU concepts: seL4 SMMUv2 is concept-only by default: `D:\Cellos\.references\seL4\src\drivers\smmu\smmuv2.c:8`.

## Reuse rule

Use permissive code only after retaining notices and reconciling with Cellos `#![forbid(unsafe_code)]` Cell policy. Use GPL/proprietary/unknown license material as clean-room concepts only. Vendor NPU SDK/API code is out of scope until hardware and license terms are present.
