---
title: "x86_64 PC lane — controller-family bring-up, QEMU first"
description: "Close roadmap X86-PC-0..7 as controller-family slices (descriptor+HCL, AHCI, xHCI, igb, ACPI DMAR, multi-port COM/RS485, physical lane). Each phase has a QEMU-first gate; Intel VMX stays in P09 and x86 AI SIMD stays in the cpu-engine plan."
status: in-progress
priority: P2
effort: 7 phases; per-family driver estimates follow spec 04 §7 scale (NVMe ~3-5K LOC, real NIC ~5-8K LOC)
branch: main
created: 2026-10-04
tags: [x86, pc, ahci, xhci, igb, dmar, iommu, hcl, qemu-first, g2]
---

# x86_64 PC lane — controller-family bring-up (QEMU first)

## Goal

Make Cellos usable and qualifiable on ordinary x86_64 PCs/servers, which is the
G2 "organization server / office PC" cohort (ADR-0014,
`.agents/260905-1139-sas-lbi-outcome-closure/organization-deployment-profiles.md`).
The roadmap already records the missing prerequisites as **X86-PC-0..7** in
`docs/roadmap/hardware-tracks.md` and as risk `CELLOS-X86-PC-001`; this plan is
the owner for X86-PC-0..5 plus the physical lane. It does **not** own Intel VMX
(that is `.agents/260711-1917-tier3b-x86-vtx/phase-09-vtx-backend-apic.md`) or
x86 CPU-inference kernels (`.agents/260914-cpu-engine-optimization/`).

Phases are split by **controller family**, not by machine model, because that is
the unit of reuse: one AHCI driver, one xHCI driver and one `igb` driver each
cover a whole generation of industrial and office PCs from either vendor. Per
machine work is reduced to a board descriptor plus an HCL row.

## Why QEMU first

Every phase must first pass a QEMU gate before any hardware is bought or
claimed. This is the existing repo discipline (`docs/roadmap/hardware-tracks.md`,
`docs/project-roadmap.md`): QEMU and compile results are regression evidence and
never qualify a board. QEMU-first is what makes the phases cheap to iterate:

| Phase | QEMU device model | Fidelity caveat (must be stated in the phase evidence) |
|---|---|---|
| 01 descriptor | `q35` + SeaBIOS/Limine, COM1 ISA serial, ACPI MADT/HPET/MCFG | none material for facts |
| 02 AHCI | q35 built-in ICH9 AHCI + `ide-hd` backed by a raw image | real PCH AHCI is register-compatible; port-count/remap differ |
| 03 xHCI | `-device qemu-xhci` + `-device usb-kbd` | QEMU model is NEC uPD720200-class, not the board's controller |
| 04 igb | `-device igb,netdev=net0` | QEMU `igb` is 82576-class, **not bit-exact i210/i211** (PHY, NVM, some registers differ) |
| 05 DMAR | `-device intel-iommu` (must precede endpoint devices) — already used by `tests/integration/tests/nic-x86.rs` | vIOMMU tables are QEMU-generated, not firmware DMAR |
| 06 multi-port COM | several `isa-serial` ports | RS485 DE/RE timing has **no** QEMU model and cannot be gated here |
| 07 physical lane | — | hardware only; no QEMU substitute is acceptable |

`scripts/qemu-x86_64-test.sh` currently accepts only
`X86_NIC_MODEL=e1000|e1000e`; each phase extends the runner and the
`QemuRunner` x86 constructors (`tests/integration/src/lib.rs`, today
`boot_x86_bios`, `boot_x86_bios_with_nvme`, `boot_x86_bios_with_nic`,
`boot_x86_bios_with_vtd`) rather than inventing a second harness.

## Phases

