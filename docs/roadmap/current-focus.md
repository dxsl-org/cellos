# Current Focus

**Last updated**: 2026-08-21

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
- That tooling completion is not production loader readiness. Three adjacent,
  pre-existing blockers are registered as `CELLOS-LOADER-SIG-001` (Critical;
  Phase 03 provenance/signature boundary), `CELLOS-LOADER-RACE-002` (High;
  Phase 07 atomic task publication), and `CELLOS-LOADER-CLEANUP-003` (Medium;
  Phase 07 denial rollback). Phase 08 remains dependent on Phase 07.

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
4. Close the three registered loader-security blockers under their Phase 03
   and Phase 07 owners before claiming production loader readiness; do not
   reopen completed Manifest-v2 tooling acceptance to absorb that work.
5. Continue reducing kernel-resident legacy driver/orchestration code only when
   a slice has explicit runtime evidence and rollback notes.
6. Keep HAL/board boundary checks in CI whenever board descriptors, SoC facts,
   or HAL ABI hook declarations change.
7. Use [hardware-tracks.md](hardware-tracks.md) and
   [runtime-and-platform-tracks.md](runtime-and-platform-tracks.md) for lane
   status instead of re-reading the archive.
