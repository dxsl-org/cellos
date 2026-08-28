# Current Focus

**Last updated**: 2026-08-28

## Development-first execution boundary

[ADR-0007](../decisions/0007-development-first-hardware-constrained-execution.md)
records the current decision: use QEMU and the two existing Raspberry Pi 3
boards; procure no additional hardware now. The currently available peripherals
are a camera and an HDMI cable for external-display testing. Other sensor work
is deferred. G1 Robot & Embedded is the active product-stage overlay, not a
global queue. Capability dependencies
and evidence ceilings determine executable order.

QEMU evidence is software-only. RPi3 and sensor evidence is development and
hardware-integration evidence for the exact exercised devices only. The RPi3 is
not, and must never be presented as, a production-security qualification target
or qualified independent external floor.

## Current executable work

- Continue useful QEMU software and integration work to the `qemu` ceiling.
  Host/QEMU results never qualify a board, secure root, cloud authority,
  physical-hostile posture, or production release.
- Use both existing RPi3 boards for G1 boot and peripheral integration work.
  The immediate peripheral lane is external-display testing over HDMI. The HDMI
  unsafe-copy review gates only that governed boundary: complete its
  host/build/policy work when authorized, then stop at its framebuffer-range,
  mailbox-coherency, and visual hardware gates. It does not globally block other
  RPi3 work.
- Defer camera and other sensor integration until the sensor lane is resumed.
  The camera's exact identity and interface must be recorded before it is
  exercised or used as physical-behavior evidence.
- Tier 3 hostile-QEMU qualification remains blocked at its current result, but
  adding the missing VMM/VirtIO bounds, descriptor, backend-error transport and
  independent preemption/supervisor-restart outcomes is executable QEMU work.
  The existing CPU-bound budget stimulus under pinned QEMU-TCG 10.2.0 is not
  those missing outcomes.
- The approved local Cell-to-Cell fixture and RV64 `app-bench` broker oracle
  are now executable through `scripts/run-c2c-broker-oracle-qemu.sh`. The
  isolated QEMU run measured 100 warmup / 1,000 calibration calls, passed the
  1/2/4/8/16-client sweeps and role gate, completed the 10,000-call soak with
  zero silent drops, and passed the queue-overflow oracle. This is single-guest
  local-runtime QEMU evidence only; it does not prove two-node direct LAN,
  relay, remote/public operation, or protected relay identity.
- Project each completed lane immediately into the roadmap and acceptance views
  at its exact evidence ceiling.
- The managed-surface child is implementation-complete with host/RISC-V
  evidence. Its QEMU input/scanout run invoked the repository-owned disk
  generator, which refused to sign the image until the shared F1 policy is
  restored: the Hypha gateway lacks `#![forbid(unsafe_code)]` and BCM mailbox
  unsafe code lacks a reviewed allowlist entry. No additional desktop contract
  is authorized.

## Work classification

- **Current executable work:** the QEMU, two-RPi3, HDMI external-display,
  local Cell-to-Cell, evidence-projection, sensor, and separately reopened
  governed lanes above. Camera and other sensor integration retains this
  classification but is deferred in the current session order.
- **Current-scope technical debt:** confirmed defects and maintainability gaps
  in supported paths, including the raw TLS length contract and interactive
  polling/CI evidence gaps tracked by the
  [open risk register](open-risk-register.md). This label does not apply to all
  advanced work.
- **Future capability:** remote/public Cell-to-Cell operation, additional
  desktop depth, x86 parity beyond current dependencies, G3 accelerators, G4
  `rust-std`, and G5 virtualization expansion.
- **External-gated prerequisite:** unavailable exact boards, protected relay
  assets/cloud identity, and an exact production-root vendor evidence package.
  No stock TPM or generic secure-element counter is selected as the production
  floor.
- **Production release gate:** remote C2C identity where applicable, protected
  relay identity, production KMS/root, secure/measured boot, a qualified
  rollback-resistant external floor, persistent recovery, physical hostile
  evidence, an authenticated runner, required human approvals, and governed
  release-ledger closure.