| # | Phase | Family | QEMU-first gate | Status |
|---|---|---|---|---|
| 01 | [x86_64-pc board descriptor + HCL model](phase-01-descriptor-and-hcl.md) | board facts | ISO boots on `q35` with the new descriptor; `x86_64-boot` stays 7/7 | **completed** (2026-10-04, `qemu` ceiling) |
| 02a | [AHCI part A — PCI binding, HBA init, IDENTIFY](phase-02a-ahci-hba-init.md) | storage | `ahci-x86` binds controller, HBA/port up, IDENTIFY ok; `x86_64-boot` 7/7 | **completed** (2026-10-05, `qemu` ceiling) |
| 02b | [AHCI part B — data path, block registration, persistence](phase-02b-ahci-block-persistence.md) | storage | `ahci-x86` two-boot persistence on the same raw image | **completed** (2026-10-05, `qemu` ceiling) |
| 03 | [xHCI + HID family](phase-03-xhci-hid.md) | USB | new `xhci-x86` suite: controller init, HID boot keyboard report reaches the input path | pending |
| 04a | [igb part A — identity, registration, Tx/Rx](phase-04a-igb-identity-txrx.md) | network | `igb-x86`: `8086:10c9` no longer rejected, registration, first Tx/Rx | pending |
| 04b | [igb part B — DHCP data plane and VT-d variant](phase-04b-igb-dhcp-vtd.md) | network | `igb-x86`: DHCP ordinary + VT-d (isolation active before DMA) | pending |
| 05 | [ACPI DMAR discovery → real IOMMU](phase-05-acpi-dmar.md) | IOMMU | DMAR-less boot stays fail-closed; `intel-iommu` boot programs per-device domains; q35 hardcode removed | pending |
| 06 | [Multi-port COM / RS232-485](phase-06-multiport-com-rs485.md) | serial | COM2..COMn enumerate and echo in QEMU; RS485 explicitly not claimed | pending |
| 07 | [Physical lane + HCL rows](phase-07-physical-lane.md) | qualification | none (hardware-gated) | pending |

## Dependency graph

```
01 ──► 02a ──► 02b ─┐
01 ──► 03           │
01 ──► 04a ──► 04b ─┼──► 07
01 ──► 06           │
02b  ──► 05 ────────┘        (05 also needs 04a/04b as the DMA client it gates)
```

## Sequencing (validated 2026-10-04)

- **Sequential, one family at a time.** The user chose sequential execution over
  worktree parallelism: each new driver brings its own QEMU lane, and a red lane
  must be attributable to one change. Do not start 02b before 02a is green, or
  04b before 04a.
- **Hardware is bought only after 02b / 03 / 04b / 05 are green on QEMU** — the
  HCL checklist is ready (`docs/hardware-compatibility-list.md`), and the
  purchase decision follows working drivers instead of spec sheets. Phase 07
  therefore starts last and is the only phase with no QEMU gate.
- **e1000e/I219 is a recorded follow-up decision**, not a dropped requirement:
  revisit it at 04b exit, using the HCL and the actual chip list of the machines
  to be bought.

## Evidence rules

- Every phase publishes evidence at the `qemu` ceiling with the exact device
  arguments and commit; no phase promotes a result to `physical`.
- Phase 07 evidence binds to **one exact machine** (model, chipset, BIOS
  version/date, serial where readable) and is recorded as *development
  physical evidence, unqualified* unless a separate governance decision
  promotes it.
- Fail-closed is the default for unknown device IDs, absent DMAR, absent HPET,
  and locked Secure Boot.
- **CI wiring is part of a phase, not an afterthought.** The x86 lane lives in
  the `qemu-x86_64-boot` job (`.github/workflows/ci.yml:892`), which builds a
  fixed cell subset, signs it, assembles `kernel/src/embedded-x86_64/kernel_fs.img`
  through explicit `tools/mkfat32.py` mappings, then boots the ISO. A phase that
  adds a cell or a lane updates that job in the same change: the `cargo build -p …`
  list, the `sign_cells` argument list, the `mkfat32.py` mapping, and a step that
  runs the new integration test. A lane that only runs locally is reported as
  local, never as a gated lane.
- Removing the q35 IOMMU hardcode (05) must keep the existing QEMU VT-d lanes
  green; a regression there is a stop-the-line event for that phase.

## Out of scope (separate owners)

- **Intel VMX / Tier 3 on Intel boards** — P09 in
  `.agents/260711-1917-tier3b-x86-vtx/`; cannot be QEMU-gated (TCG has no VMX
  model). This plan only ensures the descriptor and driver substrate P09 needs.
- **x86 CPU-inference kernels (AVX2/FMA) and kernel vector state** — X86-PC-7,
  owned by `.agents/260914-cpu-engine-optimization/`; risk `CELLOS-AI-SIMD-004`.
