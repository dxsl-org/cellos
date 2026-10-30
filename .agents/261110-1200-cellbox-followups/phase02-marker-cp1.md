# PHASE02-MARKER CP1: five KSSH closed marker names into the Phase 02 production-checker frozen set (KSSH `ci.yml.md` item 3 option B1; K4b D-4b.13 / K5 hand-off) (final text; decisions in section 16; documents only; nothing applied)

Written 2026-10-30 at HEAD `952d84c8a` (`git status --short` empty at binding). Documents only: no source, script, workflow, plan, lock, `Cargo.toml`/`Cargo.lock`, changelog, image, key, network, build, QEMU or Docker action was made to produce this file. Everything cited as `file:line` or as a hash was read or computed in this session; `[INFERENCE]` marks anything not observed in a file or command output; `OPEN (decider)` marks a decision for the named owner. Form follows `.agents/261110-0900-tier3-guest-endpoint/net-owner-death-cp1.md` (lock packet = its section 11, itself the form of `lock-handoff-k2-cp2.md` / `k3-cp1.md` section 11).

Inputs read: `.agents/261030-1000-kms-ssh-key-custody/cleanup/ci.yml.md` (SHA-256 `fdbc8e10…ea14f`), `cleanup/approval.md` (`69c4421a…`), `k4b-cp1.md` (`e44e3017…3fba`; sections 5, 11, 14, D-4b.13) and `k4b-cp1-approval.md` (`c1d70458…`), `reports/k5-cp2-dossier.md` (`18389fb1…`, line 162), `.agents/260826-1605-phase4-dev-reference-authority/{plan.md (6e85c906…ab38), phase-02-private-protocol-and-dev-separation.md (ebc1ec1b…9e88), phase-03/04/05/06/08}` (full text of phase 02; the checker/marker passages of the others), `.agents/cell-contract-0024-locks.md` (`1f674e01…74c0`), `.agents/plan-portfolio.md` (`d1e89ceb…ddd8`), `.agents/261110-0900-tier3-guest-endpoint/net-owner-death-cp1.md` (`c40fdf98…`), commit `0d05633f0` (the 2026-08-31 precedent), `scripts/check-production-relay-image.py`, `scripts/test_check_production_relay_image.py`, `scripts/build-production-relay-image.sh`, `scripts/check-ssh-dev-persist-exclusion.py` and its test, `.github/workflows/ci.yml` (job `kms-ssh-dev-posture`, `:1741-1811`), `cells/services/kms/src/{main.rs,ssh/persist/devroot.rs}`.

**Name check.** The brief names the sibling checker `scripts/check_ssh_dev_persist_exclusion.py`. The file is `scripts/check-ssh-dev-persist-exclusion.py` (hyphens; its test is `scripts/test_check_ssh_dev_persist_exclusion.py`, underscores). Only the hyphenated checker exists (`glob scripts/`).

## 0. Summary

