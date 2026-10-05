# 2026-10-04 — x86_64 PC lane: roadmap prerequisites, plan, and phase 01 shipped

## Why

The question was whether an industrial mini PC can run Cellos, and specifically
Tier 3. Answering it honestly required reading the tree: Tier 3 on x86 is AMD SVM
only, no real-PC driver path exists (no AHCI, xHCI frozen out, e1000 binds
82540EM only, IOMMU base hardcoded to q35), and there is no physical x86 target at
all. That turned a hardware-buying question into a prerequisite inventory, which
is now recorded instead of being implied.

## What landed (docs, commit `b3d1ae563` plus the docs commit before it)

1. **Roadmap prerequisites** in `docs/roadmap/hardware-tracks.md` as gates
   X86-PC-0..7, a Capability Lane row, an Immediate Open Gate bullet, risks
   `CELLOS-X86-PC-001` / `CELLOS-X86-DMAR-002` / `CELLOS-X86-VMX-003` /
   `CELLOS-AI-SIMD-004`, G2 posture, and the x86 PC driver order in `specs/04`.
   Two hard requirements are recorded, not preferences: **COM1/16550 at
   `0x3F8`/IRQ 4** (the only working x86 log and input path) and **Secure Boot
   must be disable-able** (no signed/measured x86 boot path exists).
2. **`docs/hardware-compatibility-list.md`** — mandatory R1–R7, evidence levels
   (`S1 qemu` / `S2` / `S3`), an intentionally empty machine table, the
   pre-purchase checklist, the refusal register, and one admission rule: a
   mandatory failure goes to the refusal register, never to a row.
3. **Plan `.agents/261004-1957-x86-pc-lane/`** — phases split by controller
   family (01 descriptor+HCL, 02a/02b AHCI, 03 xHCI, 04a/04b igb, 05 DMAR,
   06 multi-port COM/RS485, 07 physical lane), QEMU-first for 01–06.
   Validated by interview + 36/36 claim verification; promoted to Active in
   `.agents/plan-portfolio.md` with phases 01–06 authorized and 07 gated on
   hardware.
4. **Phase 01 completed** — `boards/pc/x86_64-pc` compatibility contract
   (`SocId::GenericX86Pc`, `hal_soc_x86::GENERIC_X86_PC`, feature
   `board-x86-pc`, `[x86-gate] board=… soc-profile=…` boot line), the family
   `DriverId` variants declared ahead of their phases, and the board-config gate
   extended (`README` command pinned verbatim + the `x86-pc` matrix entry).

## Evidence

- PC-descriptor ISO boots to `Cellos >` with
  `[x86-gate] board=x86_64-pc soc-profile=x86_64-pc`; the same tree **without**
  the feature still reports `board=qemu-q35-x86_64` (both logs under
  `.agents/261004-1957-x86-pc-lane/evidence/`).
- `cargo test -p cellos-boards -p hal-soc-x86 --target x86_64-unknown-linux-gnu`
  → 13/13 + 3/3 (baseline before the phase: 12/2).
- `scripts/check-hal-boundaries.sh` passes; `check-board-configs.sh` passes
  everything except its pre-existing `rpi4` failure.

## Two things worth remembering

1. **The QEMU gate caught a real defect.** The first build hung silently: the new
   diagnostic `puts` ran before `uart_16550::configure()`, and `putchar` asserts a
   configured port — so the panic happened before any serial output could exist.
   A silent hang at that point is now a known signature: it means "
   `uart_16550` used before `configure()`".
2. **`check-board-configs.sh` fails its `rpi4` entry on pre-existing WIP**
   (`hal/arch/arm/src/aarch64/monitor.rs:318` calls `pi_monitor_mmu_init`, gated
   behind `board-rpi3` in `el2.rs:91`; the call site does not exist in `HEAD`).
   Not attributable to this work; recorded so the next reader does not chase it
   in the x86 lane.

## Review

An independent reviewer audited the changeset: no code defects, verdict driven by
8 consistency findings (unqualified physical-PC claims, stale "no descriptor/HCL"
lines, phase-07 dependency missing 03, RS485 with no owning gate, weak R1
evidence rule, missing table columns, contradictory admission rule, and a
"universal PC facts" framing). All eight were fixed in the same change; the
descriptor is now framed as a **COM1-required compatibility contract**.

## Phase 02a (same session) — AHCI part A shipped

Delegated to a subagent with the phase file as spec, then verified and fixed
myself. Shipped as `fadcb51c9` (28 files).

- New `cells/drivers/ahci/` (759 lines): binds `01:06:01`, claims the ABAR,
  resets the HBA with `GHC.AE` held, waits for the initial D2H FIS (task-file idle
  → known `PxSIG`), starts the engine, completes and validates IDENTIFY DEVICE.
  Diskless machines idle instead of erroring. Part B (data path, block
  registration, persistence) stays out of scope by design.
- Two interface changes the phase had to make, recorded rather than incidental:
  `PcieDeviceInfo` **appended** `bar_mem_base`/`bar_mem_len` (ICH9's ABAR is
  BAR5, i.e. not BAR0), and `/bin/ahci` was wired through the launch profile,
  boot ceiling, caps, the init block-driver spawn, the dev policy, the packaging
  script and CI (build/sign/fat32 + a new step running the lane). CI wiring was
  added to the plan's Evidence rules because the phase is not gated without it.
- Verification I ran myself: SATA boot (all markers → `Cellos >`), diskless boot
  (idle), default q35 lane (`board=qemu-q35-x86_64`), `ahci-x86` 2/2,
  `cellos-boards` 13/13, HAL boundaries pass.
- Review found what q35 tolerates but hardware need not: AE-before-register and
  reset-with-AE, the initial-FIS wait, IDENTIFY payload validation, COMRESET held
  against the monotonic clock rather than an iteration count, test-image RAII, ISO
  provenance. All fixed and re-verified. The one open point — `bar_mem_*` relying
  on the kernel's own ECAM scan retaining every BAR, since the Platform-Cell
  registration path stores only BAR0 — is recorded as a plan risk with the syscall
  site annotated; it belongs to the Platform-Cell cutover owner.

## Next

Phase 02b (AHCI part B: READ/WRITE data path, block registration, two-boot
persistence) is the next sequential slice: it makes the SATA device usable as
storage so `/data` and the guest disk can live on it. Then 03 (xHCI), 04a/04b
(igb), 05 (DMAR), each with its own QEMU gate and CI wiring; hardware is bought
only after 02b/03/04b/05 are green on QEMU, and phase 07 is the only phase with
no QEMU gate.
