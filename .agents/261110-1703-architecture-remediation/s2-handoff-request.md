# S2 handoff request — authority (phases 04, 05, 06)

**Date:** 2026-11-10 · **Plan:** [plan.md](plan.md) · **Prior slice:** S1 open only for untrack (waits for a pushed CI cycle, [prepared](untrack-after-ci.md)) and release (blocked on release signing); neither touches S2 files.

## 1. Scope

- Phase 04 — one derived trust posture with mix rejection; release policy key provisioning; owner-gate decision; guarded user copy unconditional on 64-bit targets; release-posture boot lane.
- Phase 05 — one declarative cell/driver catalog replacing the seven path tables and the `sign-policy.py` source scraper; byte-identical `POLICY.BIN`.
- Phase 06 — launch authority keyed on the kernel-measured image identity instead of the spawner-chosen basename (C-1); PCIe handout limited to the caller's catalog row (H-3).

Regression net: the `x86-c2c` steps, x86 boot, RV64 baseline and AArch64 boot are run locally at each checkpoint until CI runs on a push.

## 2. File set (file-bounded exception)

Phase 04
- Modify: `kernel/build.rs`, `kernel/Cargo.toml` (features only), `kernel/src/main.rs` (posture guard at top), `kernel/src/signing.rs`, `kernel/src/policy.rs`, `kernel/src/admission/{gate,mod,owner_key}.rs`, `kernel/src/loader/governed_spawn.rs`, `kernel/src/task/copy_glue/mod.rs` and the `validate_user_buf` call sites in `kernel/src/task/syscall.rs` that the copy view replaces, `.github/workflows/ci.yml`, `scripts/lanes.toml`, `docs/security-model.md`
- New: `kernel/src/posture.rs`

Phase 05
- New: `catalog/cells.toml`, `catalog/fixtures.toml`, `kernel/src/catalog.rs`, `scripts/check-catalog.py`
- Modify: `kernel/build.rs`, `kernel/src/task/cap.rs`, `kernel/src/loader.rs`, `kernel/src/loader/boot_ceiling.rs` (+ `boot_ceiling/selftest.rs`), `kernel/src/loader/launch_profile/{mod,profiles,targets,tests}.rs`, `kernel/src/policy.rs`, `kernel/src/task/drivers/pcie_ecam.rs`, `scripts/sign-policy.py`, `.github/workflows/ci.yml`

Phase 06
- Modify: `kernel/src/loader/launch_profile/**`, `kernel/src/loader/{governed_spawn,spawn_request}.rs`, `kernel/src/task/syscall.rs` (launch-edge authorisation, name gates, `pcie_usable_by`), `kernel/src/task/drivers/pcie_ecam.rs`, `catalog/*.toml`, `cells/services/supervisor/src/main.rs`, `docs/security-model.md`
- New: one negative-control test cell under `cells/tests/` (fixture catalog row only), root `Cargo.toml` member line for it
- **Conditional, Law-1:** `libs/api/src/abi/caller_identity.rs` — only if the supervisor needs an identity the caller trailer does not carry; a separate CP1 is filed before any edit.

All phases: this plan directory, `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md`, `docs/project-changelog.md`, `scripts/unsafe-allowlist.toml` / `scripts/mutable-globals.toml` only if a touched file's entries move.

## 3. Behaviour changes the owner should expect

- Phase 04: mixed feature sets (e.g. `release-posture` + `test-hooks`) stop compiling. Kernels built without `native-domains` on 64-bit targets stop compiling under a release posture.
- Phase 05: none intended; generated tables must equal today's (equality test), `POLICY.BIN` byte-identical.
- Phase 06: a binary's launch rights follow its signed image name, not its file name. Unsigned dev cells get rows only where the dev posture lists them. A cell holding `PcieDriverCap` can no longer claim a PCI function outside its catalog row.

## 4. Irreversible or externally visible effects

None persistent. Anchors are not changed for already-signed artifacts. If Phase 06 needs a Law-1 delta, that part waits for CP1/CP2.

## 5. Conflicts

- iGPU DMA-export slice (approved, not effective): touches `kernel/src/task/syscall.rs`, `iommu*`, `libs/api/src/abi/syscall.rs`. Granting S2 the lane keeps it waiting; S2 edits disjoint regions of `syscall.rs`.
- x86 Tier-2 GPU runtime R1–R4/R6: user-space files only; no overlap. Its pending H-1 launch rows (`/bin/x86-fpu-preserve`, `/bin/cpp-hosted-smoke`, `/bin/drm-lite-x86`) move into `catalog/cells.toml` in Phase 05.

## 6. Checkpoint

Phase 06 closes S2: local full lane set plus an independent `haily-reviewer` pass on the authority diff.