* **What is needed (B1, exact).** One hunk in `scripts/check-production-relay-image.py`: insert five closed ASCII names into `FROZEN_DEV_MARKERS` (+5 lines, sorted position), nothing else in that file. `KMS_FORBIDDEN`, `scan_artifact`, `require_exact_posture`, `main`, exit codes and the ADR-0006 text are untouched (section 3).
* **Tests.** The existing test file already iterates `CHECKER.FROZEN_DEV_MARKERS` for feature rejection, artifact-content rejection and 64 KiB chunk-boundary split, so the five names are covered with no edit. Following the 2026-08-31 precedent (`0d05633f0`) CP1 adds **two** tests to `scripts/test_check_production_relay_image.py` (a frozen-membership test, and a realistic-banner end-to-end test across all three artifact slots). Observed on `/tmp` copies: baseline 8 tests OK; patched 10 tests OK; patched tests against the *unpatched* checker fail 7 sub-assertions (section 7).
* **Does `status: blocked` forbid it?** The status is not a mechanical write lock (nothing in the repo checks it against edits; section 4). It does mean the edit needs the accountable owner's explicit file-bounded adoption, exactly as K2 CP2/K3/K5/K7/K4b/NET-OD obtained it. The ownership rule (`phase-02:46,74`; `plan.md:57`) and the Phase 6 "Correction" (`phase-06:183-187`, a non-owner checker edit was reverted) make a *silent* edit by anyone but the owner a breach. With the packet of section 11 approved, the edit is inside the rules; without it, it is not. This text asks and applies nothing.
* **Why now.** The approved cleanup item 3 chose B0 (text hand-off only, `cleanup/approval.md:7`). The user has since ordered the leftover follow-ups completed, which supersedes B0 with B1 (D-P2-1). The gap B0 left is real and demonstrated: a production-shaped candidate whose KMS artifact is the custody or persist build exits **3** (the normal ADR-0006 block) instead of **1** (`ci.yml.md` F8, re-run in section 2 F2 on the stored K4b builds).
* **Production-image pipeline: none exists** to attach `--expect none` to (section 9). What is already wired: CI job `kms-ssh-dev-posture` (landed; `ci.yml:1741-1811`) which proves the three KMS variants with the sibling checker in nine both-direction rows. One optional extra CI step (run the production checker's own tests; `ci.yml` +5 lines) is proposed (D-P2-4).
* **Pins.** The checker and its test are digest-pinned in the Cell-contract P01 manifests (`exact-deltas/manifest.json:13568-13569,14004-14005`, `consumer-census-source.json` `consumer_inputs[3287],[3396]`); the P01 verifier is already red at HEAD (352 `PACKAGE_SOURCE_DRIFT`, 58 `CENSUS_SOURCE_DRIFT`, 3 `DOCUMENT_LINK`, observed now). B1 adds drift for these two paths; pins are not refreshed by this item (section 5, D-P2-6).
* **Effort: XS** (three edited paths for B1, about 60 changed lines in total; a few seconds of host Python for all verification; no cargo, no QEMU).

## 1. Scope and non-scope

In scope: add the five closed names handed off in `k4b-cp1.md` section 5 item 5 and `k5-cp2-dossier.md:162` to the Phase 02 frozen marker set; test them; record the hand-off and the reciprocal notes; state what CI wiring exists.

Out of scope (each also forbidden by section 11.6): any other change to `scripts/check-production-relay-image.py` (no redesign, no new option, no provider/feature policy change, no change to `KMS_FORBIDDEN`/`NET_FORBIDDEN`/`KERNEL_FORBIDDEN`), `scripts/build-production-relay-image.sh` (a stub that exits 3), the sibling checker (unchanged; it stays as the variant-identity proof for K8 and CI), any KMS source, `libs/**`, `kernel/**`, images, signing, `config/`, `Cargo.lock`, any hardware/AWS/production gate, ADR-0006 (the block text and exit code 3 stay).

## 2. Findings (evidence)

| # | Finding | Evidence |
|---|---|---|
| F1 | The five names are closed ASCII strings produced by exactly two source sites: banners and the dev-root label. `CELLOS-KSSH-T1-DEV-ROOT-v1` is the first 26 bytes of `DEV_ROOT_LABEL` (55 bytes). Banners: custody `"[kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf"`, persist `"[kms] DEV_POSTURE ssh-dev-persist: wrapped identity persisted to /srv, dev root key co-resident, no rollback or crash-durability claim"`. The two feature names are substrings of the banners. | `cells/services/kms/src/main.rs:19-24` (`7fc9a2ba…bdf516`); `cells/services/kms/src/ssh/persist/devroot.rs:11-12` (`5d5d9b2a…`); sibling checker `scripts/check-ssh-dev-persist-exclusion.py:32-42` (the same five, order = report order). |
| F2 | **The gap B1 closes, re-observed.** Production checker on `/tmp` copies, valid net/kernel posture (`verified-tls,tls-roots-embedded,tls-ca-private`, `production-relay-image`), `--kms-features hardware-relay-provider`, KMS artifact = the stored K4b CP2 x86_64 builds `/tmp/k4b-art/after-x86_64-unknown-none-{default,ssh-dev-custody,ssh-dev-persist}.elf` (sha256 prefixes `7b8779ee…`, `572fe092…`, `65a05a28…`): **unpatched** exit `3`,`3`,`3`; **patched** exit `3` (default), `1` (custody; `contains forbidden DEV marker: DEV_POSTURE ssh-dev-custody` and `…: ssh-dev-custody`), `1` (persist; `CELLOS-KSSH-T1-DEV-ROOT-v1`, `DEV_POSTURE ssh-dev-persist`, `ssh-dev-persist`). `--kms-features hardware-relay-provider,ssh-dev-persist` with the default artifact: unpatched `3`, patched `1` (`FAIL: KMS forbidden DEV marker: ssh-dev-persist`). A synthetic kernel artifact containing the persist banner: unpatched `3`, patched `1`. Sibling `--expect none` on the same three artifacts: `0`, `1`, `1` (unchanged by B1). | `/tmp/p2b1/{a,b}` copies of the two Phase 02 scripts (repo untouched); the K4b dossier recorded the same three artifacts (`ci.yml.md` F8). The stored artifacts are the K4b CP2 evidence builds, not re-built at HEAD. |
| F3 | **No false positive observed on clean builds.** Counting the four strings (`ssh-dev-custody`, `ssh-dev-persist`, `CELLOS-KSSH-T1-DEV-ROOT-v1`, `DEV_POSTURE ssh-dev`) with `grep -c -a -F` gives `0 0 0 0` for the default KMS builds on riscv64/aarch64/x86_64 (`/tmp/k4b-art/{base,after}-*-default.elf`, six files), for `target/x86_64-unknown-none/release/{service-net,cellos-kernel}` and for `kernel/src/embedded-x86_64/{init,kernel_fs.img,manifest.json}`. The only non-KMS strings near the names are the admin cell's `"DEV_POSTURE volatile: …"` / `"DEV_POSTURE persistent: …"` (`cells/tools/kms-ssh-admin/src/report.rs:56-61`), which are **not** among the five (they have no `ssh-dev-` word) and are not matched. | Observed now; the local `target/` files are whatever build state is present (not a provenance claim). No kernel/net/libs/boards/config source contains any of the five (`grep` over `kernel;cells/apps;cells/tools;cells/tests;libs;boards;config;docs`: only `kms-ssh-admin/{Cargo.toml,src/tests/real.rs}` feature-dependency text and `docs/code-standards.md`, none linked into a production artifact). |
| F4 | The checker scans **three** artifacts (KMS, net, kernel), so B1 also rejects a kernel or net artifact that embeds a KSSH build; the sibling checker scans the KMS artifact only. Whether a production kernel embeds KMS bytes is [INFERENCE] (the kernel build embeds `kernel/src/embedded-<arch>/`, `kernel/build.rs:92-123`; the local x86_64 embedded image does not contain the KMS banner string `fail-closed node identity service starting`: `grep -c` = 0). | `scripts/check-production-relay-image.py:91-109,125-148`. |
| F5 | Existing marker names already overlap by substring (`root-stream` ⊂ `vf2-root-stream`; `dev-reference` ⊂ `dev-reference.toml`). Adding `ssh-dev-custody` ⊂ `DEV_POSTURE ssh-dev-custody` follows the same pattern; the content test asserts `assertIn` on each name so a payload with the longer name reports both names. `overlap = max(len) - 1 = 26` (`stm32h573i-dk-dev-authority` is 27 bytes and stays the longest together with the new `DEV_POSTURE ssh-dev-persist`, also 27; `DEV_POSTURE ssh-dev-custody` is 27, `CELLOS-KSSH-T1-DEV-ROOT-v1` is 26), so the chunk-overlap arithmetic needs no change. | `scripts/check-production-relay-image.py:40-58,95`; lengths computed. |
| F6 | Phase 02 owns the checker and test and consumes handed-off marker names into its own frozen set. Precedent: Phase 2 consumed Phase 5's `SOFTWARE_HARNESS` handoff on 2026-08-31 as commit `0d05633f0`: **three** paths: `scripts/check-production-relay-image.py` (+1 line), `scripts/test_check_production_relay_image.py` (+7 lines, one frozen-membership test), and a one-line Deviation Log entry in `phase-02-private-protocol-and-dev-separation.md` (`:128`). The same plan **reverted** a non-owner checker edit by Phase 6 (`phase-06:183-187`: "That breached the serialized ownership contract … Phase 2 remains the sole checker owner"). | `git show 0d05633f0`; `phase-02:46,74,128`; `plan.md:57`; `phase-03:89`; `phase-04:245`; `phase-05:95,195`; `phase-06:151,183-187`. |
| F7 | The handoff rule's wording covers "Phases 3-7 … through their reviewed manifests"; KSSH is outside the 3-7 phases. The same sole-owner sentence ("This phase remains the sole owner of `scripts/check-production-relay-image.py`") is what K4b CP1 recorded as C-K4b-3 and the user confirmed in `k4b-cp1-approval.md:3` ("rulings C-K4b-1..3 as recommended"). The KSSH "reviewed manifest" for the names is the approved K4b CP1 section 5 item 4-5 plus the sibling checker constants (`k4b-cp1.md:214-215`). | `phase-02:46`; `k4b-cp1.md:42,215,302,328`; `k4b-cp1-approval.md:3`. |
| F8 | Phase 02 frontmatter is `status: blocked` (`phase-02:4`), body "Status: BLOCKED on completed Phase 1" (`:23`), while its todo list (`:89-95`) is ticked and the last Success Criteria line is the only open box (`:104`), and the log records "SOFTWARE_HARNESS complete" (`:126`, `:129`; `plan.md` table "blocked; SOFTWARE_HARNESS complete"). The plan carries CELL-CONTRACT-0024 (`plan.md:9-14`; `locks.md:159`: "blocked … P01 then P08/P10; explicit per-slice handoff"). `scripts/verify-cell-contract-p01.py:203` **requires** `executionStatus: blocked` + a `lockScope` in the plan: notes must not alter the plan frontmatter. | files cited. |
| F9 | The checker and its test are digest-pinned in the P01 manifests (checker `5cb07d35…af7d65`, test `b13e1c29…74304`): `exact-deltas/manifest.json:13568-13569,14004-14005`; `consumer-census-source.json` `consumer_inputs[3287]`, `[3396]` plus per-symbol line maps in `occurrences[3157]`, `[3265]`. `scripts/verify-cell-contract-p01.py` at HEAD: 352 `PACKAGE_SOURCE_DRIFT`, 58 `CENSUS_SOURCE_DRIFT`, 3 `DOCUMENT_LINK` (counts observed now by `grep -c`; neither script is among the drifted paths today, so each is currently pinned correctly). | read-only run now. Same effect and same policy as `cleanup/completion-queue.md:220`. |
| F10 | The production test file also exercises `scripts/build-production-relay-image.sh` (exit 3, stderr == ADR text, needs `cells/services/kms/src/storage/capability.rs` present, `test_check_production_relay_image.py:169-191`). B1 does not touch the builder; the builder test must stay green. | observed 8/8 OK at HEAD (run in the repo, read-only). |
| F11 | The production checker's tests are **not run in CI** (the only Python test steps in `ci.yml` before the KSSH job were `:95,404,1747`; the KSSH job runs only the sibling's test, `ci.yml:1760-1761`). So today a regression of the Phase 02 marker list is caught only by running the test by hand. | `grep -n "test_check" .github/workflows/ci.yml`. |

## 3. Decision record and design

**3.1 Decision record.**
1. K4b CP1 (approved 2026-10-30) handed the five closed names to Phase 02 as text only and committed the KSSH program never to edit Phase 02 files (C-K4b-3).
2. KSSH cleanup item 3 approved "B0" (`cleanup/approval.md:7`): no edit of the checker.
3. The user then ordered the leftover follow-ups to be completed. The B0 gap (F2) is a production-shaped candidate that carries a KSSH dev build and passes to exit 3. Completing the hand-off means B1: the names *enter the Phase 02 frozen set* (`phase-02:46`: "every handed-off name enters the frozen marker set here before the handing-off phase proceeds"). KSSH has already proceeded, so this is a late consumption, recorded as such.
4. The edit is performed **as the Phase 02 owner's consumption of a closed-name hand-off**, never as a KSSH edit of a foreign file: the packet (section 11) is the owner's adoption record, in the K2 CP2 form. This keeps C-K4b-3 true in substance (KSSH adds no checker logic) and mirrors `0d05633f0`.

