# PHASE02-MARKER dossier (implementation of phase02-marker-cp1.md)

Date 2026-10-30. Agent P2Marker. Approval: `approval.md` item (1) (D-P2-1..10 as recommended, incl. optional CI step and parity test; R1-R5; no frontmatter change; P01 pins not refreshed). HEAD at start `62dd1308c` (docs only after the CP1 binding `952d84c8a`). Host Python only; no cargo/QEMU/Docker/network/key. CHANGELOG.md (R5) is left to Main; the locks-file entry (R3) was already written by Main (grep found it at `.agents/cell-contract-0024-locks.md:127`, hash `77ea32c3...` differs from the CP1 "before" because of that) and was not touched.

## Hashes (SHA-256)

| Path | Before | After |
|---|---|---|
| `scripts/check-production-relay-image.py` | `5cb07d35ee48434e54aa6210590c74e473a2ea6da54c7640136af33f29af7d65` | `b0b973467527c3d05388cb1252dfbe0d8c43dd602e477df45f9f8a256876cd29` (= CP1) |
| `scripts/test_check_production_relay_image.py` | `b13e1c298fb6581fbd7dc33840fc08d241b74f1ed9f3608c16be3eadf8e74304` | `a1caa985c57ca6db0692be9546949b72bb55f33938f489ef97300b373577ce79` (= CP1) |
| `scripts/test_check_ssh_dev_persist_exclusion.py` (D-P2-3) | `d2d2231a5d11d9a6a0cacf0754c8f601416911c4b1d1539705f3b4a3fec5c6d3` | `ca2fa66701a93ff4f370f2636e13be55fc405f2470b32e472a044cbe8dda99e5` (= CP1) |
| `.github/workflows/ci.yml` (D-P2-4) | `731e5ffa99edb338222a0c9bc153069217dc89292821987dc9b8f1c682e62a9f` | `e9d8481856c24204f1e476ce6820bb2dd5bb1858666c4c94f9e792c6b9309bc1` (= CP1; 2253 -> 2258 lines) |
| `phase-02-private-protocol-and-dev-separation.md` (R1) | `ebc1ec1b658faf04e088e9e9de5ebe64922e934bcfde187e2001ecd9ef469e88` | `13b17fc88d7a95ebddaa13b580c29cde70b393714bcee4ea4923b8d6b8ce237c` |
| `.agents/260825-1726-kms-silo-production-root/plan.md` (R2) | `717ec671db795a73e52cae8afeccb2ce8df073446ce589350b830fa4cf5ee59d` | `af8b2558badeefcfe3df8cfd12138a6bd488cee4469e361070df9fedaffbd83f` |
| `.agents/261030-1000-kms-ssh-key-custody/plan.md` (R4) | `de0e5ea7b352250f9b90b5803cc0c09b3a7726e28c714f0cd137ae36cdc0e943` | `f5723ea6b4d921ab299a3693e4c67c123ea2acdf76d110a1eb90e0511ba86bd2` |
| `.agents/plan-portfolio.md` (KSSH state clause, CP1 11.5) | `d1e89ceb1c0433627fa68b4122e2127ff6fee6b9e6a734e669503c347a42ddd8` | `02f6498068b5077b8711f2a97859694ba8b7aeb7ba40b1b6809c2f0483fdbea8` |

Read-only inputs unchanged (re-hashed after edits): `scripts/build-production-relay-image.sh` `3ee7736b...0cda`, `scripts/check-ssh-dev-persist-exclusion.py` `71ceab71...8735`, Phase 4 `plan.md` `6e85c906...ab38`.

## Verification