- **Signed/measured boot** — Security track
  (`.agents/260605-2107-full-reliability-track/plan.md:76`); until it ships,
  "firmware must allow disabling Secure Boot" is a hard HCL row, not a code task.
- **WiFi/BT, audio, GPIO/Super-I/O extras, hardware watchdog, SIM/Mini-PCIe** —
  not required by X86-PC-0..5; each would need its own scope decision.
- **AMD SVM backend** — already implemented (QEMU-qualified only); no work here
  beyond the physical lane's AMD row.

## Assumptions (unverified — verify in the phase that needs them)

- `A-01` QEMU 10.2.0's `igb` model registers closely enough to i210/i211 that a
  driver validated against it needs only bounded changes on real silicon. Phase
  04 must record which registers it could **not** validate on the model.
- `A-02` q35's ICH9 AHCI is register-compatible with the PCH AHCI on the target
  industrial boards. Phase 02 must state the un-validated parts (e.g. port
  multiplier, remap, NCQ behaviour).
- `A-03` `qemu-xhci` exercises the same driver paths (command ring, event ring,
  port reset, HID boot protocol) as real xHCI controllers. Phase 03 records
  divergences.
- `A-04` Industrial boards expose a 16550-compatible COM1 at `0x3F8`/IRQ 4.
  Phase 01's HCL checklist turns this into a per-machine verified row.

## Risks

| Risk | Mitigation |
|---|---|
| Phase scope creep into "support every PC" | Phases are family-scoped; expansion requires a new phase and an HCL row each |
| QEMU model divergence makes the QEMU gate falsely reassuring | Every phase records its QEMU-model caveat and the registers/paths **not** validated on the model; phase 07 re-validates on hardware |
| Driver lands before the descriptor/HCL model exists | 01 ships first and defines the HCL file the other phases write into |
| Touching kernel PCIe/IOMMU code destabilizes QEMU VT-d lanes | 05 keeps the q35 base as a fallback path until DMAR discovery passes both QEMU and hardware gates |
| No hardware is ever bought, so the lane stalls at `qemu` | 07 is explicitly the only hardware-gated phase; 02–06 stay useful as regression coverage and as the prerequisite inventory for a purchase decision |
| `bar_mem_*` depends on the kernel's early ECAM scan retaining every BAR | The kernel's own scan retains all BARs (that is why phase 02a passes), but the Platform-Cell registration path stores only BAR0 (`register_device`), so a device registered through that path with an I/O BAR0 and MMIO at BAR5 would report `bar_mem_base = 0` and the AHCI cell would fail closed with a named error. Extending PCI registration to retain per-BAR index/base/size belongs to the Platform-Cell cutover owner, not to this lane; the syscall site carries a note and the AHCI cell fails closed meanwhile |
| Two storage drivers on one machine: registration is single-slot and last-wins, but the *consumer* caches its provider | Both `/bin/nvme` and `/bin/ahci` register successfully and the later TID replaces the earlier one (`driver_cell.rs:84-91`, registry `insert`); however `service-vfs` resolves the block driver through a cached TID, so if it looks up before the second registration, the replacement does not redirect I/O — and the observed registration order varied between runs. Consequence: which drive serves `/mnt/sd` on a machine with two storage devices is scheduling-dependent. This lane's lanes each attach exactly one storage device, and `ahci_and_nvme_both_register_x86` asserts only the registration contract; deterministic storage selection needs owner-side arbitration (Platform/VFS) and is recorded here rather than assumed |

## Validation log

### Validation Decisions (interview, 2026-10-04)

1. **NIC family — `igb` (i210/i211) first.** Confirmed as planned; rationale in
   phase 04a. `e1000e`/I219 is a recorded follow-up decision to revisit at 04b
   exit against the actual chip list of the machines to be bought.
2. **Sub-phase breakdown — 02 and 04 split.** `phase-02-ahci-sata.md` became
   `phase-02a-ahci-hba-init.md` + `phase-02b-ahci-block-persistence.md`;
   `phase-04-igb-nic.md` became `phase-04a-igb-identity-txrx.md` +
   `phase-04b-igb-dhcp-vtd.md`. The superseded files were deleted, not kept
   alongside. 03/05/06 stay single-phase.