**3.2 Design.** Closed-set only. No new code path:

* `FROZEN_DEV_MARKERS` (a sorted list, `:40-56`) gets five entries at their sorted ASCII positions.
* `DEV_MARKER_NAMES = frozenset(FROZEN_DEV_MARKERS)` (`:57`) and `DEV_MARKERS = tuple(... encode ...)` (`:58`) derive from the list, so the **two** existing rejection channels follow automatically:
  1. *feature channel* (`require_exact_posture`, `:85-87`): any of the names in a KMS/net/kernel feature list is `FAIL: <label> forbidden DEV marker: <names>`;
  2. *artifact channel* (`scan_artifact`, `:91-109`): any of the names in any of the three artifacts is `FAIL: <path> contains forbidden DEV marker: <name>`, chunked at 64 KiB with `max(len)-1` overlap.
* `KMS_FORBIDDEN` is **not** changed. Adding the feature names there would duplicate the DEV-marker error line for the same feature and would change which message a feature list produces; the DEV-marker channel already rejects it (F2).
* The exit semantics are unchanged: any error → exit 1 with `FAIL:` lines on stderr; no error → the ADR-0006 block text, exit 3.
* Names that contain a space (`DEV_POSTURE …`) can never match through the comma-split feature channel; they act only in the artifact channel. This is harmless and matches how the sibling lists all five uniformly; it is not special-cased.

**3.3 Exact edit, `scripts/check-production-relay-image.py` (applies on SHA-256 `5cb07d35ee48434e54aa6210590c74e473a2ea6da54c7640136af33f29af7d65`; result `b0b973467527c3d05388cb1252dfbe0d8c43dd602e477df45f9f8a256876cd29`, computed on the `/tmp` copy):**

```diff
--- a/scripts/check-production-relay-image.py
+++ b/scripts/check-production-relay-image.py
@@ -39,6 +39,9 @@
 )
 FROZEN_DEV_MARKERS = [
     "AWS_DEV_SIGNED_TIME",
+    "CELLOS-KSSH-T1-DEV-ROOT-v1",
+    "DEV_POSTURE ssh-dev-custody",
+    "DEV_POSTURE ssh-dev-persist",
     "DEV_REFERENCE",
     "SLB9672",
     "SOFTWARE_HARNESS",
@@ -51,6 +54,8 @@
     "development-stm32-authority",
     "root-stream",
     "slb9672-dev-anchor",
+    "ssh-dev-custody",
+    "ssh-dev-persist",
     "stm32h573i-dk-dev-authority",
     "vf2-root-stream",
 ]
```

The resulting list is 20 names (15 + 5), still sorted by ASCII byte order (uppercase `AWS…`, `CELLOS…`, `DEV_POSTURE…`, `DEV_REFERENCE`, `SLB…`, `SOFTWARE…`, `STM32…`, `TPM…`, then lowercase `aws…` … `slb9672-dev-anchor`, `ssh-dev-custody`, `ssh-dev-persist`, `stm32…`, `vf2-root-stream`). `"DEV_POSTURE ssh-dev-custody" < "DEV_POSTURE ssh-dev-persist" < "DEV_REFERENCE"` holds (`P` < `R`; `c` < `p`).

## 4. Does the blocked status forbid the edit? (rows, rulings)

| Source | What it says | Effect on this edit |
|---|---|---|
| `phase-02:4,23` | `status: blocked`; "BLOCKED on completed Phase 1" | The status describes Phase 1 admission (hardware, AWS), not a file lock. The 2026-08-26 software-track decision (`plan.md:"Software Track (authorized 2026-08-26)"`; `phase-02` log `:125`) allows host-verifiable Phase 2 deliverables while blocked and says it "changes no acceptance criterion". The marker set is host-only text, no hardware or AWS claim. |
| `phase-02:46,74`; `plan.md:57`; `phase-03:89`; `phase-04:245`; `phase-05:95`; `phase-06:151` | Phase 2 is sole checker owner; hand-offs are names only, merged here | The edit is an owner-side merge of a name hand-off (F6, F7). A non-owner edit is the thing `phase-06:183-187` reverted. |
| `plan.md:9-14` / `locks.md:159` | `executionStatus: blocked`, `executionLock: CELL-CONTRACT-0024`, `lockScope: Unfinished protected authority, time, enrollment and KMS handoff contracts`; row "blocked … P01 then P08/P10; explicit per-slice handoff" | The five names are not an authority, time, enrollment or KMS-handoff contract; they narrow what a production candidate may contain (strictly more rejections, no acceptance added). But the lock text is "P01 rebase plus explicit owner handoff and the phase-local approvals are required before any selected slice is released", and "Documentation completion does not unlock execution". The conservative, precedent-conforming reading: a file-bounded adopted slice is required (as for K2 CP2, K3, K5, K7, K4b, NET-OD: `locks.md:115-125`). **Section 11 is that packet.** |
| `locks.md` unfreeze step 3 | "The accountable owner explicitly adopts/rebases that slice … names its file-owner handoff and confirms no overlapping owner" | The Phase 02 owner is not a named person in the repo; the packet asks the accountable owner (the user) to name the Phase 02 file-owner for this one edit (D-P2-1). [INFERENCE] The user is the sole accountable owner, as in every prior packet (`k4b-cp1-approval.md:3`). |
| `phase-02:48-53` (Hard Stops; `:53` "Stop if production can accept, strip, relabel, or promote a DEV artifact") | the checker must reject DEV artifacts | B1 moves toward, not away from, this stop: KSSH dev builds are DEV-posture artifacts the production candidate must reject. No fallback, no promotion switch added. |
| `phase-02:97-104` (`Success Criteria`) and the open box `:104` `- [ ] Only Phases 3/4/5 become eligible …` | open | **Unchanged by B1.** The notes do not tick or untick any box and do not change `status`. |
| P01 verifier (`verify-cell-contract-p01.py:203`) | requires `executionStatus: blocked`, `lockScope` | Notes go in the Deviation Log (a body paragraph), never in frontmatter. |
| Repo enforcement | no `.git/hooks` active (only samples), no `.githooks`/`.husky`; the only code reading `executionStatus` is the P01 consistency verifier (`:203`) | Nothing mechanically blocks the edit; the constraint is procedural. [INFERENCE] other CI jobs do not gate by plan status (`.github/workflows` grep for `executionLock`: none searched beyond `scripts/*.py`). |

Verdict: **the blocked status does not forbid the edit once the owner adopts it as a bounded slice; it forbids doing it without that adoption.** Nothing in the plan's hard stops is touched. The edit is additive-rejecting only.

Related plan relations: `260825-1726-kms-silo-production-root` (`locks.md:155`, in-progress) first *created* the checker (`phase-01-kms-tls-signing-vertical-slice.md:61,86,166`) and keeps whole-plan relations; it receives the same reciprocal note as for K2 (section 8, R2). `260826-1605-phase4-dev-reference-authority` (`locks.md:159`) is the owner.

## 5. Exact file closure (pre-edit SHA-256 at HEAD `952d84c8a`)

**B1 core (the Phase 02 hand-off):**

| Path | Action | SHA-256 before | After (expected) | Locked surface |
|---|---|---|---|---|
| `scripts/check-production-relay-image.py` | edit: 5 inserted list lines (3.3) | `5cb07d35ee48434e54aa6210590c74e473a2ea6da54c7640136af33f29af7d65` | `b0b973467527c3d05388cb1252dfbe0d8c43dd602e477df45f9f8a256876cd29` (computed on `/tmp` copy) | Phase 02 sole-owner file |
| `scripts/test_check_production_relay_image.py` | edit: 2 new tests (section 7), +36 lines | `b13e1c298fb6581fbd7dc33840fc08d241b74f1ed9f3608c16be3eadf8e74304` | `a1caa985c57ca6db0692be9546949b72bb55f33938f489ef97300b373577ce79` (computed on `/tmp` copy) | Phase 02 sole-owner file |
| `.agents/260826-1605-phase4-dev-reference-authority/phase-02-private-protocol-and-dev-separation.md` | edit: one Deviation Log paragraph after line 130 (R1, section 8), no frontmatter or checkbox change | `ebc1ec1b658faf04e088e9e9de5ebe64922e934bcfde187e2001ecd9ef469e88` | n/a (text edit) | blocked plan, reciprocal note |

