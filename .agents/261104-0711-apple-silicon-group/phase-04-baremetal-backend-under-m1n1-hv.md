---
phase: 4
title: "Bare-metal Apple backend under the m1n1 hypervisor"
status: pending
priority: P2
effort: "10d + hardware"
dependencies: ["1", "2a", "2b", "2c"]
tier: thinking
checkpoint: true
---

# Phase 04: Bare-metal backend under the m1n1 hypervisor (AS-4)

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [Research §4](research/asahi-linux-reference.md); [provenance](provenance.md)
- References (class P/D): OpenBSD `aplintc.c`, `aplcpu.c`, `aplpmgr.c`, `apldog.c`; Linux `t8103.dtsi` (MIT option)

## Overview

New backend `apple_t8103` for the Mac mini M1, run as a guest of the m1n1 hypervisor (S2-hv): AIC1 interrupts, FIQ timer and fast IPIs, the `apple,s5l-uart` console (seen as the vUART), spin-table SMP on all 8 cores, PMGR power domains, watchdog reset, and the catalog profile bound from the FDT root `compatible`.

## Key Insights

- The m1n1 hypervisor redirects the guest UART to USB, so a Samsung-style UART driver is also the vUART driver.
- AIC1 handles IRQs; the timer and IPIs are FIQs whose source is read from Apple-specific registers. The behaviour spec comes from OpenBSD `aplintc.c` (ISC).
- Running under the hypervisor first gives MMIO traces for every access, which makes clean-room debugging possible without reading Linux drivers.

## Requirements

- `ArmIrqController` impl for AIC1 (mask/unmask/ack, IRQ number from the FDT), FIQ sources (timer, IPI, PMU).
- `SecondaryStart::SpinTable`; all 8 cores online; P/E cluster in the hart table.
- `apple-t8103` profile: `DmaIsolation::Required`, driver allow-list empty until phase 05; bound by `compatible = "apple,j274", "apple,t8103"`.
- Machine pin from FDT root `compatible`, `serial-number` and system-firmware version.
- Reset via watchdog.

## Related Code Files

- Create: `hal/arch/arm/src/aarch64/backend/apple_t8103.rs` (+ `aic.rs`, `uart_s5l.rs`), `hal/soc/apple/**` (t8103 facts from the captured FDT), `boards/apple/macmini-m1/**`
- Modify: `backend/mod.rs`, `kernel/src/board.rs`, aarch64 build matrix and CI (compile-only lane)

## Implementation Steps

1. Behaviour specs for AIC1 and the s5l UART from class-P/D sources; ledger rows.
2. UART + early console; boot to banner under the hypervisor.
3. AIC1 + FIQ timer tick; scheduler running.
4. Spin-table SMP + fast IPI; TLB shootdown.
5. Profile binding, machine pin, watchdog reset.

## Todo List

- [ ] Specs + ledger
- [ ] UART/banner
- [ ] AIC + timer
- [ ] SMP + IPI
- [ ] Profile, pin, reset

## Success Criteria

- [ ] S2-hv: Cellos boots to the cellbox shell over vUART on the exact bench machine, 8 harts online, timer and IPI by FIQ, `reboot` resets through the watchdog. Logs and the hypervisor MMIO trace are stored with hashes.
- [ ] Compile-only CI lane for `apple_t8103`; group-1 and `virt` lanes unchanged.

## Risk Assessment

- The hypervisor may hide timing-sensitive bugs; native boot (phase 07) repeats every check.

## Security Considerations

No DMA-capable device is enabled in this phase; the allow-list stays empty until DART exists.

## Next Steps

Phase 05.

## Deviation Log

None.
