---
phase: 1
title: "x86_64-pc board descriptor + HCL model"
status: completed
priority: P2
effort: S (facts + docs; no new driver)
dependencies: []
tier: thinking
ceiling: qemu
---

# Phase 01 — `x86_64-pc` board descriptor + HCL model

## Evidence (2026-10-04)

- `evidence/phase-01-qemu-x86-pc-descriptor.log` — build
  `cargo build --release -p cellos-kernel --target x86_64-unknown-none --features board-x86-pc`,
  ISO via `X86_ISO_ROOT=build/x86-pc-lane/iso-root scripts/x86/make-iso-ci.sh`,
  booted with `scripts/qemu-x86_64-test.sh` on q35. Result: `PASS: x86_64 shell
  prompt reached`; `[x86-gate] board=x86_64-pc soc-profile=x86_64-pc`;
  `Tier 2 admission: DISABLED`; no panic/fault.
- `evidence/phase-01-qemu-q35-regression.log` — same tree built **without** the
  feature: `[x86-gate] board=qemu-q35-x86_64 soc-profile=qemu-q35-x86_64`, shell
  reached. No regression to the default x86 lane.
- `cargo test -p cellos-boards -p hal-soc-x86 --target x86_64-unknown-linux-gnu`
  → 13 passed / 0 failed and 3 passed / 0 failed (baseline before the phase was
  12 / 2).
- `cargo check -p cellos-kernel --target x86_64-unknown-none` (default) and
  `--features board-x86-pc` → both clean.
- `scripts/check-board-configs.sh` → the new `x86-pc` matrix entry and
  `scripts/check-hal-boundaries.sh` pass. The `rpi4` entry fails on
  `hal/arch/arm/src/aarch64/monitor.rs:318` calling `pi_monitor_mmu_init`, which
  is `#[cfg(feature = "board-rpi3")]`-gated in `el2.rs:91`. **Pre-existing, not
  from this phase**: both files are uncommitted user WIP (+243/+71 lines) and the
  call site does not exist in `HEAD`. Recorded so the next reader does not
  attribute it to the x86 lane.
- Bug caught by the gate and fixed in this phase: the first build hung silently
  because the new diagnostic `puts` ran **before** `uart_16550::configure()`;
  `putchar` asserts a configured port (`config()` in
  `hal/arch/x86/src/x86_64/uart_16550.rs`), so the panic was emitted before any
  serial output was possible. The log lines now follow `configure()`+`init()`,
  matching the existing emission point.

## Target

Roadmap gate **X86-PC-0**. Today `boards/` has only `qemu/q35-x86_64` declaring
`DriverId::Uart16550PortIo` (`boards/qemu/q35-x86_64/board.rs:8`), so a real PC
has no descriptor and no HCL to be listed in.

## Verified constraints (pre-Build recon, 2026-10-04)

- `kernel/src/board.rs:81-91` — `selected_x86_64_soc()` matches **only**
  `SocId::QemuX86Q35` and panics on anything else, after validating
  `hal_soc_x86::QEMU_Q35`. A real-PC descriptor therefore also needs a new
  `SocId` variant (`boards/src/descriptor.rs`), a matching `X86PlatformProfile`
  in `hal/soc/x86` (facts single-copy), and a new arm in that match plus the
  board-feature selection block. Descriptor facts alone are not sufficient.
- `boards/src/catalog_tests.rs` asserts each board's driver set and
  `validate_for(Architecture::X86_64)`; a new descriptor needs a row there.
- `scripts/check-board-configs.sh` is the lane this phase must extend: it
  requires `README.md` + `board.rs` per board, pins each README's build command
  **verbatim**, keeps placeholder dirs README-only, forbids registering
  placeholder names outside their READMEs, runs `scripts/check-hal-boundaries.sh`,
  and runs the per-board `cargo check` matrix plus two conflicting-feature
  negative checks. A new selectable x86 board belongs in that matrix.
- Baseline captured before any change:
  `cargo test -p cellos-boards -p hal-soc-x86 --target x86_64-unknown-linux-gnu`
  → 12 passed / 0 failed and 2 passed / 0 failed.

## Change

- Add `boards/pc/x86_64-pc/board.rs` (a facts-only descriptor): COM1
  `PortIoDevice { base: 0x3F8, irq: 4 }`, legacy firmware windows, HPET present,
  and the driver set the kernel requires to reach a shell. Mirror the shape and
  documentation style of `boards/qemu/q35-x86_64/`; keep every fact that is
  genuinely machine-dependent out of the driver code and in the descriptor.
- Decide and document how a machine-specific descriptor is selected at build
  time without forking mechanism code (one `x86_64-pc` descriptor plus a boot
  checklist, or one descriptor per qualified machine — pick the boring option
  and state the rule in `docs/code-standards.md` if it is not already covered).
- Create `docs/hardware-compatibility-list.md` with the baseline requirement
  table, the machine table (empty), the evidence-level vocabulary, and the
  acquisition checklist (COM1/16550, HPET, SATA in AHCI mode, Secure Boot
  disable-able, VT-x/VT-d, NIC family, BIOS version).
- Point `docs/roadmap/hardware-tracks.md` and `docs/project-roadmap.md` at the
  descriptor and the HCL file.
- **Declare the family `DriverId` variants here** (AHCI storage, xHCI, igb NIC,
  multi-port 16550 — plus their `boards/src/catalog_tests.rs` rows) even though
  the driver cells land in phases 02–06. Otherwise every driver phase edits the
  same `boards/src/descriptor.rs` and `scripts/build-x86_64-cells.ps1` and the
  phases cannot run in parallel; doing it in one place keeps file ownership
  disjoint.

## QEMU-first gate

- `bash scripts/build-x86_64-cells.ps1` (or the CI equivalent) + `build/make-iso.sh`
  with the new descriptor selected, boot on `q35`:
  `BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh`.
- Required markers: existing x86 boot markers unchanged, `x86_64-boot` suite
  still 7/7, no `KERNEL PANIC` / `[fault] Cell`.
- Record the exact QEMU invocation and the descriptor's facts in the phase
  evidence; state that q35 is not a PC.

## Acceptance

- A shell is reached on QEMU with the new descriptor selected, with all previous
  x86 lanes green.
- `docs/hardware-compatibility-list.md` exists, lists zero `physical` rows, and
  carries the acquisition checklist including the Secure Boot and COM1 rows.
- No driver source is copied per-board: one descriptor, no mechanism forking.

## Out of scope

- Any driver family work (phases 02–06).
- Claiming any physical machine.

## Risk assessment

- **Undo:** delete the new descriptor directory and the HCL file; revert the
  roadmap pointers. Nothing else depends on it yet.
- **Not undoable:** a published HCL row or a claim made from QEMU evidence —
  which is exactly why this phase publishes none.