**Reciprocal notes and records (docs; one scheduling change per packet 11.5):**

| Path | Action | SHA-256 before |
|---|---|---|
| `.agents/260825-1726-kms-silo-production-root/plan.md` | R2 one note (K2 precedent), body only; frontmatter untouched | `717ec671db795a73…` (prefix; recompute at binding) |
| `.agents/cell-contract-0024-locks.md` | one "Bounded adopted slice — KSSH Phase 02 marker hand-off" paragraph next to the NET-OD paragraph (`:125`-area) | `1f674e010ab0715bf7b66c554e5d125b2d35b999fb7b1668441abda7227274c0` |
| `.agents/plan-portfolio.md` | one clause in the KSSH state line (`:128`) | `d1e89ceb1c0433627fa68b4122e2127ff6fee6b9e6a734e669503c347a42ddd8` |
| `.agents/261030-1000-kms-ssh-key-custody/plan.md` | R4: append one dated line after the 2026-10-30 cleanup line (that line's "marker hand-off stays text-only (B0)" stays as history) | `de0e5ea7b352250f9b90b5803cc0c09b3a7726e28c714f0cd137ae36cdc0e943` |
| `CHANGELOG.md` | one bullet (R5), main agent | `c5ef0411d34cd77667a91ca71f8d937fe8ff819d3c22228e036a6ede3f6aa5db` |

**Optional, owner-decided (D-P2-3, D-P2-4):**

| Path | Action | SHA-256 before | After (expected) |
|---|---|---|---|
| `scripts/test_check_ssh_dev_persist_exclusion.py` | +13 lines: one parity test (section 7.3) | `d2d2231a5d11d9a6a0cacf0754c8f601416911c4b1d1539705f3b4a3fec5c6d3` | `ca2fa66701a93ff4f370f2636e13be55fc405f2470b32e472a044cbe8dda99e5` |
| `.github/workflows/ci.yml` | +5 lines: one step in job `kms-ssh-dev-posture` (section 9.3) | `731e5ffa99edb338222a0c9bc153069217dc89292821987dc9b8f1c682e62a9f` (2253 lines) | `e9d8481856c24204f1e476ce6820bb2dd5bb1858666c4c94f9e792c6b9309bc1` (2258 lines) |

**Read-only inputs that must stay byte-identical (stop on drift):**

| Path | SHA-256 |
|---|---|
| `scripts/build-production-relay-image.sh` | `3ee7736b91a8a9478e03257c488082ebfe3fda2ffe5288dbcbb88f96fa560cda` |
| `scripts/check-ssh-dev-persist-exclusion.py` | `71ceab71e8f04429a72dc70be8d312c987b8c0fd0c5b9c19ffbfc00729548735` |
| `cells/services/kms/Cargo.toml` | `6384bcf565520dd99d43c88da69d2ca3cb5a10b51616f2d313e237cf17e55ca0` |
| `cells/services/kms/build.rs` | `792505f3f152be9c322eb27666105777a61a1b0e3604254e882d76711373e900` |
| `cells/services/kms/src/lib.rs` | `5023526eda64557c6688c4f3bf69ecaef24874d8fc5b31acce30b7b97d065c95` |
| `cells/services/kms/src/main.rs` | `7fc9a2ba634a020af73efa3f43c214761acb82d4e7fe2e3ccb1eb281d4bdf516` |
| `docs/code-standards.md` | `9cb004adc0db8f4b1da77b78419507c3c936d76b23370e68752be54007da1185` |
| `phase-06-frozen-abi-kms-authority-integration.md` | `4d52a58d00795a688a5532bd8e666265f010202f25991e2953d9c4d692988f87` |
| `phase-08-fault-evidence-and-entry-gate-review.md` | `7675b282bd661c9fec3d58f0cfba82c1b0ea15b088cb3530e16dae54692ba752` |
| `.agents/260826-1605-phase4-dev-reference-authority/plan.md` (**frontmatter must not change**) | `6e85c9069d992b6bf7b163a5173c0d9f16a54f7a68f7c2c1629bf0a91561ab38` |

**P01 pin consequences (not edited by this item).** `exact-deltas/manifest.json:13568-13569,14004-14005` and `consumer-census-source.json` (`consumer_inputs[3287]`, `[3396]`; `occurrences[3157]`, `[3265]` carry per-symbol line numbers that shift) pin the pre-edit bytes. After B1 `verify-cell-contract-p01.py` gains one `PACKAGE_SOURCE_DRIFT` and one `CENSUS_SOURCE_DRIFT` per edited pinned path (two paths each; [INFERENCE] from the same rule observed for `completion_selftest.rs` in `cleanup/completion-queue.md:220`, i.e. 352→354 and 58→60). The gate is **already red** (F9); refreshing the pins would hide 400+ unrelated drifts and is a P01-owner action (`--refresh-census`), so this item records the expected drift and does not refresh (D-P2-6). The optional sibling-test and `ci.yml` edits are not pinned by those manifests (the P01 walk found no entry for either path).

No `Cargo.toml`, `Cargo.lock`, KMS/cell/`libs/**`/`kernel/**`, image, signing, `config/` or other workflow (`security|perf|docs|release|dco`) change. Any other path discovered mid-slice stops the slice and returns to the owner as an amendment.

## 6. New files

None. (This CP1 and its approval record are the only new files, both under `.agents/261110-1200-cellbox-followups/`.)

## 7. Tests

**7.1 What the existing tests already cover without an edit (F2, observed on the patched `/tmp` copy).** `test_each_dev_marker_in_features_exits_one_naming_it`, `test_each_dev_marker_in_artifact_content_exits_one_naming_it`, `test_dev_marker_split_across_chunk_boundary_is_detected` iterate `CHECKER.FROZEN_DEV_MARKERS` (`test_check_production_relay_image.py:89-133`), so the five new names get: a feature-list exit 1 naming each, an artifact-content exit 1 naming each, and a split-at-64 KiB-boundary exit 1 naming each. The block-reason tests (`:136-191`) keep proving exit 3 + exact ADR text for the clean posture and the builder stub.

**7.2 Two added tests (`scripts/test_check_production_relay_image.py`, applies on `b13e1c29…74304`).** Placement follows the `0d05633f0` precedent (membership test right after `CHUNK`, before `run_checker`) plus an end-to-end test before the chunk-boundary test:

```diff
--- a/scripts/test_check_production_relay_image.py
+++ b/scripts/test_check_production_relay_image.py
@@ -56,6 +56,14 @@
             CHECKER.DEV_MARKER_NAMES,
         )
 
+    def test_kssh_dev_custody_handoff_is_frozen(self) -> None:
+        self.assertLessEqual(
+            {"CELLOS-KSSH-T1-DEV-ROOT-v1", "DEV_POSTURE ssh-dev-custody",
+             "DEV_POSTURE ssh-dev-persist", "ssh-dev-custody",
+             "ssh-dev-persist"},
+            CHECKER.DEV_MARKER_NAMES,
+        )
+
     def run_checker(self, kms_features: str, artifact_payloads: list[bytes]):
         root = Path(__file__).resolve().parents[1]
         with tempfile.TemporaryDirectory() as directory:
@@ -115,6 +123,34 @@
                     completed.stderr,
                 )
 
+    def test_kssh_custody_and_persist_artifacts_exit_one_naming_markers(self) -> None:
+        clean = b"qualified-candidate"
+        banners = {
+            "ssh-dev-custody": (
+                b"[kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf",
+                ["DEV_POSTURE ssh-dev-custody", "ssh-dev-custody"],
+            ),
+            "ssh-dev-persist": (
+                b"[kms] DEV_POSTURE ssh-dev-persist: wrapped identity persisted to "
+                b"/srv, dev root key co-resident, no rollback or crash-durability claim "
+                b"CELLOS-KSSH-T1-DEV-ROOT-v1 PUBLIC CONSTANT NOT A SECRET",
+                ["CELLOS-KSSH-T1-DEV-ROOT-v1", "DEV_POSTURE ssh-dev-persist",
+                 "ssh-dev-persist"],
+            ),
+        }
+        for variant, (payload, names) in banners.items():
+            for slot in range(3):
+                with self.subTest(variant=variant, slot=slot):
+                    payloads = [clean, clean, clean]
+                    payloads[slot] = payload
+                    completed = self.run_checker("hardware-relay-provider", payloads)
+                    self.assertEqual(completed.returncode, 1)
+                    for name in names:
+                        self.assertIn(
+                            f"contains forbidden DEV marker: {name}",
+                            completed.stderr,
+                        )
+
     def test_dev_marker_split_across_chunk_boundary_is_detected(self) -> None:
         clean = b"qualified-candidate"
         for marker in CHECKER.FROZEN_DEV_MARKERS:
```

Why these two: (1) the membership test freezes the *hand-off* so an accidental removal fails by name (the iterating tests would silently shrink with the list); (2) the banner test uses the real banner/label bytes (F1) in each of the three artifact slots, so it proves the end-to-end rejection of a custody/persist KMS bytes **and** of a net or kernel artifact that embeds them. It does not duplicate the sibling checker's own tests.

**7.3 Optional parity test (D-P2-3), `scripts/test_check_ssh_dev_persist_exclusion.py`, in `MarkerTests` after `test_the_marker_set_is_the_closed_five` (applies on `d2d2231a…c6d3`):**

```diff
@@ -91,6 +91,19 @@
             ],
         )
 
+    def test_every_marker_is_in_the_production_checker_frozen_set(self) -> None:
+        # Read-only import of the Phase 02 checker: B1 hand-off (the production
+        # checker rejects the same five names), so a removal there fails here too.
+        path = Path(__file__).with_name("check-production-relay-image.py")
+        spec = importlib.util.spec_from_file_location("production_relay_checker", path)
+        assert spec is not None and spec.loader is not None
+        production = importlib.util.module_from_spec(spec)
+        spec.loader.exec_module(production)
+        self.assertLessEqual(
+            {marker.decode("ascii") for marker in CHECKER.MARKERS},
+            production.DEV_MARKER_NAMES,
+        )
+
     def test_a_marker_split_across_a_chunk_boundary_is_detected(self) -> None:
         for marker in CHECKER.MARKERS:
             with self.subTest(marker=marker):
```

This test *fails on HEAD* (observed: baseline copy 14 tests, 1 failure) and passes after B1 (14 OK), so it **must land in the same change as B1 or after it**. Its value: the sibling's test already runs in hosted CI (`ci.yml:1760-1761`) while the production checker's tests do not (F11), so a future removal from the Phase 02 list fails CI without a CI edit. The sibling docstring says the script "never edits or imports" the Phase 02 checker; this test imports it read-only and the script itself is unchanged.

**7.4 Observed on `/tmp` copies** (`/tmp/p2b1`, repo untouched):

| Run | Result |
|---|---|
| baseline `test_check_production_relay_image.py` (copy of HEAD scripts + the builder-test fixture file `capability.rs`) | `Ran 8 tests … OK` |
| patched checker + patched test | `Ran 10 tests … OK` |
| patched test against the **unpatched** checker | `Ran 10 tests`, `FAILED (failures=7)`: `test_kssh_dev_custody_handoff_is_frozen` plus the six `variant × slot` sub-assertions |
| sibling test, baseline copy with parity test added | `Ran 14 tests`, `FAILED (failures=1)` |
| sibling test, patched checker + parity test | `Ran 14 tests … OK` |
| in the repo (read-only, HEAD): `test_check_production_relay_image.py` | `Ran 8 tests … OK` |

The first row's builder test needs `cells/services/kms/src/storage/capability.rs` relative to the repo root (`:176-178`); a bare `scripts/` copy fails it with exit 2 (observed, fixture not a defect), so the copy tree carried that one file.

## 8. Reciprocal notes (exact text)

Dates below take the date of the owner's approval (2026-10-30 if approved the same day). Body-only edits; no frontmatter or checkbox change anywhere.

**R1 - `phase-02-private-protocol-and-dev-separation.md`, Deviation Log, one paragraph appended after line 130:**

> - 2026-10-30 — Ruling (accountable owner, KSSH Phase 02 marker hand-off, [CP1](../261110-1200-cellbox-followups/phase02-marker-cp1.md) C-P2-1..3): the KSSH program's closed marker hand-off (`k4b-cp1.md` §5 items 4-5, `k5-cp2-dossier.md:162`) is consumed into this phase's sole checker as a late consumption, as for `SOFTWARE_HARNESS` on 2026-08-31. `scripts/check-production-relay-image.py` `FROZEN_DEV_MARKERS` gains exactly `CELLOS-KSSH-T1-DEV-ROOT-v1`, `DEV_POSTURE ssh-dev-custody`, `DEV_POSTURE ssh-dev-persist`, `ssh-dev-custody`, `ssh-dev-persist` (20 names); no other checker line changes, `KMS_FORBIDDEN` is untouched, exit 1 / exit 3 and the `BLOCKED_BY_ADR_0006` text are unchanged. `scripts/test_check_production_relay_image.py` gains a frozen-membership test and a banner end-to-end test and now passes 10/10 on the host. The KSSH program did not edit either file; the edit was made under the owner's file-bounded adoption of this packet. This changes no Phase 1 admission, hardware, AWS or acceptance gate: `status` and every checkbox stay as they were, parent Phase 4 stays blocked, SOFTWARE_HARNESS evidence only. P01 pin drift for the two paths is recorded, not refreshed, in the CP1.

**R2 - `260825-1726-kms-silo-production-root/plan.md`, body note** (the plan that first created the checker, `phase-01-kms-tls-signing-vertical-slice.md:61,86,166`):

> - 2026-10-30 — Note (KSSH Phase 02 marker hand-off): `scripts/check-production-relay-image.py` and its test are owned by `260826-1605-phase4-dev-reference-authority` Phase 02; their frozen DEV marker set gained five closed KSSH names under that owner's approval. This plan's production-checker expectations (feature policy, ADR-0006 block, exit 3) are unchanged. See [CP1](../261110-1200-cellbox-followups/phase02-marker-cp1.md).

**R3 - `cell-contract-0024-locks.md`, one paragraph after the NET-OD paragraph (the same shape as `:119,:123`):**

> **Bounded adopted slice — KSSH Phase 02 marker hand-off (2026-10-30):** the accountable owner approved the [CP1 and lock handoff](261110-1200-cellbox-followups/phase02-marker-cp1.md) (section 5 and 11). File-bounded exception for `scripts/check-production-relay-image.py`, `scripts/test_check_production_relay_image.py` and one Deviation Log paragraph in `260826-1605-phase4-dev-reference-authority/phase-02-private-protocol-and-dev-separation.md` (plus, if the owner so ruled, `scripts/test_check_ssh_dev_persist_exclusion.py` and one step in `.github/workflows/ci.yml`): five closed ASCII KSSH marker names enter the frozen production-checker DEV set, host Python only, `DEV_POSTURE` rejection. No `libs/api`/`libs/types` delta, no authority/time/enrollment/KMS-handoff contract. Rows `260825-1726-kms-silo-production-root` and `260826-1605-phase4-dev-reference-authority` keep their whole-plan relations and `executionStatus: blocked`.

**R4 - KSSH `plan.md`, appended line after the 2026-10-30 cleanup line:**

> - **2026-10-30 Phase 02 marker hand-off:** cleanup item 3 option B0 is superseded by B1 on the user's order to complete the follow-ups; the five closed KSSH names enter `FROZEN_DEV_MARKERS` of the Phase 02 production checker under the Phase 02 owner's adoption ([CP1](../261110-1200-cellbox-followups/phase02-marker-cp1.md)). The sibling `scripts/check-ssh-dev-persist-exclusion.py` stays as the variant-identity proof (K8, CI job `kms-ssh-dev-posture`). The B0 builder note ("the production builder should call the sibling") is retired: the builder calls the production checker only.

**R5 - `CHANGELOG.md`, one bullet.** Place it under `## [Unreleased]` `### Changed` (the K2 CP2 KSSH bullet is at `CHANGELOG.md:10` under `[Unreleased]`). Observed: the later KSSH bullets (K3 `:1134` … CI bullet `:1152`) sit under `## [0.2.0] - 2026-05-01` (heading `:1120`), which looks misplaced; this CP1 does not move them (D-P2-9).

> - Production relay image checker: the frozen DEV marker set also rejects the KMS SSH development custody names (`CELLOS-KSSH-T1-DEV-ROOT-v1`, `DEV_POSTURE ssh-dev-custody`, `DEV_POSTURE ssh-dev-persist`, `ssh-dev-custody`, `ssh-dev-persist`) in feature lists and in the KMS, net and kernel artifacts (exit 1); a clean candidate still exits 3 with the ADR-0006 block. Closed-name hand-off from KSSH K4b/K5 under the Phase 02 owner; no KMS, ABI or image change.

`docs/code-standards.md` (KMS SSH custody bullet, `:87-98`) needs no edit; optional one clause "and the production relay image checker rejects its marker names" is D-P2-9, default no.

## 9. CI: is there a production-image pipeline to attach `--expect none` to? What is wired

**9.1 No pipeline exists (confirmed independently of `ci.yml.md`).**
* `.github/workflows/{ci,dco,docs,perf,release,security}.yml` contain no invocation of `build-production-relay-image.sh` or `check-production-relay-image.py`; the only hits for `production-relay-image`/`check-production-relay` under `.github` are the comments of the KSSH job (`ci.yml:1737-1739`, `:1782`).
* `scripts/build-production-relay-image.sh` validates inputs and exits 3 (`:43`); it never calls the checker and has no artifact inputs (so it cannot host an `--expect none` call). Its test pins exit 3 + the exact ADR text (`test_check_production_relay_image.py:169-191`).
* The only callers of the production checker are its own test and documents. `scripts/qemu-getrandom-sas-test.sh:21` builds a `production-relay-image` *kernel* for a GetRandom test lane, which is not a production candidate pipeline and is not in any workflow (`grep getrandom-sas .github/workflows/*.yml`: none).
* `.agents/TODO.md:171`: "CI chỉ build default features, không có matrix cho `--no-default-features --features production-relay-image`" (the quoted `ci.yml:164-259` lines are historical).
* ADR-0006 makes a production image a Phase 6 / superseding-GO outcome (`BLOCKED_BY_ADR_0006` text), so a pipeline *cannot* exist yet by design.

Therefore **there is nothing to attach `--expect none` to today**. After B1 the right production hook is the **production checker itself** (it now rejects the five names for KMS, net and kernel artifacts); the sibling's `--expect none` is not needed in a production pipeline, and the B0 requirement "the later builder should call the sibling before the BLOCKED path" is retired (R4). If a production pipeline is later built (Phase 6 + ADR unblock), it calls `check-production-relay-image.py` with the three artifacts and the sibling is unnecessary there.

**9.2 What is already wired (landed in HEAD; not edited by this CP1).** Job `kms-ssh-dev-posture` (`ci.yml:1741-1811`, from cleanup item 3 part A): runs `python3 scripts/test_check_ssh_dev_persist_exclusion.py`; the two feature host suites (`--features ssh-dev-custody`; `--features ssh-dev-persist --lib`); then builds the default (`CELLOS_PRODUCTION=1`), custody and persist `service-kms` for `x86_64-unknown-none` and asserts the sibling checker's nine rows in both directions (`expect none "" none 0`, … `expect persist ssh-dev-persist persist 0`). That is the minimal CI wiring: it proves, on built artifacts, that the production-like default KMS carries no marker and that each dev build carries exactly its own. It is a *variant-identity* proof, not a production-candidate check. Hosted-runner behaviour of the job is [INFERENCE]/unobserved (cleanup item 3 recorded the local equivalents only).

**9.3 Optional addition (D-P2-4): run the Phase 02 checker's tests in that job.** The production checker's 10 tests (after B1) never run in CI (F11). One step, **run only** (Phase 02 owns the files; the job just runs them), `ci.yml` after the existing sibling-test step (`:1760-1761`):

```diff
@@ -1760,6 +1760,11 @@
       - name: Unit-test the KMS SSH exclusion checker
         run: python3 scripts/test_check_ssh_dev_persist_exclusion.py
 
+      # Phase 02 owns this script; the job only runs its tests, so the five KSSH marker
+      # names stay in its frozen set (scripts/check-production-relay-image.py).
+      - name: Unit-test the production relay image checker
+        run: python3 scripts/test_check_production_relay_image.py
+
       - name: Host tests (ssh-dev-custody, ssh-dev-persist)
         run: |
           cargo test -p service-kms --features ssh-dev-custody \
```

Applies on `731e5ffa…a9`, result `e9d84818…bc1` (2253 → 2258 lines); the patched copy parses with `yaml.safe_load` (27 jobs, unchanged count); `scripts/check-cargo-config-parity.py` reads only the first workflow-level `CARGO_TARGET_*` block and passes at HEAD (`PASS: 3 targets agree`, observed; the diff is far below that block). The test needs only `bash`, `python3` and the repo's `cells/services/kms/src/storage/capability.rs` (`:176-178`), all present on the runner after checkout. No new installs. With D-P2-3 the parity test already provides CI protection for the five names, so D-P2-4 is the broader (all 20 names plus the block-text) protection; recommended yes because the cost is five lines and ~1 s of runner time [INFERENCE: wall time 0.9-1.3 s observed locally].

## 10. Verification and run budget

All host-only, stdlib Python, seconds. **No cargo, no QEMU, no Docker, no network, no key.**

**Run at binding (read-only, already done for this text):** section 7.4 rows on `/tmp` copies; F2/F3 demonstrations; `python3 scripts/test_check_production_relay_image.py` in the repo at HEAD (8 OK); `python3 scripts/check-cargo-config-parity.py` (PASS); `yaml.safe_load` of the patched `ci.yml` (27 jobs).

**To run at CP2 (after the owner approves; implementing agent):**

| # | Proof | Expect |
|---|---|---|
| V1 | `git status --short` clean at start; `sha256sum` of every path in section 5 equals "before" | stop on any drift |
| V2 | apply 3.3, 7.2 (and 7.3 if D-P2-3), `git diff --stat` | exactly the section-5 core paths; checker +5, test +36 |
| V3 | `sha256sum scripts/check-production-relay-image.py scripts/test_check_production_relay_image.py` | `b0b97346…6cd29`, `a1caa985…7ce79` |
| V4 | `python3 scripts/test_check_production_relay_image.py` | `Ran 10 tests … OK` |
| V5 | `python3 scripts/test_check_ssh_dev_persist_exclusion.py` | 13 OK (14 OK with 7.3) |
| V6 | F2 demonstration with the three K4b artifacts and the synthetic kernel artifact (command in 10.1) | exit `3`, `1`, `1`; `1` for `--kms-features hardware-relay-provider,ssh-dev-persist`; synthetic kernel with persist banner `1` |
| V7 | `bash scripts/build-production-relay-image.sh x` with the three `CELLOS_PRODUCTION_ROOT_*` env of `test_check_production_relay_image.py:172-179` | exit 3, stderr == ADR text (unchanged) |
| V8 | (if D-P2-4) `python3 -c 'import yaml;yaml.safe_load(open(".github/workflows/ci.yml"))'`; `python3 scripts/check-cargo-config-parity.py`; `bash scripts/check-board-configs.sh` | parse OK, PASS, PASS |
| V9 | `python3 scripts/verify-cell-contract-p01.py` read-only | red as at HEAD; +2 `PACKAGE_SOURCE_DRIFT`, +2 `CENSUS_SOURCE_DRIFT` for the two scripts and nothing else new (the baseline counts 352/58/3 re-taken first) |
| V10 | `git diff --stat` of `phase-02-…md`: body-only; `grep -c "^executionStatus\|^status:" ` unchanged in `plan.md` and phase 02 frontmatter | unchanged |
| V11 | first hosted CI run (only if D-P2-4) | **not runnable locally; unobserved** |

**10.1 V6 command shape** (copy the artifacts into `/tmp`, never write into the repo):

```
python3 scripts/check-production-relay-image.py --kms-features hardware-relay-provider \
  --net-features verified-tls,tls-roots-embedded,tls-ca-private --kernel-features production-relay-image \
  --kms-artifact <kms-elf> --net-artifact <clean-net> --kernel-artifact <clean-kernel>
```

with `<clean-net>` and `<clean-kernel>` two distinct non-empty marker-free files (the checker requires three distinct artifacts, `:133`).

**Budget:** one implementing session of under an hour including the docs; the CI step (if taken) adds about 1 s per run.

## 11. Lock handoff packet (CELL-CONTRACT-0024; form of `net-owner-death-cp1.md` section 11 / `lock-handoff-k2-cp2.md`; DRAFT for the accountable owner, not submitted)

**11.1 Slice identity.** KSSH Phase 02 marker hand-off: five closed ASCII KSSH marker names into the frozen DEV marker set of the production relay checker, two tests, one Deviation Log paragraph, host Python only, `DEV_POSTURE` rejection. Serves the closure of KSSH cleanup item 3 option B1 / K4b D-4b.13 and K5 dossier hand-off (`k5-cp2-dossier.md:162`); it serves Cellbox publickey exclusion from production and **not an Intel C2C outcome**; the owner is asked to confirm the same bounded-slice basis as Phase 02 (Cellbox), K2 CP2, K3, K5, K7, K4b and NET-OD. Controlling plan `.agents/261011-1200-cell-contract-authority/plan.md` (status complete; completion unlocks nothing, procedure step 6). Evidence reconciled: `k4b-cp1.md` + approval (C-K4b-3), `cleanup/ci.yml.md` + `cleanup/approval.md` (B0 chosen), `0d05633f0` precedent, this document; no history erased.

**11.2 ADR-0024 contract delta (procedure step 2).**

| Principle | Effect |
|---|---|
| Cell principal/incarnation distinct from Task | none; no runtime component is touched |
| Shared per-Cell capability space; delegation/attenuation | none; no capability, handle, syscall or manifest change |
| Revocation generations | none |
| Kernel substrate vs ordinary policy Cells | none; no kernel, net or KMS source is touched |
| Durable logical state vs execution | none (a list of strings in a host script) |
| Caller/test/doc migration, concurrency, remote | callers: `check-production-relay-image.py` has no automated caller (F9, section 9.1) and `build-production-relay-image.sh` is unchanged; tests: section 7; docs: Deviation Log R1, notes R2-R5; no concurrency, no remote impact |
| No ambient credential / remote shortcut | none; the change can only turn an exit 3 into an exit 1 for a candidate that carries a KSSH dev build; it adds no acceptance path |

**11.3 Locked rows and overlapping plans (procedure step 3).** Read-only; `git status --short` empty at HEAD `952d84c8a`; last commits touching the two scripts: `6586330c8`, `b47465282`, `0d05633f0`, `ea7503c8d` (Phase 02 owner work; none after the lock of 2026-10-11 from the listed `git log`).

| Row / plan (`locks.md`) | Blocked scope | Relation | Needed from the owner |
|---|---|---|---|
| **`260826-1605-phase4-dev-reference-authority`** (`:159`; `executionStatus: blocked`) | "Unfinished protected authority, time, enrollment and KMS handoff contracts" | **Direct: owns both scripts** (`phase-02:46,74`; `plan.md:57`). The change is not an authority/time/enrollment/KMS-handoff contract (section 4) but touches a file the plan owns | adopt as a file-bounded exception for the three core paths, **name the Phase 02 file-owner for this edit**, rule C-P2-1 (consumption of a closed-name hand-off, late), approve R1 |
| **`260825-1726-kms-silo-production-root`** (`:155`, in-progress) | mTLS, relay, protected signer, production-root integration | created the checker (`phase-01:61,86,166`), keeps whole-plan relation; its production-checker expectations (feature policy, ADR-0006 exit 3) are unchanged | ruling C-P2-2: note R2, no other effect |
| `260826-1605` Phases 3-7 hand-off rule (`phase-03:89`, `phase-04:245`, `phase-05:95`, `phase-06:151`) | marker-name hand-offs, never checker edits | the rule is about *those phases*; KSSH is an external plan; this slice follows the same discipline: names only | ruling C-P2-3: the rule is applied by analogy and consumed by the Phase 02 owner, not by KSSH |
| `261011-1200-cell-contract-authority` P01 manifests (`exact-deltas/manifest.json:13568-13569,14004-14005`; `consumer-census-source.json`) | pinned source digests | **+2 `PACKAGE_SOURCE_DRIFT`, +2 `CENSUS_SOURCE_DRIFT`** on an already red gate (F9) | owner confirms no refresh in this item (D-P2-6) or assigns it to P01 |
| KSSH K2 CP2 / K3 / K5 / K7 / K4b adopted slices (`:113-123`) | their paths | none lists either script; K4b CP1 §13 never edits Phase 02 files (C-K4b-3 stays true) | none |
| NET-OD (`:125`) and Tier-3 / Cellbox rows | kernel, Net | no path overlaps | none |
| `.github/workflows/ci.yml` (only if D-P2-4): no adopted slice names it; the `kms-ssh-dev-posture` job was approved as cleanup item 3 (`cleanup/approval.md:7`); the P01 manifest does not pin it | | one step added to that job | included in the same approval if D-P2-4 = A |
| Plans not examined for overlap: `260825-sdk-delivery` P02, `260712-1902-dice-attestation-identity` P03, `260819-1409-cell-to-cell-anywhere-core` P02, controlling `migration-ledger.md` | | grep-level only; `git grep` of the two script names over `.agents/` finds only the KSSH, Phase 02-owner, P01-manifest, KMS-silo phase-01 and S10-10 probe documents | owner confirms no overlap or names one |

**11.4 Retained gates (procedure step 4).** Law 1: not triggered (section 12); no two-confirmation checkpoint. Protected identity / non-rollback time/state: not applicable, not claimed. Independent human decisions: D-P2-1..9. Hardware, AWS, production gates: untouched; `BLOCKED_BY_ADR_0006` and exit 3 for a clean candidate preserved; **no production-image pipeline is created or implied**. A green host test, a clean diff or this document does not satisfy or bypass any of them.

**11.5 Scheduling change (step 5; text only).** In one change: record "KSSH Phase 02 marker hand-off" as a bounded adopted slice with ceiling "five closed names in the frozen production-checker DEV set + two tests + one Deviation Log paragraph, host Python, `DEV_POSTURE`" in the controlling plan, `plan-portfolio.md` (KSSH state line) and `cell-contract-0024-locks.md` (R3, next to NET-OD); apply R1, R2, R4, R5; keep every whole-plan relation and frontmatter. WIP stays one implementation slice; single integrating owner.

**11.6 Execution boundary and rebase order once adopted.**
1. Baseline first: `git status` clean; digest re-check of section 5; `python3 scripts/test_check_production_relay_image.py` (8 OK) and the sibling test (13 OK); P01 verifier counts recorded.
2. Edits only to the section-5 paths. `scripts/build-production-relay-image.sh`, the sibling checker, every KMS file and the plan frontmatter are pinned read-only. Any other path stops the slice and returns to the owner as an amendment (Phase 02 A1 pattern). No dependency, feature, manifest, allowlist or `Cargo.lock` change. No cargo, no QEMU, no VM, no network, no key.
3. Rebase order: the optional sibling-test parity (7.3) lands **in the same commit as, or after,** B1 (it fails without it); the optional CI step (9.3) lands with or after B1 (the production tests must be 10/10 first). Phase 6 and later Phase 02 edits rebase on the post-B1 hashes `b0b97346…6cd29` / `a1caa985…7ce79`.
4. **Rollback:** restore the pre-edit hashes of the section-5 paths (source reversal; no persistent state, no image, no key). Reverting removes only added rejections.

**11.7 Decisions requested from the accountable owner.** (1) Adopt the Phase 02 marker hand-off as a bounded slice with the section-5 file-bounded exception, superseding B0. (2) Name the Phase 02 file-owner for this edit; rule C-P2-1 (late consumption of a closed-name hand-off), C-P2-2 (note in `260825-1726`), C-P2-3 (the "names only, never checker edits" rule is for Phases 3-7; this external hand-off is consumed by the Phase 02 owner). (3) Confirm no overlapping owner for the two scripts or name one, and for the four unexamined plans. (4) Acknowledge the P01 pin drift (+2/+2 on a gate already red) and that pins are not refreshed here. (5) Approve the one scheduling record and notes R1-R5. Nothing in this packet authorises a source edit.

## 12. Law-1 analysis

`code-standards.md:42` scopes Law 1 to `libs/api/` and `libs/types/`. This slice changes **no** byte of either tree and no constant, payload or discriminant they define (it touches two host Python scripts and documents). Therefore **no two-confirmation checkpoint is requested**; the owner is asked only for the "no ABI delta" acknowledgement inside decision 5 of 11.7 (the same standard as NET-OD 11.4). Public KMS opcodes 9-14 and the append-only opcodes 15-24 are unaffected; the banners and the root label already exist in KMS source and are not edited.

## 13. Risks

1. **False positive in a future production candidate.** The check is by byte substring over whole artifacts. The names appear only under the opt-in features (F1); observed 0 hits in the default KMS builds of three targets and in the local net, kernel and embedded x86_64 images (F3). A future default-build string containing `ssh-dev-custody` (a log line, a doc string compiled into `.rodata`) would trip it: the intended behaviour, and the fix is to rename the string, not to relax the checker. The same risk exists for CI job `kms-ssh-dev-posture` (`none` row).
2. **Marker substring overlap** (F5): both a short and a long name are reported for one banner; the tests assert `assertIn` for each, not equality of the whole stderr.
3. **Process risk: a Phase 02 edit while its plan is blocked.** Covered by the adoption (section 4); the Phase 6 precedent (`phase-06:183-187`) is the failure this avoids.
4. **P01 pin drift** (F9): +2/+2 on a gate that is already red; a later P01 `--refresh-census` is the owner's action. The `occurrences` entries (line numbers of symbols in the checker/test) shift by 5 and 36 lines; [INFERENCE] they refresh with the census.
5. **Coverage ends at the five names.** Other KSSH-adjacent strings are not markers: the admin cell's `"DEV_POSTURE volatile/persistent: …"` (not matched, F3), the Cellbox SSH client path, the `/srv/cellos/kms/ssh-{a,b}.bin` file names. A production image that carried `/bin/kms-ssh-admin` but a default KMS would not be rejected by this change; no closed name was handed off for it and none is added here (D-P2-8, default no).
6. **The check proves identity of strings, not safety** (the sibling's own docstring `:15-16`); a binary that carries the capability without the strings (an obfuscated or stripped build) is not caught. The `build.rs` / `lib.rs` structural guards (`ssh-dev-custody` refused under `CELLOS_PRODUCTION`; excluded with `hardware-relay-provider`) remain the first line.
7. **CI-run cost of D-P2-4** is about one second; failure mode is a red existing job if someone edits the Phase 02 list (intended).
8. **CHANGELOG placement:** the earlier KSSH bullets sit under `[0.2.0]` (observed `:1120,:1134-1152`); a new bullet under `[Unreleased]` makes the file inconsistent until the owner decides to move the others (D-P2-9).
9. **Unobserved: hosted CI.** The `kms-ssh-dev-posture` job and the optional step have not run on a hosted runner (no network).

## 14. Not checked

Hosted CI behaviour of job `kms-ssh-dev-posture` and of the optional step; a fresh rebuild of the three KMS variants (the stored K4b CP2 artifacts and `target/` files were scanned as they are, with no provenance claim); whether a production kernel embeds KMS bytes (F4, [INFERENCE]); whether any process outside the repo calls the production checker; the exact post-edit P01 verifier counts (V9, [INFERENCE] 354/60); the full text of every locked row for unnamed overlap beyond the greps cited; whether other `.githooks`-like enforcement exists outside the repo (none found inside); `docs/` pages other than `code-standards.md` that might quote the checker's marker list (not grepped beyond `scripts;.agents;docs` for the script name).

## 15. Effort and entry gates

**Effort XS** (about 1-2 hours including approvals and notes; the checker edit is 5 lines, tests 36, plus docs). Entry gates: (1) decisions of section 16 answered and bound in `phase02-marker-approval.md` (digest of this file at binding); (2) the lock handoff of section 11 approved, including the Phase 02 file-owner name and overlap rulings; (3) baseline capture and start-of-slice re-check; (4) the "no ABI delta" acknowledgement.

## 16. Decisions (OPEN until the user answers; recommendations in the last column)

| ID | Decision | Options | Recommendation |
|---|---|---|---|
| D-P2-1 | Lock treatment and owner | **A** adopt the Phase 02 marker hand-off as a bounded slice (file-bounded exception for the three core paths), name the Phase 02 file-owner, supersede B0 with B1; **B** keep B0 and leave F2 open until Phase 6 | **A** (the user ordered the follow-ups completed; the edit is additive-rejecting, host-only, and has a precedent `0d05633f0`; without adoption the edit would be the Phase 6 breach again) |
| D-P2-2 | Test shape | **A** frozen-membership test + banner end-to-end test (7.2); **B** membership test only (the iterating tests cover content); **C** no test edit (iteration covers) | **A** (membership guards removal; banner test uses the real bytes in all three slots; 7 sub-assertions fail without B1) |
| D-P2-3 | Parity test in the sibling's test (7.3) | **A** add (fails without B1, passes with it; runs in hosted CI today); **B** omit | **A**, in the same commit as B1 |
| D-P2-4 | Run the production checker's tests in CI (9.3, `ci.yml` +5) | **A** add one step to `kms-ssh-dev-posture`; **B** leave CI as is; **C** a separate job | **A** (production tests never run in CI today; five lines, about 1 s; with D-P2-3 the five names are CI-guarded either way, with A all 20 names and the block text are) |
| D-P2-5 | Phase 02 documentation | **A** one Deviation Log paragraph R1, frontmatter and checkboxes untouched; **B** also tick or reword the unchecked last todo; **C** none | **A** (B would misrepresent the plan state; the P01 verifier requires frontmatter `blocked`) |
| D-P2-6 | P01 pins | **A** leave pins, record the expected +2/+2 drift on the already-red gate; **B** refresh the census/manifest entries here | **A** (`--refresh-census` would hide 400+ unrelated drifts; P01 owner action) |
| D-P2-7 | `KMS_FORBIDDEN` | **A** unchanged (the DEV-marker channel rejects the feature names, F2); **B** also add the two feature names to `KMS_FORBIDDEN` | **A** (no duplicate error line; no policy change) |
| D-P2-8 | Marker scope | **A** exactly the five names handed off; **B** also add admin-cell strings or `/bin/kms-ssh-admin` | **A** (closed hand-off; B needs its own hand-off and a measured false-positive check) |
| D-P2-9 | Changelog and docs | **A** one `[Unreleased]` `### Changed` bullet (R5); no `code-standards.md` edit; the misplaced KSSH bullets stay (separate question); **B** also edit `code-standards.md`; **C** place the bullet next to the K8 RT1 bullet under `[0.2.0]` | **A** |
| D-P2-10 | Retire the B0 builder-note | **A** record in R4 that the future builder calls the production checker only; **B** keep the "also call the sibling" requirement | **A** |
