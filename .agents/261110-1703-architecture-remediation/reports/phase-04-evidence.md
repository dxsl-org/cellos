# Phase 04 evidence — one trust posture, guarded copy always on

**Date:** 2026-11-10 · **Base:** `374658ac8` + working tree · **Ceiling:** host + x86/RV64/AArch64 QEMU TCG (local; CI not run — no push).

## What changed

- `kernel/src/posture.rs` (new): `derive(target_arch, feature_query) -> Result<Posture, reason>`; shared by `kernel/build.rs` (enforces, emits `cfg(cellos_posture = "dev" | "test" | "release")`) and the kernel host tests.
- `kernel/build.rs`: release kernels must provision `CELLOS_RELEASE_{CELL,POLICY,OWNER}_PUBLIC_KEY` (missing / zero / dev key refused); any release mix with a dev, test, recovery, fixture or measurement feature is refused; release requires `signing-required` + `policy-required`; every bootable 64-bit kernel requires `native-domains`.
- `kernel/src/policy.rs`, `kernel/src/admission/owner_key.rs`: release anchors come from the provisioned keys (no zero placeholder in release).
- `kernel/src/admission/gate.rs`: owner admission enforced under the release posture (owner decision `enforce-release`).
- CI: posture check and release workflow supply all three anchors; new step "Release-posture kernel fails closed" in `x86-c2c` (`scripts/x86/qemu-release-posture-test.sh`).

## Checks

| Check | Result |
|---|---|
| kernel host tests `--features compile-only` | 605 passed (5 new posture tests) |
| `cargo check … --no-default-features --features release-posture` (3 anchors) | Finished |
| `… release-posture,test-hooks` | refused: "kernel feature set refused: release posture cannot be combined with a development, test, recovery or fixture feature…" |
| `… release-posture,dev-policy-key` / `,maintenance-mode` / `,dev-weak-rng` | refused (same build-script failure) |
| `… release-posture` without `CELLOS_RELEASE_POLICY_PUBLIC_KEY` | refused: "requires CELLOS_RELEASE_POLICY_PUBLIC_KEY" |
| `… production-relay-image,compile-only` | Finished |
| dev features without `native-domains` (x86_64) | refused: "…require native-domains…" |
| kernel clippy `-D warnings`, `compile-only`, riscv64 / aarch64 / x86_64 | Finished ×3 |
| `scripts/x86/qemu-release-posture-test.sh` (x86 TCG) | PASS ×2: posture=release, unsigned=denied, dev POLICY.BIN rejected, `/bin/vfs` refused, init stops at "required VFS bootstrap failed", no `Cellos >` |
| discriminator: same markers on the dev boot log `qemu-x86_64.log` | `Cellos >` present, `posture=release` absent → lane would fail |
| `check-lanes.py`, `check-spec-anchors.py --check`, `generate-code-metrics.py --check`, mutable-globals `--check`, `cargo fmt --check` | OK |
| default-posture regression lanes | see § Regression below |

## Observed release behaviour (x86, throwaway anchors)

`init` is embedded in the kernel image and published as trusted; every Cell it then requests goes through the governed spawn and is refused (`SpawnFromPath refused: caller=1 path=/bin/block|nvme|ahci|virtio-scsi|pvscsi|vfs error=PermissionDenied`). This is the intended fail-closed state until (a) cells are signed with the release key and (b) a qualified backend installs an admitted owner slot.

## Not done in this phase (recorded)

- Deleting the raw-memcpy copy path: kept for the 32-bit targets (owner decision D4); unreachable on 64-bit because the build refuses a bootable kernel without `native-domains`.
- `validate_user_buf`: all 43 call sites reviewed; each precedes access through the copy view (`write_user_slice`, `read_user_string`, `view.read_into`, `write_bytes`, `preflight_user_output`, …). The five sites with `from_raw_parts` build kernel-side slices only. Kept as an argument pre-check.

## Regression (default posture, local, after the change)

| Lane | Result | s |
|---|---|---|
| two-node C2C kernel (`policy-required`) + ISO + two-node TCG | PASS (peer link-local evidence) | 40 |
| RV64 ramdisk + kernel + `qemu-boot-test.sh` | PASS "FAT16 mounted — kernel booted" | 89 |
| AArch64 `gen-disk-aarch64-ci.sh` + `qemu-aarch64-test.sh` (virt) | PASS "aarch64 shell prompt reached" | 260 |

The two lane scripts rewrite the tracked `kernel/src/embedded{,-aarch64}/init`; both were restored from HEAD afterwards.