Production admission and release remain disabled and fail-closed until every
applicable production release gate is satisfied. Those gates block only the
production-admission or production-release milestone that owns them; they do
not block the current executable work above. Precise owners and reopening
events are maintained in
[the roadmap capability table](../project-roadmap.md#capability-lanes).

## Recent State

- RPi3 inventory is partial. RPi3-B is identified as board revision `a22082` /
  Raspberry Pi 3 Model B with unique serial `000000003d042795`. Its post-reboot
  exact-device run transferred and checksum-verified `cellos.uimg`, discovered
  the SD card and four MBR partitions, mounted FAT16/FAT32/littlefs/RedoxFS,
  completed policy/kernel self-tests and the first BCM scanout flush, reached
  the shell, and finished the VFS suite at `89 PASS, 0 FAIL`. This is
  development/hardware-integration evidence. HDMI connected after firmware
  startup produced black / `No Signal`; a power-off retry with HDMI connected
  and the display active before firmware startup showed U-Boot, the Cellos boot
  log, and the `Cellos >` prompt. This confirms the reproduction condition, not
  a root cause among firmware EDID sampling, display handshake/input behavior,
  and driver behavior. No named reviewer approval is recorded for the mailbox
  unsafe DMA-page copies, so this observation is non-qualifying and the HDMI
  visual hardware gate remains `governance-gated`. RPi3-A remains entirely
  pending, and the historical shell/BCM-scanout capture remains unassigned
  because it contains no unique serial.
- RPi3 post-HAL-split smoke work has landed in `main`.
- HAL to kernel Rust ABI signatures are centralized in
  `hal/traits/arch/src/kernel_abi.rs`.
- Root `boards/` is the owner for board descriptors and fallback assets.
- SoC immutable facts live under `hal/soc/*`.
- Shared drivers remain single-copy in kernel integration paths or
  `cells/drivers/*`; boards do not fork UART, SDHCI, GIC/PLIC, PCIe, or
  DesignWare-style mechanisms.
- Cell-to-Cell Anywhere has landed its bounded local broker and fail-closed KMS
  foundation. Remote/public operation remains disabled while the production
  hardware-backed root and trusted monotonic epoch are unavailable.
- Tier 1 admission prequalification now has its canonical 18-row catalog, all
  33 stable `test-hooks` IDs, and a strict runtime parser. This is test
  infrastructure only: local runs are non-admissible, production admission is
  disabled, and Phase 04 remains blocked.
- Manifest-v2 tooling Phase 05 is complete. The loader now classifies a unique
  manifest section as `Absent`, `Valid` (v1 or v2), or `Malformed` before task
  creation; only genuine absence selects the explicit legacy path policy.
  Rust v2 remains exactly 16 bytes and Zig v1 exactly 8 bytes, with compatible
  upcast behavior and protection-class terminology separated from application
  execution tiers.
- The Phase 07 atomic-publication prerequisite is verified, not full Phase 07
  completion: a fresh `test-hooks` build/sign, a populated-fixture one-hart VFS
  run (1/1; AP-00–11 and AP-15; AP-13 explicitly `SKIP`), and an SMP atomic
  run (1/1; AP-00–15) passed. The SMP proof includes AP-02 live-PTE/TLB
  restoration evidence, an AP-13 remote-hart scheduler witness, and the
  terminal/aggregate markers. Its terminal state remains
  `ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED`.
- Phase 08 Manifest-v3 ABI predesign is validated (20/20), with pinned consumer
  inventory and content digests. Its state is
  `PREDESIGN_COMPLETE / PHASE08_BLOCKED`: it depends directly on Phases 03, 05,
  and 07 and adds no Manifest-v3 code, readiness claim, or approval.
- Full Phase 07 and Phase 08 remain blocked by the Phase 03
  provenance/signature boundary, the Phase 04 production-admission gate, and
  the Tier 2 native-domain gate. The verified atomic prerequisite does not
  clear those release conditions.
- `CELLOS-VFS-SMP-006` is closed after the owner-lifetime lifecycle
  implementation passed API90, an RV32 release compile, fresh `test-hooks`,
  one-hart VFS 2/2, and two-hart VFS 7/7. Final quality and security closure
  both passed. RV32 runtime remains unavailable on this host because OpenSBI
  firmware is missing; that compile-only evidence gap is non-blocking and is
  not a runtime claim.
- RV64 native-domain substrate and scheduler transitions (Spec 22 Items 2–3)
  have passed one-hart (`switch`, `sas-fastpath`) and two-hart (`migration`)
  QEMU evidence runners. AP-13 pre-ready quota drain race, release-build supervisor
  unregistration, and SMP UART timing were resolved. Production admission remains
  disabled, SAS remains default, and no Manifest v3 or ledger qualification claims
  are made.
- RV64 QEMU desktop has an implemented bounded window-policy scenario.
  Interactive surfaces set bounded titles and poll typed lifecycle events beside
  their captured pointer and selected-owner keyboard input. The compositor owns
  clipped frame/title/control decoration, titlebar drag, edge/corner resize,
  minimize/maximize/restore controls, and explicit close negotiation; client
  content coordinates remain unchanged.
- Resize, maximize, and restore commit only after the owner applies a
  replacement Grant and acknowledges the matching configure serial. Minimized
  surfaces are not paintable or hit-testable until restored; an accepted close
  is removed when its owner destroys the surface. `SurfaceRole::Background`
  remains visible but cannot hit-test, raise, or use decoration controls.
  The `window-policy` scenario retains QMP/PPM background, capture, and
  keyboard-focus coverage while adding lifecycle paths; the separate
  compositor-cursor scenario retains cursor coverage. This is still not a
  desktop shell or G2 qualification: taskbar, snapping, persistence, and live
  resize preview remain absent.

## Current Documentation Corrections

- MicroPython is historical, not an active workspace runtime.
- Cargo workspace count is generated/discovered data; avoid hardcoding old
  counts except in generated metrics.
- `docs/TODO.md` is no longer project documentation. Personal task tracking
  belongs in `.agents/`.

## Next-session work order

1. Record the exact identity and current condition of both available RPi3
   boards. Record the available camera's exact identity and interface without
   starting sensor integration. Buy no additional hardware.
2. Exercise the existing RPi3 boot/peripheral path on the available boards and
   retain development-only logs tied to the exact board. Do not infer a
   production-security or external-floor result.
3. Prepare the bounded external-display path using the available HDMI cable.
   Respect the HDMI unsafe-copy governance gate; do not cross the
   framebuffer-range, mailbox-coherency, or visual hardware gates without the
   required review and exact-board evidence.
4. Publish each observed result at its evidence ceiling with the remaining
   lane-local gate. Continue local Cell-to-Cell baselines if the hardware lane
   is waiting on physical access or a named review.
5. Resume camera and other sensor protocol, board-interface, driver, fixture,
   QEMU, and exact-device RPi3 work only in a later sensor session. Keep QEMU
   results software-only and physical results development/hardware-integration-
   only.
6. Keep protected relay assets, other physical boards, G3 acceleration, and the
   ADR-0006 production root external-gated. Keep every production-admission and
   release invariant mandatory without making it a global development blocker.
7. Keep HAL/board boundary checks in CI whenever board descriptors, SoC facts,
   or HAL ABI hook declarations change.
8. Use [project-roadmap.md](../project-roadmap.md#capability-lanes) for
   cross-lane routing and the topic pages for evidence details.
