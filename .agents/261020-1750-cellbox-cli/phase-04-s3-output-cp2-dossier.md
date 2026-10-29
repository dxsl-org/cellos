# S3-output CP2 dossier — implementation review request

**Status: CP2 APPROVED by the user on 2026-10-22 (option B: “CP2 baseline + soạn CP1 GUI proof”). Baseline only; quarantine, packaging, signing, GUI acceptance and rollout remain blocked.** Prepared 2026-10-22 from saved evidence; the only new execution was hashing. Parent: [S3-output proposal](phase-04-s3-output-proposal.md).

## What the user's message does and does not settle
The user wrote "duyệt GUI và rollout, xem xét CP2 cho slice S3-output". Recorded as: (1) intent to proceed toward GUI proof and rollout; (2) request for this CP2 review. It is **not** treated as CP2 approval of unobserved behavior, and cannot by itself reopen rollout: [rollout containment](reports/rollout-containment.md) requires a reviewed source change that reopens only qualified routes, plus CP2 approvals against exact evidence for every affected criterion. Those conditions are not met by this slice alone (see "Gaps").

## Authority chain
Base CP1 (user, 2026-10-22) -> [free GetTime amendment](phase-04-s3-output-deadline-amendment.md) -> [private echo edge](phase-04-s3-output-negative-grant-amendment.md) -> user approval "Duyệt shell và halt proof" (cancel/exact-status fix and Phase03 CPU-halt proof; recorded in the [evidence report](reports/s3-output-runtime-evidence.md)).

## Evidence ([report](reports/s3-output-runtime-evidence.md), sanitized copies in `reports/s3-output-evidence/`)
- Private signed x86 image (TCG): S3 runner **52/52 driver markers, 308/308 independent checks**; AC-1..AC-8 checked.
- Regression: Phase02 normal 48/48, kernel AP lane 37/37, Phase03 60/60 with DWARF-bound CPU halt proof (HLT=1, CPL0, IF=0), host suite **417 pass** (baseline 403).
- Independent reviews: helper/caller review no findings; final review of cancel/exact-status/halt proof no findings; fixture review findings (3) fixed before the final run.

## Gaps (CP2 cannot honestly cover these)
1. **GUI:** no GUI/compositor/pixel/resize proof. The Terminal app is only compile/unit tested. A desktop fixture ISO exists in `build/` but was not exercised for this slice.
2. **Rollout:** canonical `cellbox`, `terminal`, `kernel_fs.img` remain quarantined; packaging/signing guards still refuse. Other Phase 04 criteria (S2b/d/e, S4, multicall retirement) and CBX rows stay pending.
3. Boot selftest `COMPLETION-QUEUE: FAIL` (pre-existing, not attributed, not fixed).
4. Not exercised on an image: date/env/uname/kill/hotswap/shutdown banner stdout; invalid-reference helper execution (admission refuses first); riscv64/aarch64 runtime; KVM; wall-clock races.
5. The working tree has 315 changed paths from earlier phases; this inventory lists only S3-output files and does not review them all.

