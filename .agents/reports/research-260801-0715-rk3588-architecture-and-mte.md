# Research: RK3588 architecture level and MTE support
**Mode:** eval · **Depth:** standard · **Date:** 2026-08-01

## Verdict

RK3588's CPU complex is Cortex-A76 plus Cortex-A55, both Armv8.2-A cores, and neither
core implements FEAT_MTE; therefore RK3588 does not support MTE and Spec 19's
page-table-isolation premise is factually correct.

## Evidence

| Question | Authoritative evidence | Finding |
|---|---|---|
| Which CPU cores are in RK3588? | Rockchip's RK3588 product page states "quad-core Cortex-A76 + quad-core Cortex-A55." | RK3588 uses Arm Cortex-A76/A55. |
| What architecture does Cortex-A76 implement? | Arm Cortex-A76 TRM, **Features**, states "Full implementation of the Armv8.2-A A64, A32, and T32 instruction sets." | Cortex-A76 is Armv8.2-A. |
| What architecture does Cortex-A55 implement? | Arm Cortex-A55 TRM, **Features**, states the same full Armv8.2-A implementation. | Cortex-A55 is Armv8.2-A. |
| Does Cortex-A76 expose MTE? | Its TRM defines `ID_AA64PFR1_EL1[63:8]` as `RES0`; Arm's architecture-register reference defines `MTE` at `[11:8]`, where zero means not implemented. | No MTE. |
| Does Cortex-A55 expose MTE? | Its TRM also defines `ID_AA64PFR1_EL1[63:8]` as `RES0`. | No MTE. |
| Where was MTE introduced? | Arm's official document is titled **Armv8.5-A Memory Tagging Extension White Paper** and says it introduces the Armv8.5-A MTE. | FEAT_MTE is an Armv8.5-A feature, not an Armv8.2-A RK3588 feature. |

## Claim Status

- **VERIFIED:** RK3588 uses Cortex-A76 and Cortex-A55.
- **VERIFIED:** both cores fully implement Armv8.2-A instruction sets.
- **VERIFIED:** both core TRMs reserve the `ID_AA64PFR1_EL1` MTE field as zero.
- **REFUTED:** "MTE supported on RK3588" and "RK3588 confirmed supported".
- **VERIFIED:** generic Cellos MTE code can target QEMU or future Armv8.5+ hardware,
  but it cannot provide RK3588 deployment hardening.

## Active Refutation

A targeted search for an RK3588 `ID_AA64PFR1_EL1.MTE` value, Linux MTE feature
advertisement, or authoritative RK3588 MTE statement found no credible counterexample.
The core-specific Arm feature-register definitions are decisive: on both constituent
cores, all bits `[63:8]` are reserved zero, which includes the architectural MTE field.

## Consequence for Cellos

1. `docs/system-architecture.md` is wrong where it marks MTE available on RK3588.
2. Roadmap, security-model, research, and historical changelog wording must distinguish
   generic Armv8.5+/QEMU implementation from RK3588 hardware availability.
3. Spec 19 is correct: isolation that must run on RK3588 must use page tables; MTE may
   remain only an opportunistic future-hardware hardening layer.

## Sources

1. Rockchip, **RK3588 product page**: <https://www.rock-chips.com/a/en/products/RK35_Series/2022/0926/1660.html>
2. Arm, **Cortex-A76 Core TRM — Features**: <https://support.arm.com/documentation/100798/0401/Introduction-to-the-Cortex-A76-core/Features>
3. Arm, **Cortex-A76 Core TRM — ID_AA64PFR1_EL1**: <https://support.arm.com/documentation/100798/0401/AArch64-System-registers/ID-AA64PFR1-EL1--AArch64-Processor-Feature-Register-1--EL1>
4. Arm, **Cortex-A55 Core TRM — Features**: <https://support.arm.com/documentation/100442/0200/Functional-description/Introduction-to-the-Cortex-A55-core/Features>
5. Arm, **Cortex-A55 Core TRM — ID_AA64PFR1_EL1**: <https://support.arm.com/documentation/100442/0200/Register-descriptions/AArch64-system-registers/ID-AA64PFR1-EL1--AArch64-Processor-Feature-Register-1--EL1>
6. Arm, **Armv8.5-A Memory Tagging Extension White Paper**: <https://support.arm.com/documentation/102925/0100>
7. Arm, **A-profile Architecture Registers — ID_AA64PFR1_EL1**: <https://support.arm.com/documentation/ddi0601/2025-06/AArch64-Registers/ID-AA64PFR1-EL1--AArch64-Processor-Feature-Register-1>

## Unresolved Questions

None for D9. A future board should still probe `ID_AA64PFR1_EL1.MTE` at runtime rather
than infer support only from a marketing architecture label.