3. **Concurrency — sequential**, one family at a time (see Sequencing above).
4. **Hardware — bought only after 02b/03/04b/05 are green on QEMU.** Phase 07
   starts last; the HCL machine table stays empty until then.

No previously planned approach was rejected, so no
`.agents/failure-history.jsonl` entry is warranted.

### Review (2026-10-04, independent reviewer subagent)

Verdict: findings-only, no code defects; criteria 1/2/4/5 verified from the
diff and captured logs; criterion 3 documented but its transcript is not in the
artifact set. Eight consistency findings fixed in this same change:

1. Unqualified physical-PC boot claim removed (`current-focus.md`,
   `product-stages.md`, `open-risk-register.md`) — the witnessed lane is q35;
   the SATA-only industrial PC is now stated as *expected, unqualified*.
2. Stale "no `x86_64-pc` descriptor / no HCL" lines corrected in
   `product-stages.md`, `open-risk-register.md`, `04-hardware.md`,
   `current-focus.md`, `project-changelog.md` → "no machine-specific descriptor
   and no HCL machine row".
3. Phase 07 now depends on 03 (`dependencies: [2, 3, 4, 5]`), matching the
   validated "hardware only after 02b/03/04b/05" rule.
4. RS485 DE/RE timing now has an owner: captured in phase 07 (recorded, not
   claimed) instead of being deferred with no gate.
5. HCL R1 now requires an **exact-resource** OS report plus an exercised-RX
   marker — reaching `Cellos >` alone is insufficient because polled RX works
   with the IRQ gate closed.
6. HCL machine table gained the `R5 ISO boot` and `Notes` columns.
7. HCL admission is one rule: a mandatory R1–R7 failure goes to the refusal
   register, not a row; row-level gaps are optional-only and live in `Notes`.
8. Descriptor/HAL/README/changelog reframed as a **COM1-required compatibility
   contract**, not a universal claim about every PC.

### Phase 02b review (2026-10-05, independent reviewer)

Verdict: the data path is genuinely exercised (a two-boot oracle, not a boot
echo) and part-A behaviour is preserved. Findings were three wire/robustness
defects and two test-honesty gaps, all fixed (phase file, "Review fixes"). The
reviewer also concluded that the repeated `READ DMA EXT rejected: LBA …` lines
are benign probes of absent fixed partition offsets — the capacity bound doing
its job — and that the PRDBC correction supersedes the older part-A evidence
without needing a further retraction. One item is recorded rather than fixed
here: with two storage drivers present, registration is single-slot/last-wins
while the consumer caches its provider TID, so which drive serves I/O is
scheduling-dependent; owner-side arbitration (Platform/VFS) is the fix.

### Phase 02a review (2026-10-05, independent reviewer)

Verdict: part-A scope and the part-A/part-B boundary are respected; no scope
leak (no READ/WRITE, no block registration). The findings were spec-conformance
and robustness gaps that q35 tolerates but real hardware need not — AHCI mode
enabled before register access and reset with AE held (§10.1.2), waiting for the
initial D2H FIS before starting the command engine (§3.3.9), validating the
IDENTIFY payload (PRDBC + word 0), measuring the COMRESET hold against a clock
rather than an iteration count, test-image RAII, and ISO provenance. All fixed
before shipping (phase file, "Review fixes"). The one open point is the recorded
`bar_mem_*` dependency on the kernel's own ECAM scan retaining every BAR — a
Platform-Cell registration gap owned elsewhere, with the cell failing closed.

### Phase 01 evidence (2026-10-04, `qemu` ceiling)

- Descriptor + feature + tests are in: `boards/pc/x86_64-pc/{board.rs,README.md}`,
  `SocId::GenericX86Pc`, `hal_soc_x86::GENERIC_X86_PC`,
  `--features board-x86-pc`, kernel selection + `[x86-gate] board=` identity line,
  and the `x86-pc` entry in `scripts/check-board-configs.sh`.
- Gates: PC-descriptor ISO boots to `Cellos >` with
  `board=x86_64-pc soc-profile=x86_64-pc`
  (`evidence/phase-01-qemu-x86-pc-descriptor.log`); the same tree without the
  feature still reports `board=qemu-q35-x86_64`
  (`evidence/phase-01-qemu-q35-regression.log`); tests 13/13 + 3/3 (baseline
  12/2); `check-hal-boundaries.sh` passes; both kernel `cargo check` variants are
  clean.
