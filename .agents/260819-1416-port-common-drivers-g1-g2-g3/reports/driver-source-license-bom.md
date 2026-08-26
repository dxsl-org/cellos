# Driver Source License BOM

Rule: `adaptable` = permissive code may be ported with retained notices; `concept-only` = use algorithms/register sequencing only, rewrite clean-room; `blocked` = do not ingest until file-level license is rechecked or external terms are available.

| Source | License evidence | SPDX / mode | Notice / reuse rule | Best-fit Cellos targets |
|---|---|---|---|---|
| `D:\Cellos\.references\Tock` | `Cargo.toml:1-3`, `LICENSE-APACHE`, `LICENSE-MIT` | `Apache-2.0 OR MIT` / adaptable | Keep Apache or MIT notice text with derived files; compatible for Rust trait/controller patterns. | G1 GPIO/I2C/SPI/PWM concepts; x86 typed PCI helpers. |
| `D:\Cellos\.references\Theseus` | `LICENSE-MIT:1-9` | `MIT` / adaptable | Retain MIT notice in copied/adapted files; reconcile with Cellos Cell safety policy. | G2 PCI/e1000/IOMMU patterns; ARM/x86 timer/serial structure. |
| `D:\Cellos\.references\Redox` | `README.md:5`, `Cargo.toml:1-4`, `LICENSE:1-21` | `MIT` / blocked-for-local-copy | Local checkout is the `redox_cookbook` build system, not the standalone driver repos; do not treat it as local reusable driver code. | None from this checkout; upstream Redox driver repos must be acquired separately. |
| `D:\Cellos\.references\nanvix` | `Cargo.toml:1`, `LICENSE.txt:1-18` | `MIT` / adaptable | Retain MIT notice; useful as architecture/runtime reference, not primary driver source. | Supplemental DMA/runtime patterns only. |
| `D:\Cellos\.references\seL4` | `LICENSE.md:9-24` | mixed repo; default `GPL-2.0-only` kernel / `BSD-2-Clause` user headers / concept-only | Treat kernel driver code as GPL-tainted unless the exact file header proves permissive; no direct kernel-driver copy into Cellos. | BCM mini UART, PL011, SMMU, timer register sequencing as concepts only. |

## Reference-to-target map

| Cellos target | Candidate source | Mode | Why |
|---|---|---|---|
| G1 real I2C/SPI/PWM/GPIO controllers | Tock capsules/chips | adaptable | Best match for embedded Rust controller structure and ownership discipline. |
| G1 BCM mini-UART / timer bring-up details | seL4 BCM/PL011/timer files | concept-only | Good register sequencing, but license mix blocks direct import by default. |
| G2 PCIe ECAM host walker | Tock x86 PCI helpers + separately acquired Redox drivers | adaptable/blocked | Tock is locally usable; Redox driver logic is blocked here because the local checkout is only the cookbook/build system. |
| G2 e1000 / VT-d | Theseus | adaptable | MIT, Rust, and closer to current Cellos q35/x86 lane. |
| G2 RV IOMMU / ARM SMMU ideas | seL4 | concept-only | Architecture concepts only unless exact file SPDX is revalidated. |
| G3 accelerator envelope | none in tree | blocked | No vendor SDK/license/hardware evidence present; Phase 01 forbids speculative import. |

## Block list

| Item | Status | Reason |
|---|---|---|
| seL4 kernel driver code (default) | blocked-for-copy | Mixed repo, kernel side generally GPLv2. |
| Vendor NPU SDKs / blobs | blocked | Not present in `D:\Cellos\.references`; no license review or hardware yet. |
| Local `D:\Cellos\.references\Redox` as driver source | blocked | It is the build-system cookbook checkout, not the driver repositories. |
| Mellanox `mlx5` / large Linux-class NIC ports | blocked by scope | Explicitly out of scope in hardware spec. |
| USB xHCI / WiFi / Bluetooth / audio | blocked by scope | Phase 01 freezes them out. |

## Notice checklist for future ports

| Mode | Required action |
|---|---|
| adaptable | Preserve upstream copyright + license notice in the adapted file or adjacent `THIRD_PARTY_NOTICES` entry. |
| concept-only | Record source path in phase deviation/evidence log, then rewrite from Cellos interfaces without copying code/comments. |
| blocked | Stop before reading deeply for copy intent; require a new license ruling first. |

## Evidence anchors

- Tock dual-license: `D:\Cellos\.references\Tock\Cargo.toml:1-3`.
- Theseus MIT: `D:\Cellos\.references\Theseus\LICENSE-MIT:1-9`.
- Redox local-tree identity: `D:\Cellos\.references\Redox\README.md:5`, `D:\Cellos\.references\Redox\Cargo.toml:1-4`, `D:\Cellos\.references\Redox\LICENSE:1-21`.
- Nanvix MIT: `D:\Cellos\.references\nanvix\LICENSE.txt:1-18`.
- seL4 mixed licensing and GPL syscall note: `D:\Cellos\.references\seL4\LICENSE.md:9-24`.
