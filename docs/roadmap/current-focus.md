# Current Focus

**Last updated**: 2026-08-27

## Execution Model

G1 Robot & Embedded is the active product-stage overlay, not a global queue.
Capability dependencies and their evidence ceilings determine executable order.
Host and QEMU evidence are software evidence only; they never qualify a board,
secure root, cloud authority, or production release.

## Work Available Without New Hardware

- Complete the RPi3 HDMI software boundary through its host/build/policy checks,
  then stop at framebuffer-range, mailbox-coherency, and visual hardware gates.
- **Blocked:** Tier 3 hostile QEMU reaches a CPU-bound budget stimulus under
  pinned QEMU-TCG 10.2.0, but bounds/descriptor/backend lack VMM/VirtIO
  transport and reset/preemption have no independent VMM recovery outcome.
- Project each completed lane immediately into the roadmap and acceptance views.
- The managed-surface child is implementation-complete with host/RISC-V evidence.
  Its QEMU input/scanout run invoked the repository-owned disk generator, which
  refused to sign the image until the shared F1 policy is restored: the Hypha
  gateway lacks `#![forbid(unsafe_code)]` and BCM mailbox unsafe code lacks a
  reviewed allowlist entry. No additional desktop contract is authorized.

Desktop/ViUI/SDK, local Cell-to-Cell, security/PAL remediation, authenticated
evidence, and x86 VirtIO each have independent scope, contract, governance, or
dependency gates. Their precise owners and reopening events are maintained in
[the roadmap capability table](../project-roadmap.md#capability-lanes).

## Recent State

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

## Next Useful Work

1. For local Cell-to-Cell, establish the approved test-only K1 image fixture
   before recording IPC, queue/cache, and saturation baselines.
2. For every `scope-gated` or `governance-gated` lane, perform only its named
   reopening action: obtain the RPi3 unsafe review, add the Tier 3 VMM/VirtIO
   stimuli and independent outcomes, or obtain the required desktop/security/
   evidence approval.
3. After a lane transitions, publish its exact evidence ceiling and remaining
   reopening event through the roadmap/ledger owner; do not wait for another
   product stage.
4. Keep physical board evidence, protected relay assets, G3 accelerator work,
   and the ADR-0006 production root visibly external-gated.
5. Keep HAL/board boundary checks in CI whenever board descriptors, SoC facts,
   or HAL ABI hook declarations change.
6. Use [project-roadmap.md](../project-roadmap.md#capability-lanes) for
   cross-lane routing and the topic pages for evidence details.
