# Phase 03 evidence — explicit build inputs (partial, 2026-11-10)

**Base:** `d32492e15` + working tree · **Ceiling:** host builds. Phase 03 stays **in progress**: two steps wait for a CI cycle (owner condition), one is blocked on a missing signing prerequisite, one is not started.

## Done

| Item | Check | Result |
|---|---|---|
| `build.rs` verifies `manifest.json` (`cellos-x86-image-manifest/1`) against `init` and `kernel_fs.img` with the kernel's own SHA-256 | build with the gate image dir | Finished |
| | negative: flip one byte of `kernel_fs.img` in a copy | build fails: "…kernel_fs.img does not match …manifest.json (stale or swapped embed input…)" |
| `build.rs` warns when no `EMBEDDED_OVERRIDE` is given (fallback chain) and when the VIFS1 stub is embedded; both silent under the new `compile-only` feature | build without override | `warning: … embedding the in-tree fallback src/embedded-x86_64 (deprecated …)` |
| | kernel host tests, default features | 600 passed, 0 failed (fallback warning shown) |
| | kernel host tests `--features compile-only --no-run` | builds, no warning |
| CI build-only kernels marked `compile-only` (RV64/x86 workspace clippy, aarch64 clippy, kernel host tests) | x86 workspace clippy with `--features cellos-kernel/compile-only` | rc 0, no fallback warning |
| Disk layout: `scripts/check-disk-layout-literals.py` reads `libs/api/src/abi/disk.rs`, re-checks contiguity, keeps `scripts/disk-layout.json`, checks 47 literal sites in 10 tools | run | OK |
| | negative: `CELLSTORE_BASE_LBA=1062145` in `gen-disk-ci.sh` | `FAIL … is 1062145, canonical 1062144`, exit 1 |
| Stale literal fixed | `tools/write-mbr.py` docstring total 931,072 → 1,127,680 | check passes |

## Not done

| Item | State | Reason |
|---|---|---|
| Remove the fallback chain (hard error without `EMBEDDED_OVERRIDE`) | waiting | Owner condition: one CI cycle on the warning first, to list lanes that still rely on the chain |
| Untrack `kernel/src/embedded*/{init,kernel_fs.img}`, `gh_2.55.0_linux_amd64/`, `build/`, `limine/` objects | waiting | same CI-cycle condition |
| One Python recipe per arch (RV64/AArch64) replacing `gen_disk.ps1` + `gen-disk-ci.sh` + `gen-disk-aarch64-ci.sh` | not started | ~2,000 lines of shell/PowerShell to port with per-partition byte-identity proof |
| Release builds, boots and publishes the x86 ISO | **blocked** | A release-posture image refuses unsigned cells and the workflow only has the release *public* key; signing cells needs the release private key or a signing service, which does not exist (TODO "Production trust keys"). No CI-only throwaway key can stand in for a published artifact |

## Pre-existing failure observed (not caused here)

`python3 scripts/generate-code-metrics.py --check` fails at `d32492e15` with the working tree stashed ("stale generated metrics"); the lint job's metrics step is already red at HEAD.
