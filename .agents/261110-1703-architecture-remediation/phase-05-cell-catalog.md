---
phase: 5
title: "Single cell and driver catalog"
status: completed
priority: P1
effort: "7–10d"
dependencies: [4]
tier: thinking
checkpoint: false
---

# Phase 05: Single cell and driver catalog

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-2, Medium authority/hardware](../reports/architecture-review-261110.md) · `kernel/src/task/cap.rs` · `kernel/src/loader/boot_ceiling.rs` · `kernel/src/loader/launch_profile/` · `kernel/src/policy.rs` · `kernel/src/task/drivers/pcie_ecam.rs` · `scripts/sign-policy.py` · `config/services.toml` · `boards/src/descriptor.rs`

## Overview

Replace the hand-synchronised path tables and the Python source scraper with one declarative catalog that the kernel, the policy signer and the image assembler all read. Pure data refactor: resulting tables must be equal to today's.

## Key Insights

- Reviewer-cited tables: `with_path_caps` + helpers (`cap.rs:469-626`), `legacy_path_caps` (`loader.rs:66-81`), `boot_ceiling::lookup` (`boot_ceiling.rs:56-253`), `reviewed_user_target_ceiling` (`targets.rs:30-162`, partly verified), caller profiles (`profiles.rs`), `is_trusted_core` (`policy.rs:1063`), PCI ID → path (`pcie_ecam.rs:42-52`), plus mirrors in `sign-policy.py:117-124,489-524`.
- Tables have diverged: five packaged NIC drivers have empty ID tables and no cap row.
- Fixture rows live inline under `test-hooks`/`phase0x` cfgs (30 sites).

## Requirements

- Functional: one catalog source; generated Rust tables for the kernel; the signer imports the same data; build-time cross-check of `config/services.toml`, the image manifest and `boards` `DRIVERS` against it; fixture rows in a separate catalog file compiled only under `posture = "test"`.
- Non-functional: no `POLICY.BIN` wire change; no behaviour change; no allocation added on the spawn path.

## Architecture

```
catalog/cells.toml ──► kernel/build.rs ──► OUT_DIR/catalog.rs (const tables)
        │                                   (request caps, boot ceiling, launch edges,
        │                                    trusted-core flag, PCI IDs/class, DriverId)
        ├──► scripts/sign-policy.py (reads TOML, no Rust parsing)
        └──► scripts/check-catalog.py (services.toml, image manifests, boards)
catalog/fixtures.toml ──► only when posture = "test"
```

Row shape: `path`, `role`, `request`, `boot_ceiling`, `edges[] = {route, target}`, `trusted_core`, `pci = {ids[], class}`, `driver_id`.

## Assumptions

- **Claim:** every existing table can be expressed in the row shape without special cases beyond VFS region masks.
  **Confidence:** medium
  **How to verify:** transcribe three hardest rows first (`/bin/vfs`, `/bin/init`, `/bin/dwc2-usb`) and compare generated output.
- **Claim:** existing selftests (`loader/boot_ceiling/selftest.rs`, `launch_profile/tests.rs`) pin enough of the tables to detect a transcription error.
  **Confidence:** medium
  **How to verify:** add a one-off equality test that dumps old and generated tables for every known path and diffs them; keep it until the old code is deleted.
- **Claim:** `build.rs` may depend on a TOML parser as a build-dependency without affecting the `no_std` target.
  **Confidence:** high
  **How to verify:** `cargo tree -e build -p cellos-kernel`.

## Related Code Files

- Create: `catalog/cells.toml`, `catalog/fixtures.toml`, `scripts/check-catalog.py`, `kernel/src/catalog.rs` (include of generated tables + lookup fns)
- Modify: `kernel/build.rs`, `kernel/src/task/cap.rs`, `kernel/src/loader.rs`, `kernel/src/loader/boot_ceiling.rs`, `kernel/src/loader/launch_profile/{mod,profiles,targets}.rs`, `kernel/src/policy.rs`, `kernel/src/task/drivers/pcie_ecam.rs`, `scripts/sign-policy.py`, `.github/workflows/ci.yml`
- Delete: `legacy_path_caps`, the `with_path_*` bodies, the scraper block in `sign-policy.py`