- Defect caught by the gate: a diagnostic `puts` placed before
  `uart_16550::configure()` panicked silently (the panic handler has no UART yet).
  Fixed by emitting after `configure()`+`init()`.
- Pre-existing failure, **not** attributable to this phase:
  `scripts/check-board-configs.sh` fails its `rpi4` entry on uncommitted WIP in
  `hal/arch/arm/src/aarch64/monitor.rs:318` (calls `pi_monitor_mmu_init`, gated
  behind `board-rpi3` in `el2.rs:91`; the call site does not exist in `HEAD`).
  The new `x86-pc` entry passes.

### Verification Results
Claims checked: 36 | Verified: 36 | Failed: 0 | Unverified: 0
Tier: Full (7 phases)

- The "no AHCI implementation" claim holds for Cellos source: matches appear only
  inside embedded guest Linux artifacts (`kernel/src/embedded-hv/kernel_fs.img`,
  `kernel/src/embedded-hv-x86/kernel_fs.img`); `kernel/src/task`, `cells/drivers`,
  `hal/soc`, `boards` contain none.
- Device-class claims verified against the pinned QEMU 10.2.0 `-device help` and
  `evidence/qemu-device-attach-smoke.log`: `igb` = 8086:10c9 (82576),
  `qemu-xhci` = 1b36:000d, `ide-hd` on ICH9 AHCI = 8086:2922, `usb-kbd` and
  `isa-serial` (with `iobase`) exist.
- Harness claims verified in source: `QemuRunner` has `boot_x86_bios_with_vtd`,
  a `with_vtd` root-port path, and `boot_with_pointer` (QMP monitor);
  `scripts/qemu-x86_64-test.sh` still accepts only `e1000|e1000e`.
- Pre-Build constraints verified in source: `kernel/src/board.rs` matches only
  `SocId::QemuX86Q35`; `boards/src/descriptor.rs` owns `SocId`/`DriverId`;
  `boards/pc/` does not exist yet; `scripts/check-board-configs.sh` defines the
  board gate.
- Baseline captured before any change: `cargo test -p cellos-boards -p hal-soc-x86
  --target x86_64-unknown-linux-gnu` → 12 passed / 0 failed and 2 passed /
  0 failed.

- 2026-10-04 — **QEMU-first premise smoke** (phase 02/03/04/05 device attach).
  Command: pinned `qemu-system-x86_64` 10.2.0 with `-machine q35
  -cpu qemu64,+pdpe1gb -m 256M -nographic -cdrom build/vicell-x86.iso -boot d
  -no-reboot -device igb,netdev=net0 -netdev user,id=net0,restrict=on
  -device qemu-xhci,id=xhci -device usb-kbd,bus=xhci.0 -drive
  file=<64 MiB raw>,if=none,id=sata0,format=raw -device ide-hd,drive=sata0`.
  Evidence: `evidence/qemu-device-attach-smoke.log` (201 lines).
  Result: `Cellos >` reached; no `KERNEL PANIC` and no `[fault] Cell`.
  Observed devices: `00:02.0 8086:10c9` (QEMU `igb`, 82576-class) with the
  existing fail-closed line `[e1000] unsupported Ethernet 8086:10c9; driver gate
  closed`; `00:03.0 1b36:000d class 0c:03:30` (`qemu-xhci`);
  `00:1f.2 8086:2922 class 01:06:01` (ICH9 AHCI) carrying the attached SATA
  image. With no `-device intel-iommu` the kernel logs
  `[vtd] Intel VT-d not present (GCAP=0x0)` — the phase-05 negative-lane
  baseline.
  Consequences: (a) phase 02/03/04 device gates are physically attachable on the
  existing harness; (b) the QEMU `igb` model is 82576 (`8086:10c9`), so phase 04
  must add the real i210/i211 IDs explicitly (confirmed from the datasheet, not
  from the model) and keep rejecting families without a driver; (c) phase 05's
  absent-DMAR path already logs a named reason.
  This smoke proves device attachment and today's fail-closed behaviour only —
  no implementation, board qualification, or evidence-ceiling change.
- 2026-10-04 — plan created from `docs/roadmap/hardware-tracks.md` X86-PC-0..7
  (roadmap entries added the same day). No code or hardware claim.