| # | Result |
|---|---|
| V1 | `git status --short` clean at start; every "before" hash matched except `cell-contract-0024-locks.md` (Main's own entry, not in my closure). |
| V2 | `git diff --stat`: checker +5, production test +36, sibling test +13, ci.yml +5, Phase 02 doc +1, KMS-silo plan +1, KSSH plan +1, portfolio 1 line changed (+ locks/CHANGELOG are Main's). Nothing outside the closure. |
| V3 | `b0b97346...6cd29`, `a1caa985...7ce79` (match). |
| V4 | `python3 scripts/test_check_production_relay_image.py`: `Ran 10 tests ... OK` (baseline before edit: 8 OK). |
| V5 | `python3 scripts/test_check_ssh_dev_persist_exclusion.py`: `Ran 14 tests ... OK` (13 baseline, 14 with the parity test). |
| V6 | Stored K4b artifacts copied to `/tmp/p2m`, two distinct clean 24/30-byte net/kernel files: default KMS artifact exit **3** (ADR-0006 text); custody exit **1** (`DEV_POSTURE ssh-dev-custody`, `ssh-dev-custody`); persist exit **1** (`CELLOS-KSSH-T1-DEV-ROOT-v1`, `DEV_POSTURE ssh-dev-persist`, `ssh-dev-persist`); `--kms-features hardware-relay-provider,ssh-dev-persist` with default artifact exit **1** (`FAIL: KMS forbidden DEV marker: ssh-dev-persist`); synthetic kernel artifact carrying the persist banner exit **1** (`DEV_POSTURE ssh-dev-persist`, `ssh-dev-persist`). |
| V7 | `bash scripts/build-production-relay-image.sh x` with the three `CELLOS_PRODUCTION_ROOT_*` env: exit **3**, stdout empty, stderr = the ADR-0006 text. |
| V8 | `yaml.safe_load(ci.yml)`: 27 jobs, parses. `check-cargo-config-parity.py`: `PASS: 3 targets agree`. `bash scripts/check-board-configs.sh`: **exit 1** at its first step `check-hal-boundaries.sh` ("HAL boundary violation ... hal/arch/x86/src/x86_64/idt/dispatch.rs:10 `unsafe extern "Rust" {`"). Not caused by this slice: no `hal/` path is modified (`git status --short hal` empty) and the script does not read any file edited here. CP1 expected PASS; this is a pre-existing red (see deviations). |
| V9 | Verifier stays red (exit 1). Baseline (before edits, this session): `PACKAGE_SOURCE_DRIFT` 352, `CENSUS_SOURCE_DRIFT` 58, `DOCUMENT_LINK` 3. After: **354 / 58 / 3**. The two new `PACKAGE_SOURCE_DRIFT` entries are exactly `scripts/check-production-relay-image.py` and `scripts/test_check_production_relay_image.py`; nothing else new. |
| V10 | Phase 02 doc diff: one inserted body line in the Deviation Log (+1/-0), no frontmatter or checkbox line touched; `plan.md` of Phase 4 byte-identical (`6e85c906...`); `grep -c "^executionStatus\|^status:"` = 2 (plan.md) and 1 (phase 02), same as the unchanged frontmatter (no such lines in any diff). |
| V11 | Hosted CI: not runnable locally; **unobserved**. |

## Deviations from the CP1

1. V8 `check-board-configs.sh` is red (pre-existing HAL boundary violation in `hal/arch/x86/...`, outside this closure). `yaml` parse and cargo-config parity pass as expected.
2. V9: `CENSUS_SOURCE_DRIFT` did not rise (58 -> 58); CP1 predicted 60 (marked `[INFERENCE]` there). Only `PACKAGE_SOURCE_DRIFT` rose (+2 as predicted). Pins not refreshed (D-P2-6).
3. R3: not applied by me; the locks-file paragraph already existed (Main's combined "Cellbox follow-ups" paragraph, `cell-contract-0024-locks.md:127`). R5 (CHANGELOG) left to Main as instructed.
4. Plan-portfolio clause: CP1 11.5/5 names the portfolio KSSH state line (`:128`) without exact text; I appended one short sentence to that line (text in the file).
5. Commit/push: none made.

Temp files only under `/tmp/p2m` and `/tmp/p2m-p01-{before,after}.txt`.
