---
phase: 3
title: "Explicit build inputs and single image recipe"
status: residual
priority: P1
effort: "6–9d"
dependencies: [2]
tier: medium
checkpoint: true
---

# Phase 03: Explicit build inputs and single image recipe

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-14, Medium build/repo](../reports/architecture-review-261110.md) · `kernel/build.rs` · `scripts/assemble-x86-image.py` · `gen_disk.ps1` · `scripts/gen-disk-ci.sh` · `scripts/gen-disk-aarch64-ci.sh` · `libs/api/src/abi/disk.rs` · `.github/workflows/release.yml` · [Red team](reports/red-team.md) #5

## Overview

Make the kernel binary a function of HEAD plus one named input directory, make release build what CI boots, and give each architecture one image recipe.

## Key Insights

- Reviewer-cited: `build.rs:163-187` picks the embed directory by a fallback chain and `:229-244` silently embeds a stub when the image is missing; CI overwrites the tracked directory in place (`ci.yml:956-976`); `release.yml:57-68` builds only an RV64 ELF.
- `.agents/TODO.md` records the committed `embedded-x86_64` image as deliberate and manifest-protected; removing it needs a replacement path first.
- Disk layout has a Rust source of truth (`libs/api/src/abi/disk.rs`) retyped as literals in seven tools.

## Requirements

- Functional: one explicit `CELLOS_EMBED_DIR`; missing image is a hard error unless a `compile-only` feature is set; `build.rs` verifies `manifest.json` hashes; release publishes an x86-64 ISO built by the CI recipe and booted once; RV64/AArch64 image assembly driven by one declarative cell list each; tools read a generated `disk-layout.json`.
- Non-functional: no change to image contents or layout values; PowerShell entry points remain as thin callers.

## Architecture

```
cells built+signed ──► target/embed/<arch>-<profile>/{init,kernel_fs.img,manifest.json}
                                   │  CELLOS_EMBED_DIR (required)
                                   ▼
                          kernel/build.rs: verify manifest, include_bytes!
```

- `disk-layout.json` emitted by a host test of `api::disk`; a literal-drift check modelled on `scripts/check-cargo-config-parity.py`.
- `scripts/assemble-image.py --arch {riscv64,aarch64}` with `scripts/images/<arch>.toml` cell lists; `gen_disk.ps1` and the two bash scripts become callers.

## Assumptions

- **Claim:** `EMBEDDED_OVERRIDE` already carries the same meaning in ~54 files and can be renamed mechanically.
  **Confidence:** medium
  **How to verify:** grep `EMBEDDED_OVERRIDE`; confirm each use passes a directory with `init` + `kernel_fs.img`.
- **Claim:** no lane depends on the stub-ramdisk behaviour except compile checks.
  **Confidence:** low
  **How to verify:** make the stub path a warning that prints the lane name for one CI cycle before making it an error.
- **Claim:** the RV64 PowerShell and bash recipes produce the same cell set today.
  **Confidence:** low
  **How to verify:** diff the cell lists both scripts build before unifying.

## Related Code Files

- Modify: `kernel/build.rs`, `kernel/Cargo.toml` (`compile-only`), `.github/workflows/{ci,release}.yml`, `gen_disk.ps1`, `scripts/gen-disk-ci.sh`, `scripts/gen-disk-aarch64-ci.sh`, `tools/write-mbr.py`, `tools/write-rpi3-mbr.py`, `scripts/format-disk-arm.sh`, `scripts/mksrv-img.sh`, `tools/cellos-fs-provision/provision.sh`, `.gitignore`
- Create: `scripts/assemble-image.py`, `scripts/images/{riscv64,aarch64}.toml`, `scripts/check-disk-layout-literals.py`
- Delete (untrack): `kernel/src/embedded*/{init,kernel_fs.img}`, `gh_2.55.0_linux_amd64/`, tracked files under `build/`, `limine/` objects and `.deb`

## Implementation Steps

1. Add `CELLOS_EMBED_DIR` to `build.rs` beside the current chain; warn when the chain is used; add manifest verification.
2. Point every CI job and script at `target/embed/...`; run one full CI cycle with both mechanisms.
3. Remove the fallback chain; stub only under `compile-only`.
4. Untrack embedded binaries and third-party artifacts; fetch `gh`, Limine and the WAD by checksum where a lane needs them; ignore `/build/` wholesale.
5. Generate `disk-layout.json`; switch the tools; add the literal check.
6. Write `assemble-image.py` for RV64, prove byte-identical partition contents against the old script, then AArch64; reduce old scripts to callers.
7. Release workflow: call the x86 recipe with the release key, boot once, publish ISO + manifest.

## Todo List

- [x] Explicit embed dir + manifest check (manifest check, explicit dirs in every CI kernel build; fallback kept as a warning — owner decision)
- [x] CI switched (no CI job reads a tracked embed binary); chain kept as a warning for dev scripts — owner decision 2026-11-10
- [ ] Binaries untracked
- [x] Disk layout generated
- [x] One recipe per arch — moved to [Phase 03b](phase-03b-single-image-recipe.md) (owner decision 2026-11-10)
- [ ] Release builds and boots x86 ISO