## Implementation Steps

1. Dump today's effective tables for every path literal in the kernel (one-off host test) to `reports/phase-05-tables-before.json`.
2. Transcribe to `catalog/cells.toml`; move fixture rows to `fixtures.toml`.
3. Generate tables in `build.rs`; route each lookup through `catalog.rs`, one table per commit, with the equality test green after each.
4. Switch `sign-policy.py` to the TOML; confirm the produced `POLICY.BIN` is byte-identical for the dev key.
5. Add `check-catalog.py` to the pre-build guards; fix or tier the five NIC rows it reports (empty IDs ⇒ not packaged, or IDs added).
6. Delete old tables and the equality harness's "old" side.

## Todo List

- [x] Before-dump
- [x] Catalog transcribed
- [x] Kernel lookups switched
- [x] Signer switched, byte-identical policy
- [x] Cross-check guard
- [x] Old tables deleted

## Success Criteria

- [x] Generated tables equal the before-dump for every path (equality test).
- [x] `POLICY.BIN` from the new signer is byte-identical to the old one for the same inputs.
- [x] `python3 scripts/check-catalog.py` fails when a path is in `config/services.toml` but not in the catalog (negative control).
- [x] Kernel compiles for x86_64, riscv64, aarch64; loader host tests pass.

## Risk Assessment

- A transcription error silently widens a ceiling: the equality test is the control; it must exist before any lookup is switched. Rollback: revert per-table commits; build-time only, nothing persistent.

## Security Considerations

The catalog becomes TCB input. It is reviewed like kernel source and covered by the signer's hash in the image manifest.

## Next Steps

Phase 06 changes the catalog's key from name to measured identity.

## Deviation Log

- Decision (owner, `rust-source-json-export`): the catalog source is `kernel/src/catalog.rs`; `scripts/catalog.generated.json` is exported by the `catalog_export` host test (stale file fails the test); no `catalog/*.toml`, no `build.rs` generation.
  Why: avoids hand-transcription into TOML and keeps per-row rationale next to the code that enforces it.
  Impact: `kernel/src/catalog.rs`, `kernel/src/catalog_export.rs`, `scripts/catalog.generated.json`.
  Revert: revert the phase commit; tables return to their former files.
- Decision: launch *edges* (caller → target) stay code in `launch_profile/profiles.rs`; only the path-keyed ceilings and requests moved.
  Why: edges are logic per caller and route, not per-path data; Phase 06 re-keys the caller.
  Impact: none on behaviour.
  Revert: n/a.
- Decision: the five packaged-but-inert NIC drivers and four edge-only/plan-only paths are recorded in `check-catalog.py` `OUTSIDE_CATALOG` with reasons instead of gaining rows.
  Why: adding rows would add authority; this phase is behaviour-preserving.
  Impact: `scripts/check-catalog.py`.
  Revert: n/a.
- Deviation: the equality harness was deleted after the comparison; its "before" dumps are kept in `reports/`.
  Why: once the old tables are gone a dump-vs-itself test only tests wiring.
  Impact: none.
  Revert: n/a.
- Deviation: files outside the S2 list touched as consequences: `tests/integration/fixtures/build-incomplete-policy.py` (calls the signer's `ptrust_paths` with the JSON path), `docs/code-metrics.generated.md`; two kernel tests that read source text were changed (`launch_profile` sweep now iterates catalog rows; the policy test no longer greps the signer for path mirrors that no longer exist).
  Why: direct consequences of removing the scraper and mirrors.
  Impact: listed files.
  Revert: with the phase.
- Surprise: the old P-TRUST scraper also counted `/bin/vfs` and `/bin/bcm-display` (literals in the `with_path_caps` body that mint no P-TRUST cap); the exported `ptrust` flag is exact. Both rows remain in DEV_POLICY, so the signed policy is unchanged.
  Why: scraper read literals, not semantics.
  Impact: `assert_ptrust_covered` now checks 17 paths, not 19.
  Revert: n/a.

## Evidence

[reports/phase-05-evidence.md](reports/phase-05-evidence.md).
