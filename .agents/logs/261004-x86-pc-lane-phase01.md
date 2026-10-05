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

## Phase 02b (same session) — AHCI part B shipped

Delegated, verified, fixed, shipped as `295720ea9` (17 files).

- READ/WRITE DMA EXT, FLUSH CACHE EXT, a new `dispatch.rs` and block registration
  along the NVMe path; the cell registers only when it found a usable disk and
  idles otherwise, so a diskless machine spawns it harmlessly.
- Verified through the standard packaging path — this time correctly. The phase
  carries a self-correction: my earlier 02a "re-verify" rebuilt the cell binary
  but booted an embedded FS image assembled earlier, so it exercised the old
  cell. Rebuilding a cell does **not** update `kernel/src/embedded-x86_64/`;
  only the packaging script (or `EMBEDDED_OVERRIDE`) does. That is how a part-A
  source/binary mismatch stayed hidden until 02b rebuilt the image: the committed
  part-A source read the IDENTIFY PRDBC from command-header offset 12 instead of
  DWORD1 at offset 4. Fixed, and the 02a record now says so.
- Review found three defects QEMU tolerates — the command-header write flag was
  bit 5 (ATAPI) instead of bit 6 (W), completion ignored the fatal
  `HBFS`/`HBDS`/`IFS`/`OFS` conditions, and a poll timeout left tag 0 owned by the
  HBA — plus two test-honesty gaps (a cached VFS provider TID claimed as routing,
  and a persistence oracle that could skip silently in CI). All fixed and
  re-verified: `ahci-x86` 5/5 with the two-boot FAT32 oracle, regressions
  `nvme-x86` 3/3, `pcie-multibus-x86` 2/2, `x86_64-boot` 9/9,
  `driver-registration-contract` 3/3.
- Recorded risk, not fixed here: registration is single-slot/last-wins while the
  consumer caches its provider TID, so on a two-storage-device machine which
  drive serves I/O is scheduling-dependent. Owner-side arbitration (Platform/VFS)
  is the fix; every lane attaches one storage device meanwhile.
- The `unsafe` allowlist entries the F1 gate requires were approved by the
  repository owner (field records `dmin`).

## Phase 03 + 03b (same session) — xHCI, HID, and the producer role

Delegated, reviewed, fixed, shipped as `5506fc25b` (54 files). The owner reopened
the xHCI scope for this lane (recorded in the G1–G3 driver plan's phase-01 gate
and its license BOM) and approved three trust decisions: sharing allowlist bit 50
for the new syscall, the `/bin/xhci` capability grant, and the new image
composition.

- **Phase 03**: `cells/drivers/xhci/` brings the controller up, addresses a slot,
  enumerates `0627:0001`, parses the HID boot keyboard and completes an
  interrupt-IN transfer. The HID decode path moved into a shared `driver-hid`
  crate that `dwc2-usb` re-exports (single-copy; ARM builds verified).
- **Phase 03b**: append-only `RegisterUsbHidProducer` (423) +
  `service::USB_HID_PRODUCER` (17), gated on `usb_driver`; the input service's
  producer gate now accepts that role, and `driver-dwc2-usb` publishes both roles
  because on RPi3 the LAN9514 *is* the NIC. Result: the injected keystroke is
  echoed by the shell (`shell: command not found: q`), with zero
  `[kernel] syscall denied` lines.
- **What the reviews caught** (two rounds, both material): the Enable Slot timeout
  was every operational-register access missing `CAPLENGTH`; then five follow-on
  defects (slot-ID bits, IN-endpoint DCI, HID subclass descriptor offset,
  scratchpad field order, and the singleton-NIC collision that forced 03b); then,
  on 03b, a CI job that did not mirror the new image, `/bin/xhci` **and**
  `/bin/ahci` running at the kernel's `u64::MAX` allowlist default, and a cached
  input-service route that would never recover from a service restart.
- **Process notes worth keeping**: (a) rebuilding a cell does not update the
  embedded FS image — only the packaging script (or `EMBEDDED_OVERRIDE`) does,
  and I was bitten by that in 02a; (b) files carrying unrelated user WIP
  (`cells/tools/init/src/boot.rs`, `cells/drivers/dwc2-usb/src/main.rs`,
  `cells/services/input/src/dispatcher.rs`, `hal/arch/arm/**`, …) were staged
  hunk-by-hunk, and the committed state was then compiled out-of-tree in a
  temporary worktree to prove it is self-consistent; (c) the repo's u64 syscall
  allowlist bitmap is full, so new opcodes must share a family bit and rely on the
  capability gate.

## Next

Phase 04a (igb NIC part A: identity, registration, Tx/Rx — no scope decision
needed, QEMU has an `igb` model verified as `8086:10c9`), then 04b (DHCP + VT-d),
05 (ACPI DMAR → real IOMMU), 06 (multi-port COM/RS232-485). Hardware is bought
only after 02b/03/04b/05 are green on QEMU, and phase 07 is the only phase with no
QEMU gate. Follow-up hygiene recorded but not scheduled: `nvme`/`e1000` still run
without a `declare_syscalls!` allowlist; the `bar_mem_*` fields depend on the
kernel's own ECAM scan retaining every BAR; two storage drivers plus a caching
consumer make storage selection scheduling-dependent.