## File inventory (current SHA-256)
| Path | Bytes | SHA-256 |
|---|---:|---|
| `libs/ostd/src/io.rs` | 19284 | `13d7f25435f45e147c64c8b5c1ce8d3de171eda3c92d2ddd2fb98ec8e0ec5029` |
| `libs/ostd/src/session.rs` | 30187 | `ccc432013fd6188a6d4489faf1188f0b334815603e0a53fb1eed804ffcced704` |
| `cells/tools/sys-tools/src/bin/cat.rs` | 1437 | `8332884bbb8a13a1aad5605611d96f8e1fe835daaa83b4128aebec5af38a13fe` |
| `cells/tools/sys-tools/src/bin/date.rs` | 4012 | `307937adee708cf73ca8b06f5e2430c72964a8c63ee9ee00ace2dba2db5953e6` |
| `cells/tools/sys-tools/src/bin/echo.rs` | 945 | `c9c99db784299a48ab677bc43179ed110c1f924a8d34abdc74a4829477be2494` |
| `cells/tools/sys-tools/src/bin/env.rs` | 759 | `65118cfb00f8d2a41cb3aebacf7ce12f731df4060549546e2da8b430d441347d` |
| `cells/tools/sys-tools/src/bin/free.rs` | 2799 | `7a7158913b191cce13bc94b51580f9d7f7436f9b6f5ae1af8011748b92f3bfcf` |
| `cells/tools/sys-tools/src/bin/hotswap.rs` | 8664 | `9b036d3718f405d96eef4bbb5c6dc5fbf161c9b171338e6bb57bd915649bcf6f` |
| `cells/tools/sys-tools/src/bin/kill.rs` | 2934 | `ae38c38551938e31769e2f2cd97ec4cd1749707be41b9f587bd5156fecbe73e0` |
| `cells/tools/sys-tools/src/bin/ls.rs` | 1435 | `3018d734359e686bb9a1680faf1f6f34f26c320c315388774a9488fe5a10c6f6` |
| `cells/tools/sys-tools/src/bin/ps.rs` | 2001 | `6ef259deca728f924b5c04883c6fed13262aa9ca0611e338593dc7eb93b923e6` |
| `cells/tools/sys-tools/src/bin/shutdown.rs` | 1024 | `14fd32d00fa15172ebaa72697896beaf1b42c712c8c5e4016fbb37f70a27dff7` |
| `cells/tools/sys-tools/src/bin/uname.rs` | 947 | `d433db2fa5fd4f36833dc0764d766dad3b515dfbed3942e1251bce8d10bf0c70` |
| `cells/tools/cellbox/src/executor.rs` | 131742 | `28278b5e2488a5adc144620d2e19407fb8c41ce98a705c4a699f8a5879b1a614` |
| `cells/tools/cellbox/src/shell_state.rs` | 8749 | `20b4b89151562593003ebc10d81da82c2e51a6c4d219b64dcf737e11c3104ec4` |
| `kernel/src/loader/launch_profile/profiles.rs` | 13627 | `cd51eb0a41abe091e3077b8848e6cbc5f9f70f7be9d3e310970da244dc50ec54` |
| `kernel/src/loader/launch_profile/tests.rs` | 19559 | `a60748c827fd44a68befdb0f503cd7f7bad0fd6457efcdf742de6cae31848d47` |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 41380 | `ec167edc8be3f8cd694c8bae647ecdb535d7d4cef9775d27d6dae8da57f0c06b` |
| `scripts/qemu-phase04-terminal-test.sh` | 34917 | `bcce7aaddd963838f1a3c7af534f7d70c7f48c16354036f42f84c375948c89aa` |
| `scripts/qemu-phase03-pipeline-test.sh` | 48013 | `166bce2a2e120e88b79ce513fb8bf2dc7a1b73750595813522b01b44b43d2f1f` |

Quarantined artifacts, unchanged: cellbox `64eb28f3be3d08bf2560d6eb0b58bee4a33ff8ea3439bcd5ed6e1ccc42f84a39`, terminal `636497acc2bdf735471173c5c428e61d16b1d8c870935cfe868e6d0120e52fa2`, `kernel_fs.img` `447d0348735a25e8b396c11c0091ff3d214b0c0b392043bb0515d4104a298e2c`.

## What CP2 would authorize (proposal)
Accept the files above as the S3-output implementation baseline, so later Phase 04 slices may consume `ProcessOutput`, the migrated stock tools and the exact-status/interrupt shell behavior. It would **not** lift the quarantine, packaging, signing or rollout guards, and would **not** tick the GUI criterion or any CBX row.

## Options for the user
A. **CP2 baseline only.** Accept as above; GUI and rollout stay blocked.
B. **CP2 baseline + GUI proof slice.** As A, and authorize drafting a separate CP1 for a desktop-fixture GUI Terminal proof (private image, no canonical writes). Rollout stays blocked.
C. **Not yet.** Withhold CP2 until GUI proof exists.
Rollout itself is not an option here: it needs the containment release conditions, which this slice cannot satisfy.

