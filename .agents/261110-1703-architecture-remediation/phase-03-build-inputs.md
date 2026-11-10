---
phase: 3
title: "Explicit build inputs and single image recipe"
status: pending
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

- [ ] Explicit embed dir + manifest check
- [ ] CI switched, chain removed
- [ ] Binaries untracked
- [ ] Disk layout generated
- [ ] One recipe per arch
- [ ] Release builds and boots x86 ISO

## Success Criteria

- [ ] Building the kernel without `CELLOS_EMBED_DIR` and without `compile-only` fails with a message naming the variable.
- [ ] A tampered `kernel_fs.img` (one byte) fails the build on manifest mismatch.
- [ ] Old and new RV64 recipes produce identical per-partition SHA-256 for the same inputs.
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

None.
