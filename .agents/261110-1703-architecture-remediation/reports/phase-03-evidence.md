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

## Continuation 2026-11-10 — CI no longer reads tracked embed binaries

Changes (working tree):
- `kernel/build.rs`: `compile-only` without `EMBEDDED_OVERRIDE` reads nothing from the tree (empty `init` + VIFS1 stubs); an empty `EMBEDDED_OVERRIDE` counts as unset.
- `ci.yml`: `build` matrix — rv64 passes `EMBEDDED_OVERRIDE=kernel/src/embedded` (the ramdisk/init the job just built); aarch64/x86_64 rows are `compile-only` and no longer upload stub kernels (nothing downloaded them). `qemu-aarch64-boot` and `qemu-x86_64-boot` pass the directory they just assembled. The KSSH posture `cargo check` is `compile-only`. Every other kernel build in `ci.yml` already set `EMBEDDED_OVERRIDE` or used an isolated script.

Untrack simulation: every tracked `kernel/src/embedded*/{init,kernel_fs.img}` moved out of the tree, then (throwaway `/tmp/s1-lanes/untrack-sim.sh`):

| CI step reproduced | Result | s |
|---|---|---|
| rv64: `build-boot-ramdisk-ci.sh` | rc 0 | 28 |
| rv64 kernel, `EMBEDDED_OVERRIDE=kernel/src/embedded` | rc 0 | 23 |
| rv64 `qemu-boot-test.sh` | PASS "FAT16 mounted — kernel booted" | 55 |
| x86: `assemble-x86-image.py --profile headless --out-dir kernel/src/embedded-x86_64` | rc 0 | 25 |
| x86 kernel, manifest features, explicit override | rc 0 | 29 |
| `make-iso-ci.sh` (incl. `check_default_kernel.py`) | rc 0 | <1 |
| `qemu-x86_64-test.sh` | PASS "x86_64 shell prompt reached" | 90 |
| x86 kernel `compile-only`, `EMBEDDED_OVERRIDE=` (empty) | Finished | 29 |
| KSSH posture `cargo check … production-relay-image,compile-only` | Finished | 12 |
| kernel host tests `--features compile-only` | 600 passed | — |

Tracked binaries restored afterwards. Not reproduced locally: the aarch64 boot job (its inline step assembles `kernel/src/embedded-aarch64` from source and copies `app-init`, so it does not read a tracked image), the hypervisor jobs (scripts set `EMBEDDED_OVERRIDE` already), and the `boot-suite` `gen_disk.ps1` path (writes `kernel/src/embedded` before its kernel build).

Dev-only scripts still using the fallback chain (35 files, e.g. `run-x86.ps1`, `run.ps1`, `gen_disk.ps1`, `scripts/qemu-*-test.sh` RV64 lanes) keep working: after untracking, the fallback can only find images those scripts generated locally, and the build warns.