## Rollback
Revert the inventory files and the feature-gated echo row together; no persisted state. Bytes already written to streams or files cannot be recalled.

## Decision recorded
Option B: the inventory above is the S3-output implementation baseline. A separate [GUI proof CP1](phase-04-s3-gui-proof-proposal.md) is to be drafted; it is not approved by this record.

## GUI/signing/drain-order addendum (2026-10-22; outside approved baseline)

**The CP2 baseline and inventory above remain exactly as approved (option B).** The later user-approved [GUI CP1](phase-04-s3-gui-proof-proposal.md), including guarded private Terminal signing, and [drain-order option 1](phase-04-s3-terminal-drain-order-proposal.md) produced the following additional files/changes. **These files are NOT covered by the earlier S3-output CP2 unless the user explicitly approves them.** Listing hashes or successful private tests is not that approval.

Current source bytes were hashed from disk for this addendum; no build/test was run by the documentation worker:

| Additional path | Bytes | SHA-256 |
|---|---:|---|
| `scripts/cellos_sign/signing.py` | 8328 | `e1ec59c35887d6ea14a5631c565d9307121b91d5a4fed329be0045b48063b985` |
| `scripts/test_cellos_sign.py` | 27680 | `bfb3d6dc620c8de817ccce65c82843a5704ddd6d3c8df3f9d145b4588fc46fee` |
| `cells/apps/terminal/src/session.rs` | 13568 | `0377b432bddcde47177c8938cced42c99ef108183b60faa8902a9ed91905da0f` |
| `scripts/qemu-phase04-gui-test.sh` | 22241 | `795df7ff2880f19ff10d7f27367b36329764030d2cf74a5d3f14eecb07e701e7` |
| `scripts/phase04_gui_drive.py` | 28907 | `8d2c264021a1bd6bc06d2277aff6321ba3a2b651c28e9f12a9dc5260511946f9` |
| `scripts/phase04-gui-glyphs/Cargo.toml.in` | 442 | `ff464ceb96d8740d5a7f87498b9a7851bcd21d4cfa4eac2cade0fd5e98063215` |
| `scripts/phase04-gui-glyphs/src/main.rs` | 3196 | `79967ee3da85c771a0e5b547f3cc2ac20f154c185a691fdcd2dae5a1be7824ae` |

New evidence: [GUI runtime report](reports/s3-gui-runtime-evidence.md), run3 58/58 and fixed run4 **103/103**, including **10/10** exact stderr-before-prompt screens, real taskbar launch/keyboard/rendering, stock stdout/stderr, Ctrl-C `cc=130`, UART negatives and unchanged saved canonical hashes. GUI rendering is now proven on the **private TCG fixture**, superseding gap 1 above only at that scope; resize/focus/multi-window/KVM/physical Intel/VMware remain unexercised. The original run1 ordering defect and separate unconsumed-pipeline OOM remain saved/reported.

The signing exception is only `{cellbox, app-cellbox, terminal}` with `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` plus an explicit non-dev seed; quarantined byte hashes remain unconditionally refused and canonical packaging guards remain closed. Parent observed **49 guard tests OK**, Terminal/terminal-sdk **33/49 pass**, bare-metal check clean, plus signing-dependent private S3/Phase03/Phase02 runners passing as detailed in the report. **Those newer test/regression results are parent-observed counts only, without archived raw logs**; they are not represented by the older baseline captures.

The user's “duyệt rollout” does not meet the containment release conditions. Other CBX rows, S2b/d/e, S4, canonical qualification and affected CP2 decisions remain open; **nothing was unblocked and rollout stays BLOCKED**. Historical approval and artifact observations above are preserved, not broadened into GUI/signing CP2 or full Phase04 acceptance.
