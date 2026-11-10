# Phase 05 evidence — single cell catalog

**Date:** 2026-11-10 · **Base:** `b78d47f31` + working tree · **Design:** owner choice `rust-source-json-export` (Rust module is the source; JSON is exported, not hand-written).

## What changed

- `kernel/src/catalog.rs` (new): one `Cell` row per install path — `request`, `boot`, `user_target`, `trusted_core` — plus the non-row rules (sandbox prefixes, observe/mutate request, legacy manifest-less fallback). 116 production rows; `test-hooks` and `phase04-driver-profile` fixture rows in separate cfg'd arrays.
- Former tables now delegate to it: `CapSet::with_path_caps` (and its four helper tables, deleted), `loader::boot_ceiling::lookup`, `launch_profile::targets::reviewed_user_target_ceiling`, `policy::is_trusted_core`, `loader::legacy_path_caps`. Launch *edges* (which caller may launch which target) stay code in `launch_profile/profiles.rs`.
- `kernel/src/catalog_export.rs`: host test that renders the production catalog (+ Ethernet driver IDs) to `scripts/catalog.generated.json` and fails when the committed file is stale.
- `scripts/sign-policy.py`: reads the JSON; the `cap.rs` source scraper and the path mirrors (`SANDBOX_PREFIXES`, `CONFIG_WRITE_PATHS`, `KMS_SSH_*_PATHS`, `OBSERVE_MUTATE`) are gone; new `assert_net_rights_match_kernel`.
- `scripts/check-catalog.py` + CI step: every `/bin` path in `config/services.toml` and in committed image manifests has a catalog row or a stated reason.

## Equality proof

A temporary host-test harness dumped, for 163 paths (every path literal in the old tables plus sandbox/unknown probes): the request, P-TRUST flag, boot row, effective boot ceiling, trusted-core flag, legacy fallback, and every launch edge for 20 caller names × spawn/supervisor flags × 4 routes (2,664 edge lines). Dumps were taken from the unmodified tree and after the refactor:

| Feature set | Lines | `diff` |
|---|---|---|
| default + `compile-only` | 3,642 | 0 |
| + `test-hooks` | 3,654 | 0 |
| + `phase04-driver-profile` | — | 0 |
| + `phase03-filter-profile` | — | 0 |

The "before" dumps for the first two are kept as `reports/before-default.txt` and `reports/before-hooks.txt`. The harness was deleted after the comparison (its job is done; `catalog_export` and the launch-profile sweep test guard the tables now).

`POLICY.BIN` from the new signer is byte-identical: SHA-256 `97e8d5a9…cdb27` (dev) and `fab8ee66…4521b` (`--maintenance`), before and after. `tests/integration/fixtures/build-incomplete-policy.py` (omit a P-TRUST row) still builds.

## Checks

| Check | Result |
|---|---|
| kernel host tests `compile-only` / `compile-only,test-hooks` | 606 passed / 606 passed |
| kernel clippy `-D warnings`, riscv64 / aarch64 / x86_64 | Finished ×3 |
| `python3 scripts/check-catalog.py` | OK (116 rows, 53 paths checked, 9 outside with reason) |
| negative: add `/bin/zz-unknown` to `config/services.toml` | `FAIL … has no catalog row` |
| `sign-policy.py` net-rights assertion | holds for every DEV_POLICY row |
| regression lanes | see § Regression |

## Findings surfaced by the cross-check (recorded, not changed)

- `/bin/igc`, `/bin/ixgbe`, `/bin/i40e`, `/bin/r8169`, `/bin/bcm57xx` are packaged in the canonical x86 image with no catalog row and no Ethernet ID: inert. Listed in `OUTSIDE_CATALOG` with that reason (x86 breadth S5 owns them).
- `/bin/ai` is init-launched with the empty boot ceiling; `/bin/kill` and `/bin/shutdown` get their ceilings from cellbox launch edges; `/bin/intel-gpu` is a service row no image packages yet.

## Regression (local, after the refactor)

| Lane | Result | s |
|---|---|---|
| x86 default image (manifest features) + `qemu-x86_64-test.sh` | PASS shell prompt; boot log `boot-ceiling self-test PASS (per-path table, no union collapse)`, `P-TRUST self-test PASS` | 121 |
| two-node C2C TCG | PASS | 40 |
| release-posture lane | PASS (fails closed as designed) | 90 |
| x86 Tier-2 domain build + test | PASS | 166 |
| RV64 ramdisk + kernel + boot | PASS | 87 |
| AArch64 disk + boot (virt) | PASS shell prompt | 224 |
