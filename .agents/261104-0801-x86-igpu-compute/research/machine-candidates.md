# G0 machine candidates (2026-11-04)

**Owner decision (2026-11-04):** a mini PC on Panther Lake, Wildcat Lake or Lunar Lake with an I219/I226 NIC. This file narrows that class to models. The exact model and the purchase are still owner decisions.

## Facts that decide the choice

- **GPU size matters more than the CPU for G4.** Panther Lake `X` parts (e.g. Core Ultra X7 358H) carry the large Xe3 iGPU. Core Ultra 5 325 has 4 Xe cores (ASUS spec). Wildcat Lake Core 5 320 has 2 Xe3 cores and Core 3 304 has 1 (ASRock spec). `[inference: 358H = 12 Xe3 cores; confirm from Intel ARK before purchase]`
- **The iGPU uses system memory.** Capacity and bandwidth come from the RAM: SO-DIMM DDR5-7200 up to 128 GB on PTL-H boxes. Lunar Lake memory is on-package and fixed at 16/32 GB, which limits model size.
- **NIC.** None of the candidates has I219. All have I226, which needs x86 breadth phase 15 (`igc`). Realtek RTL8125 (breadth phase 16) is not used.
- **HCL R1 (COM1)** is mandatory until breadth phase 10 relaxes it to "framebuffer + USB keyboard". Most candidates have no COM port, so they wait for phase 10 or use a COM add-in.
- **HCL R4:** where Intel VMD RAID is offered, it must be disabled.

## Candidates

| Model | SoC / iGPU | RAM | NIC | COM1 | Notes | Source |
|---|---|---|---|---|---|---|
| **ASRock Industrial NUC Ultra 300 BOX-358H** | PTL-H, Core Ultra X7 358H (large Xe3) | 2× SO-DIMM DDR5-7200, ≤128 GB, in-band ECC | I226-LM (vPro) + I226-V | no | Best GPU and memory headroom; VMD RAID present (disable) | qscompute.com spec page |
| ASUS NUC 16 Pro RNUC16GDKX76C00U | PTL, Core Ultra X7 358H | 32 GB LPDDR5X, soldered | 2× I226-V | no | Large iGPU but fixed 32 GB; listed at $1,799.95 | mitxpc.com |
| ASUS NUC 16 Pro RNUC16GDKU56000U | PTL, Core Ultra 5 325, 4 Xe cores | 2× DDR5-6400 SO-DIMM, ≤64 GB | 2× I226-V | no | Cheapest PTL ($599.95); small iGPU | mitxpc.com |
| Avalue EPC-PTL | PTL Core Ultra 7 365/355, 5 335/322 | 1× SO-DIMM, ≤48 GB | 2× I226-IT | **2× DB-9 RS-232** | Meets R1 now; small iGPU; fanless | Avalue datasheet 2026-09-17 |
| ASRock Industrial NUC BOX-320 | WCL Core 5 320, 2 Xe3 cores | 1× SO-DIMM DDR5-6400, ≤64 GB | I226-V + RTL8125BG | header on board models only | Weak iGPU | qscompute.com spec page |
| ASUS NUC 14 Pro AI | LNL Core Ultra 200V | on-package 16/32 GB | 2.5 GbE (chip not confirmed) | no | Fixed memory; NIC chip unverified | asus.com |

## Recommendation

**ASRock Industrial NUC Ultra 300 BOX-358H** with 64–128 GB DDR5-7200:

- largest iGPU in the class;
- upgradable memory for models;
- two I226 ports (admin and test network);
- in-band ECC.

Its cost is HCL R1: there is no COM port. Bring-up waits for breadth phase 10 (framebuffer console) or uses a COM add-in mapped at `0x3F8`. If R1 must be met with no extra work, choose the Avalue EPC-PTL instead and accept the smaller iGPU.

Before purchase, confirm from the vendor or a Linux capture:

- the iGPU PCI ID;
- that VT-x/EPT and VT-d can be enabled;
- that Secure Boot can be disabled;
- that VMD can be disabled;
- that the DMAR has a graphics DRHD.
