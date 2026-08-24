# Current Focus

**Last updated**: 2026-08-24

## Active Stage

G1 Robot & Embedded remains the active product stage. The practical focus is
keeping board/HAL/kernel contracts accurate while closing real hardware evidence
without treating QEMU or compile-only checks as board qualification.

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
- RV64 QEMU desktop now has a verified end-to-end path from VirtIO tablet
  events through input, compositor hit-testing and surface-local routing to a
  ViUI dashboard control. QMP captures prove non-black GPU scanout before and
  after a STOP click. This is a bounded surface interaction slice, not a claim
  of a window manager, desktop shell, drag/resize policy, or G2 qualification.

## Current Documentation Corrections

- MicroPython is historical, not an active workspace runtime.
- Cargo workspace count is generated/discovered data; avoid hardcoding old
  counts except in generated metrics.
- `docs/TODO.md` is no longer project documentation. Personal task tracking
  belongs in `.agents/`.

## Next Useful Work

1. Integrate and qualify a concrete Cell-to-Cell Anywhere production root;
   keep remote/public exports disabled until root provenance, rollback state,
   and live `/srv/cellos` persistence all have runtime evidence.
2. Close remaining hardware-gated board evidence with PASS/FAIL/BLOCKED logs.
3. Keep Phase 04 blocked until signed CI or a secure measured runner can retain
   authenticated evidence for a qualified floor, persistent recovery, physical
   hostile cases, provisioned anchors, production wiring, and both human
   approvals; local verification cannot satisfy this gate.
4. Keep full Phase 07 and Phase 08 blocked until the Phase 03 provenance,
   Phase 04 production-admission, and Tier 2 native-domain gates are closed;
   the verified atomic prerequisite and Phase 08 predesign do not authorize
   production loader or Manifest-v3 claims.
5. Continue reducing kernel-resident legacy driver/orchestration code only when
   a slice has explicit runtime evidence and rollback notes.
6. Keep HAL/board boundary checks in CI whenever board descriptors, SoC facts,
   or HAL ABI hook declarations change.
7. Use [hardware-tracks.md](hardware-tracks.md) and
   [runtime-and-platform-tracks.md](runtime-and-platform-tracks.md) for lane
   status instead of re-reading the archive.
