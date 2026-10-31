---
title: "Separate test and release cell-signing keys; replace the name-based cellbox guard"
status: completed   # 2026-10-31, phases 1-5 at QEMU/TCG ceiling; release key not yet provisioned in CI
priority: P1
created: 2026-10-31
blockedBy: []
related:
  - .agents/261020-1750-cellbox-cli/reports/rollout-containment.md   # owner of the current guard
  - .agents/cell-contract-0024-locks.md                               # kernel/src/{signing,loader} locked scope
---

# Test/release signing key separation

## Decision (user, 2026-10-31)

During alpha the **test (dev) key signs every native cell**, including `cellbox`, and test images admit them to Tier 1. At release each cell is placed in Tier 1 (release-signed) or Tier 2 according to its state. A test-signed artifact must never run as a release artifact. Enforcement uses the key and the kernel posture, not the file name.

## Observed baseline (source, 2026-10-31)

| Fact | Evidence |
|---|---|
| The kernel trusts the dev key only with `dev-signing-key`; without it, an all-zero placeholder that fails every verify | `kernel/src/signing.rs:32-49` |
| Valid signature → Tier 1 (unless FFI/UNTRUSTED manifest → Tier 2); wrong key → **denied**; unsigned → Tier 2 | `kernel/src/loader/governed_spawn.rs:60-100` |
| `default` features include `dev-signing-key`, `dev-policy-key`, `dev-weak-rng` and `signing-required` | `kernel/Cargo.toml:70` |
| `signing-required` is documented as "unsigned is denied", but spawn sends unsigned cells to Tier 2 | `kernel/Cargo.toml:88-91` vs `governed_spawn.rs:90-99` |
| The signer refuses the dev key for the names `cellbox`/`app-cellbox`/`terminal` outside the reviewed routes; 6+ lanes duplicate a private-seed workaround; the domain/lifecycle lanes ship `cellbox` unsigned | `scripts/cellos_sign/signing.py:36-41,231-250`; `scripts/qemu-phase0{2,3,4}*.sh`, `qemu-phase09-ssh-test.sh`, `qemu-tier3-ssh-guest-x86.sh` |
| No release key is provisioned | `signing.rs:45` `TODO(prod)` |

So a dev-signed `cellbox` is already refused by a kernel built without `dev-signing-key`. The name guard protects nothing that the key does not, except against one mistake: shipping a kernel that still trusts the dev key.

## Phases

| # | Phase | Change | Owner / lock | Law-1 |
|---|---|---|---|---|
| 1 | Release kernel posture | Named `release` feature set without any `dev-*` key/RNG feature; build-time release cell key (`include_bytes!` from a provisioned public key file, same pattern as `phase02-terminal-profile`); compile error if `release` and `dev-signing-key` are both enabled; boot log prints posture and key fingerprint | `kernel/src/signing.rs`, `kernel/Cargo.toml`, `kernel/build.rs` (locked: needs handoff) | no ABI; posture semantics review |
| 2 | Unsigned-cell rule for release | Decide D2; make `signing-required` behave as documented, or rename it to match the code | `governed_spawn.rs`, docs | semantics review |
| 3 | Signer guard by key, not name | Dev key may sign any name (test images). Release key: only from CI or a qualified route (existing `guard_prod_key` + `QUALIFIED_ROUTES`). Keep the quarantined-hash block unconditionally. Remove `CELLBOX_PRIVATE_FIXTURE_SIGNING` and the name list | `scripts/cellos_sign/**` + its tests | no |
| 4 | Lane migration | Every lane signs `cellbox` with the dev key through `sign_cells`; drop per-lane private seeds where they existed only for the name guard (lanes that test key rotation keep their own key); domain/lifecycle lanes go back to a signed `cellbox` (Tier 1) | lane scripts | no |
| 5 | Evidence | (a) dev-signed `cellbox` boots on the test kernel in Tier 1; (b) the same bytes on a `release` kernel are denied; (c) release-signed cell is admitted on `release`, denied on test kernel; (d) quarantined hash still refused; (e) host tests for the signer | runners + host tests | — |

Order: 1 → 2 → 3 → 4 → 5. Phase 3 must not land before phase 1: removing the name guard is safe only once a release build cannot trust the dev key.

## Decisions requested

- **D1. Release key custody:** CI secret (existing `guard_prod_key` CI markers) or external KMS/HSM; the public key file lives in the repo.
- **D2. Unsigned cells on a release kernel:** (a) admit to Tier 2 only (current code), or (b) deny (current docs). Your direction ("release cells go to Tier 1 or Tier 2 by state") fits (a) if a Tier-2 release cell may stay unsigned, or a stricter (c): Tier-2 release cells are also release-signed and placed by manifest class.
- **D3.** Retire the `x86-candidate-*`/`x86-canonical-*` route rule that the *dev* key may sign `cellbox`: once release builds use the release key, these routes sign with the release key only.

## Risks and rollback

- A kernel built with default features keeps trusting the dev key. Mitigation: phase 1's compile-time exclusion plus a release-artifact check that the key fingerprint is not the dev key.
- Rollout containment records (Cellbox Phase 10, S10-2a/2b) reference the name guard. Update them in the same change; quarantined hashes stay blocked.
- Rollback: revert the signer change; kernel phases only add a profile and do not change the dev posture. Signed artifacts already distributed cannot be recalled by a revert: their trust ends only when the kernel stops trusting the key.

## Result (2026-10-31)

| Check | Observed |
|---|---|
| Build: dev default / `--no-default-features --features release-posture,native-domains` / release + default features | OK / OK / build.rs refusal "cannot be combined with dev-signing-key" |
| Release kernel + dev-signed cells | `posture=release`, `/bin/vfs` and `/bin/nvme` refused `PermissionDenied`, no prompt |
| Release kernel + unsigned cells | same refusals, no prompt |
| Release kernel + release-signed cells | `Init: OS ready`, `Cellos >`; tier2-smoke/tier2-exploit/cpp-smoke/tier2-rpc-provider in Tier 2 (manifest class), others Tier 1 |
| Test kernel + dev-signed `cellbox` (domain lane) | `posture=dev`; no Tier-2 line for `cellbox` (Tier 1); lane PASS; lifecycle 2/2 |
| Signer/assembler/default-kernel host tests | 112/112 |

Release evidence used a throwaway key under `/tmp` (deleted), not a provisioned key.

Residuals:
- No release key is provisioned in CI yet; packaging routes still sign with the dev key (D3 deferred until then; editing the pinned assembler needs a re-pin).
- Embedded `init` is spawned by `spawn_trusted_init` without cell-signature verification in every posture: its integrity follows the kernel image (secure boot / kernel signing), not the cell key.
- `scripts/assemble-x86-image.py` docstring still mentions `CELLBOX_PRIVATE_FIXTURE_SIGNING` (pinned file; fix with the next re-pin).
- Riscv/aarch64 and other lane scripts not exercised here.
