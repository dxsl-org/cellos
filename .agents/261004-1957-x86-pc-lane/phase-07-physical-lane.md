---
phase: 7
title: "Fixed Intel C2C physical lane + exact-machine HCL rows"
status: pending
priority: P2
effort: M per machine (procurement + capture + review)
dependencies: [2, 3, 4, 5]  # 02b/03/04b/05 QEMU gates plus explicit procurement authorization
tier: thinking
ceiling: physical (development evidence, unqualified)
---

# Phase 07 — Fixed Intel C2C physical lane + exact-machine HCL rows

## Target

The hardware half of **X86-PC-0**. Every other phase ends at the `qemu` ceiling;
this phase is where a result can first be called physical — and only for the
exact machines captured.

As of 2026-10-08, [ADR-0022](../../docs/decisions/0022-intel-x86-64-c2c-only-direction.md)
replaces the two-vendor goal with one fixed headless Intel x86-64 C2C
configuration. This phase is still pending and procurement-gated; strategic
approval and completed QEMU prerequisites do not authorize a purchase.

## Change

- Select **one exact Intel machine** against the HCL. Acquire it only after
  explicit procurement approval; capture it and obtain first-machine
  qualification before considering a separately authorized second Intel node.
  Identical configuration is preferred to reduce work, not mandatory; a different
  model needs independent qualification and bounded driver scope. No AMD
  qualification row is an objective.
- For each machine, capture and record: vendor/model, chipset/PCH, CPU family,
  BIOS version **and date**, COM1 verified at `0x3F8`/IRQ 4 (or BMC SOL
  equivalent), HPET present, AHCI-mode or NVMe storage, Secure Boot
  disable-able, Intel VT-x/EPT and VT-d enabled, usable DMAR, exact NIC PCI ID,
  storage and USB controllers. Current `igb` physical candidate admission is
  flash-backed i210 `8086:1533`, not a generic i210/i211 family claim;
  `8086:10c9` is QEMU evidence, not physical qualification.
- Produce per-machine UART logs for boot to shell, storage mount + write/read
  across reboot, NIC DHCP Tx/Rx, and required VT-d isolation active before
  device DMA. Record failures as prominently as passes. VT-x/EPT availability
  is a prerequisite, not evidence of implemented Intel guest execution.
- Optional RS485 inventory and any already available timing evidence remain
  recorded but unclaimed. New DE/RE capture or serial expansion requires a
  named direct Intel C2C dependency; it is not an industrial side program.
- Write the HCL rows with the evidence level the capture supports
  (`development physical evidence, unqualified` unless a separate governance
  decision promotes it). No row may cite QEMU evidence.

## Gate

- **Hardware only.** There is no QEMU substitute for this phase; a QEMU run
  neither qualifies nor substitutes for a machine.
- **Procurement gate.** QEMU prerequisites 02b/03/04b/05 must remain satisfied
  and an explicit purchase decision must name the first configuration.
  First-machine qualification and a separate purchase decision gate the
  second Intel node, whether matching or different. No further machines or new
  hardware platforms are authorized by this phase.
- No production-qualification, fleet, or security claim. The production root and
  secure/measured boot remain separate external gates.

## Acceptance

- First-machine milestone: one exact Intel HCL row with complete captures and
  a separately recorded qualification decision before second-node acquisition.
  Two-node milestone: a second independently qualified Intel row, bound
  to that unit's own logs; no transfer of first-unit evidence by model name.
- Every mandatory requirement (HCL R1–R9) has its specified capture for the machine. A
  machine that fails any of them is recorded in the HCL refusal register instead
  of a row; optional-capability gaps go in the row's `Notes` column and are
  never left blank or implied.
- The roadmap entries X86-PC-0 (hardware half) and the enablers' `physical`
  status are updated to whatever the captured evidence actually supports.

## Out of scope

- Intel VMX implementation and the explicit C2C Tier-3 guest adapter: separate
  prerequisite owners; no VM or C2C readiness is inferred from this HCL capture.
- AMD qualification/AMD-Vi and new ARM/RISC-V ports: paused.
- RS485 claims, watchdog, SIO GPIO or other peripheral programs without a
  direct C2C dependency: paused; optional inventory does not authorize work.
- Expanding beyond the separately gated two-node Intel setup or adding unrelated drivers.

## Risk assessment

- **Undo:** a hardware row can be demoted or withdrawn by editing the HCL file;
  the log remains as evidence of what was observed.
- **Not undoable:** a claim published before the capture — the reason this phase
  records failures first and promotes nothing on its own.
- **Procurement is not evidence:** a purchase decision made from the checklist
  adds no HCL row until the machine is captured.
