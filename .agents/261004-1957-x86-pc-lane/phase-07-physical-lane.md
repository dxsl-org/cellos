---
phase: 7
title: "Physical lane + HCL rows"
status: pending
priority: P2
effort: M per machine (procurement + capture + review)
dependencies: [2, 3, 4, 5]  # sub-phases 02b, 03, 04b, and 05; hardware bought only after these are green
tier: thinking
ceiling: physical (development evidence, unqualified)
---

# Phase 07 — Physical lane + HCL rows

## Target

The hardware half of **X86-PC-0**. Every other phase ends at the `qemu` ceiling;
this phase is where a result can first be called physical — and only for the
exact machines captured.

## Change

- Acquire/qualify **one exact Intel and one exact AMD** machine (the pair is the
  minimum that keeps both Tier-3 backends honest). Selection must follow the
  acquisition checklist in `docs/hardware-compatibility-list.md`.
- For each machine, capture and record: vendor/model, chipset/PCH, CPU family,
  BIOS version **and date**, COM1 verified at `0x3F8`/IRQ 4 (or BMC SOL
  equivalent), HPET present, SATA controller in **AHCI mode**, Secure Boot
  disable-able, VT-x / VT-d exposed, NIC family, storage and USB controllers.
- Produce per-machine UART logs for: boot to shell, storage mount + write/read
  across a reboot, NIC DHCP Tx/Rx, and (where VT-d exists) DMA isolation active
  before traffic. Record every item that **failed** as prominently as the
  passes.
- Capture **RS485 direction timing** on any machine that exposes RS485 (phase 06
  leaves it unclaimed because QEMU has no DE/RE model): the DE/RE assertion path,
  the turnaround timing observed, and the test rig used. The capture lives here so
  the plan's RS232-485 deliverable has an owner; promoting it to a *claim* still
  needs its own phase, and until then the capability is recorded as `optional,
  captured, unclaimed`.
- Write the HCL rows with the evidence level the capture supports
  (`development physical evidence, unqualified` unless a separate governance
  decision promotes it). No row may cite QEMU evidence.

## Gate

- **Hardware only.** There is no QEMU substitute for this phase; a QEMU run
  neither qualifies nor substitutes for a machine.
- No production-qualification, fleet, or security claim. The production root and
  secure/measured boot remain separate external gates.

## Acceptance

- Two HCL rows exist with the fields above complete, each bound to one exact
  machine, each citing logs under the phase evidence directory.
- Every mandatory requirement (HCL R1–R7) has a capture for the machine. A
  machine that fails any of them is recorded in the HCL refusal register instead
  of a row; optional-capability gaps go in the row's `Notes` column and are
  never left blank or implied.
- The roadmap entries X86-PC-0 (hardware half) and the enablers' `physical`
  status are updated to whatever the captured evidence actually supports.

## Out of scope

- Intel VMX / Tier 3 on Intel boards (P09, its own hardware/KVM lane).
- AMD-Vi/DMAR on AMD boards (phase 05 covers Intel DMAR only).
- RS485 direction timing, hardware watchdog, SIO GPIO — record if the machine
  exposes them, but claiming them needs its own phase.
- Buying more than the Intel/AMD pair before the first two rows exist.

## Risk assessment

- **Undo:** a hardware row can be demoted or withdrawn by editing the HCL file;
  the log remains as evidence of what was observed.
- **Not undoable:** a claim published before the capture — the reason this phase
  records failures first and promotes nothing on its own.
- **Procurement is not evidence:** a purchase decision made from the checklist
  adds no HCL row until the machine is captured.