## Success Criteria

- [x] Building the kernel without `CELLOS_EMBED_DIR` and without `compile-only` fails with a message naming the variable. — replaced by owner decision: a warning naming `EMBEDDED_OVERRIDE` (dev scripts keep the fallback)
- [x] A tampered `kernel_fs.img` (one byte) fails the build on manifest mismatch.
- [x] Old and new RV64 recipes produce identical per-partition SHA-256 for the same inputs. — moved to Phase 03b
- [ ] Full suite at this checkpoint: all `pr`-tier lanes in `lanes.toml` green; `x86-c2c` job green; evidence in `reports/phase-03-evidence.md`.
- [ ] `git ls-files kernel/src/embedded*` lists no binary.

## Risk Assessment

- Untracking does not shrink history; a rewrite is out of scope and irreversible for clones — not planned.
- A lane silently relied on a stale committed image: step 2's dual run exposes it. Rollback: revert the commits; binaries return from history.
- Release artifacts already published cannot be recalled; this phase only changes future tags.

## Security Considerations

Manifest verification in `build.rs` prevents a stale or swapped userspace image from being embedded into a signed-posture kernel.

## Next Steps

Phase 04 adds a release-posture boot lane on top of this recipe.

## Deviation Log

- Deviation: kept the variable name `EMBEDDED_OVERRIDE` instead of renaming it to `CELLOS_EMBED_DIR`.
  Why: 99 files reference it (scripts, tests, docs, evidence); a rename changes no behaviour and would rewrite evidence text.
  Impact: plan wording only.
  Revert: n/a.
- Deviation: tools keep their literal layout copies; `check-disk-layout-literals.py` pins every site (47) to `api::disk` instead of making each tool read `disk-layout.json`.
  Why: shell/PowerShell readers would add a JSON dependency to every recipe; the drift check catches the failure the plan targets.
  Impact: new script + generated JSON + CI step.
  Revert: delete them.
- Deviation: the disk-layout source is read by a strict regex over `disk.rs` (`pub const PART_*: u64 = <literal>;`, all 12 required, contiguity re-checked), not a host test.
  Why: a host test would need a new crate outside `libs/api`; the format is fixed and the script refuses anything else.
  Impact: `scripts/check-disk-layout-literals.py`.
  Revert: n/a.
- Decision: release workflow left unchanged.
  Why: blocked — a release image needs release-signed cells; only the public key exists in CI.
  Impact: success criterion on release stays open; recorded in evidence.
  Revert: n/a.
- Surprise: `generate-code-metrics.py --check` already fails at `d32492e15`.
  Why: pre-existing drift.
  Impact: none from this phase; reported.
  Revert: n/a.

- Decision: step 6 (single recipe per arch) split into Phase 03b; `docs/code-metrics.generated.md` regenerated in this slice (it was already stale at `d32492e15`).
  Why: owner answers `split-later`, `regenerate` (2026-11-10).
  Impact: new phase file; generated metrics file (kernel nLOC 87,669 → 91,272).
  Revert: merge 03b back; regenerate.

- Decision (owner, 2026-11-10): the fallback chain stays a warning, not a hard error; untrack waits for a pushed CI cycle; `build/` evidence stays tracked until Phase 19.
  Why: 35 dev scripts use the fallback and cannot all be run here; after untracking the fallback can only find locally generated images. `build/` files are cited provenance.
  Impact: success criterion 1 replaced; untrack scope = embedded binaries, `gh_2.55.0_linux_amd64/`, `limine/` intermediates.
  Revert: n/a.
- Deviation: `build` job aarch64/x86_64 rows became `compile-only` and stopped uploading kernel ELFs.
  Why: those rows embedded a stub or a tracked image; no job downloads `kernel-aarch64`/`kernel-x86_64`.
  Impact: `ci.yml` build job.
  Revert: restore the two upload steps and drop the feature.
- Decision (owner, 2026-11-16, Phase 19): the plan closes with this phase as a residual. Binaries stay tracked until one pushed CI cycle has run; the release ISO stays blocked on release signing. Both items are in the [closure matrix](reports/closure-matrix.md) and the hand-back list in `docs/roadmap/current-focus.md`.
- Decision (2026-11-22): untrack done in `f830f377c5` after CI run 44921343700 on `124d4279d5` went green (30 jobs; KVM skipped) with 0 `EMBEDDED_OVERRIDE not set` lines across every job log. 1,501 paths left the index (7 embedded images, 194 `gh_2.55.0_linux_amd64/` files, 1,300 Limine `.o/.a/.d/.deb`); `.gitignore` covers them; `kernel/src/embedded-x86_64/manifest.json` stays tracked. The only item left in this phase is the release ISO, blocked on release signing.

## Evidence

[reports/phase-03-evidence.md](reports/phase-03-evidence.md) (partial).
