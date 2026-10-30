# KSSH cleanup item 3 — CP1 proposal: CI leg for `ssh-dev-custody` / `ssh-dev-persist` and the production-image marker hand-off

**Status:** proposal, **documents only**. No source, workflow, script, lock, changelog or plan file was edited and nothing was committed. Base: HEAD `ca4b872e6361174af46b8984a0f02bcdbc84a76f`, tree clean when read (`git status --short` showed only the untracked `.agents/261030-1000-kms-ssh-key-custody/cleanup/` directory created by this cleanup program).
**Scope:** (A) a hosted-CI job that compiles and tests the two opt-in KMS SSH features and proves, on built artifacts, which variant each build is; (B) the production-image checker marker hand-off that K4b CP1 D-4b.13 / section 5 item 5 and K5 CP2 dossier line 162 left open; (C) the one-line changelog record.
**Inputs read:** `.github/workflows/ci.yml` (whole job list; lines cited below), `scripts/check-ssh-dev-persist-exclusion.py`, `scripts/test_check_ssh_dev_persist_exclusion.py`, `scripts/check-production-relay-image.py`, `scripts/test_check_production_relay_image.py`, `scripts/build-production-relay-image.sh`, `scripts/qemu-phase09-ssh-test.sh`, `cells/services/kms/{Cargo.toml,build.rs,src/lib.rs}`, `k4b-cp1.md` (sections 5, 7, 8, 11.3, 13, 14), `reports/k4b-cp2-dossier.md`, `reports/k5-cp2-dossier.md`, `k4b-cp1-approval.md`, the Phase 02 plan file, `.agents/cell-contract-0024-locks.md`, `.agents/plan-portfolio.md`.

## 0. Verdict

* **A is a pure CI addition in one file** (`.github/workflows/ci.yml`, +89 lines, one new job, inserted after `unit-tests` so no existing line number at or below 1722 moves). No cell, library, `Cargo.lock`, script or doc file is needed for it.
* **B is separable and needs the Phase 02 owner.** The production checker today lets a production-shaped candidate whose KMS artifact is the custody or persist build through to its normal exit 3 (demonstrated, section 1 F8). The exact five-line fix is given as text and is **not** part of the CI closure.
* Recommended: approve A as written (one file), rule B as option B1 (owner-approved edit of `scripts/check-production-relay-image.py`, test file unchanged) or B0 (keep the text hand-off, rely on the CI `none` leg), and add the changelog bullet (C).

## 1. Findings (evidence)

| # | Finding | Evidence |
|---|---|---|
| F1 | No CI job compiles `ssh-dev-custody` or `ssh-dev-persist`. The only KMS host test is default-feature: `cargo test -p viui -p service-kms -p service-net --target x86_64-unknown-linux-gnu`. | `ci.yml:1689-1690`. `grep` of `ci.yml` for `ssh-dev`, `kms` and `service-kms` hits only `ci.yml:1689`. |
| F2 | Other jobs cannot reach the features either: workspace clippy and the rv64/aarch64 builds use default features; the x86_64 build row is a curated list that **excludes** `service-kms`. `app-kms-ssh-admin` enables `ssh-dev-custody` only as a dev-dependency, which `cargo build/clippy --workspace` (no `--tests`) does not unify. | clippy `ci.yml:131-141`; build matrix `ci.yml:176-209` (x86_64 `cells_args` `ci.yml:207-209`); `cells/tools/kms-ssh-admin/Cargo.toml:21-23`. `app-kms-ssh-admin` (71 tests, `reports/k7-cp2-dossier.md:15`) is **not** run by any CI job either (not in the lists at `ci.yml:1659-1722`) — see D-CI-6. |
| F3 | `ssh-dev-persist` must be tested with `--lib` and `--target x86_64-unknown-linux-gnu`. `lib.rs` refuses a non-`test` build of it on every target except x86_64 bare metal; a bare `cargo test` also compiles the non-test lib on the host and trips that rule. The mechanism is [INFERENCE]; the observation is the dossier's. | `cells/services/kms/src/lib.rs:76-83` (`compile_error!`); `reports/k4b-cp2-dossier.md:132` (§7 item 1, "`--no-run` fails too"), `:98` (191 passed, 0 failed, 1 ignored with `--lib`), `:140` (item 9: "the owner's one combined leg should use `--lib` for the persist run"). `--lib` precedent already in CI: `ci.yml:1683-1684,1716-1718`. |
| F4 | `ssh-dev-persist = ["ssh-dev-custody"]`; both guards (`build.rs` refusal under `CELLOS_PRODUCTION`, `compile_error!` with `hardware-relay-provider`) therefore apply to persist. | `cells/services/kms/Cargo.toml:20,25`; `build.rs:19-22`; `lib.rs:70-74`. The persist suite already asserts these strings at source level (`src/tests/ssh/persist/boundary.rs:135-155`), so a separate negative-build step would duplicate it. |
| F5 | `CELLOS_PRODUCTION` is not set anywhere in `ci.yml` (no hit), so the host legs cannot trip the `build.rs` guard. `cells/tools/init/build.rs:11` asserts it **unset**, which is irrelevant here (init is not in the `service-kms` graph). | `grep CELLOS_PRODUCTION .github/workflows/ci.yml` → empty; `git grep` of non-`.agents` sources: `cells/services/kms/build.rs`, `cells/services/silo/build.rs:43`, `cells/tools/init/build.rs:4-11`, two scripts. |
| F6 | The sibling checker and its 13 stdlib-only tests are **not run in CI** (the only `python3 …` test steps are `ci.yml:95,404,1747`). The K8 lanes run the checker only locally. | checker `scripts/check-ssh-dev-persist-exclusion.py:98-116` (`--expect none|custody|persist`, exit 0/1/2); lane use `scripts/qemu-phase09-ssh-test.sh:373-380`. `python3 scripts/test_check_ssh_dev_persist_exclusion.py` → `Ran 13 tests … OK` (run now, tempfiles only). |
| F7 | A KMS dependency graph for `x86_64-unknown-none` + `ssh-dev-persist` contains **no** `cc`, `bindgen`, `cmake` or `littlefs` package, so the variant build needs no cross C compiler (unlike the Build job's x86_64 row, whose `CC_*`/`BINDGEN_*` env exists for `service-vfs`). | `cargo tree --offline --locked -p service-kms --features ssh-dev-persist --target x86_64-unknown-none -e normal,build` filtered for those names: only `aes`, `api`, `cell-build`, `cpufeatures`, `ostd`, `types`, `service-kms` matched. |
| F8 | **The production checker does not reject the SSH dev features or their markers.** `KMS_FORBIDDEN` has five names, `FROZEN_DEV_MARKERS` fifteen; none is an SSH name. Demonstration (copies in `/tmp`, repo untouched), with `--kms-features hardware-relay-provider`, valid net/kernel posture, KMS artifact = K4b CP2 x86_64 builds (`/tmp/k4b-art`, sha256 custody `572fe092…f6ba95`, persist `65a05a28…f75f8b`, default `7b8779ee…a1d1`): unpatched checker exit **3** (the normal ADR-0006 block) for default, custody **and persist**; with `--kms-features hardware-relay-provider,ssh-dev-persist` also exit **3**. | `scripts/check-production-relay-image.py:15-21,40-56,71-87`. |
| F9 | Nothing invokes the production checker or the builder automatically: `build-production-relay-image.sh` validates inputs and `exit 3`s (a Phase-1 stub, `:43`), never calls the checker; the checker's only callers are its own test and plan documents (`git grep check-production-relay-image` outside `.agents/` and `build/`: `scripts/test_check_production_relay_image.py:11`, the sibling checker's docstring `:4`). The builder's test pins exit 3 and stderr == the ADR text exactly (`scripts/test_check_production_relay_image.py:169-195`). There is no production image CI job (`.agents/TODO.md:171`: CI builds default features only; no `production-relay-image` matrix). | Consequence: **there is no production-image pipeline to attach `--expect none` to today**; the only place the "production-like KMS carries no SSH marker" invariant can be enforced now is a CI build of the production-like KMS (A) and, later, the Phase 02/6 builder. |
| F10 | The production checker's test iterates `CHECKER.FROZEN_DEV_MARKERS` for feature rejection, artifact-content rejection and chunk-boundary split. New marker names are therefore covered **without editing the test file**. | `scripts/test_check_production_relay_image.py:89-131`. Patched copy in `/tmp`: `Ran 8 tests … OK` (baseline copy also 8 OK). |
| F11 | Phase 02 owns the checker and its test; Phases 3-7 hand off closed ASCII names "never checker edits", and every handed-off name "enters the frozen marker set here before the handing-off phase proceeds". Phase 02 frontmatter is `status: blocked` while its todo list is all ticked but the last line. K4b CP1 §5 item 5 recorded a **text-only** hand-off (C-K4b-3, confirmed in `k4b-cp1-approval.md:3` "rulings C-K4b-1..3 as recommended"); the hand-off has not been applied. | `.agents/260826-1605-phase4-dev-reference-authority/phase-02-private-protocol-and-dev-separation.md:4,46,74`; `plan.md:57`; `phase-04-…:245`; `k4b-cp1.md:42,215,302`; `k5-cp2-dossier.md:162`. |
| F12 | `ci.yml` is hashed into the attested evidence bundle (`--input workflow.yml=.github/workflows/ci.yml`), but no repository file pins its digest except a **stale** source baseline in the G1/G2 authorization (`8ddd3696…`, already drifted from the current `0d77f752…`). The two ci.yml-parsing guards are unaffected: `check-cargo-config-parity.py` anchors on the first workflow-level `CARGO_TARGET_*` occurrence (`ci.yml:22-33`; verified equal before/after on the proposed text); `check-board-configs.sh:96-99` greps `ci.yml` for three placeholder-board names (0 hits in the proposal). `generate-code-metrics.py` counts `*.rs` only (`:128`). | `ci.yml:2134`; `.agents/261011-1200-cell-contract-authority/scoped-g1-g2-authorization.json:94,125-127`; `scripts/check-cargo-config-parity.py:25-69`. |
| F13 | `f1-admission` is **red at HEAD** independent of this work (`cellos-sign --check --strict`: F5 OK, F1 FAIL with seven items: five `cells/tests/*` crates lacking `forbid(unsafe_code)`, `service-input`, `app-cellbox`; allowlist `review_by 2026-10-28` already past). The new job must not depend on it and must not be read as repairing or masking it. | `python3 scripts/cellos-sign --check --strict` run now (read-only). Own cleanup item (the K8 per-run waiver diff `reports/k8-evidence/f1-waiver-unsafe-allowlist.diff`). |

## 2. Exact minimal fix

### 2.1 Part A — one new job in `.github/workflows/ci.yml`

Placement: after the `unit-tests` job (insert after current line 1722, `cargo test -p service-vfs --target x86_64-unknown-linux-gnu`; before `f1-admission`). Every job at or below line 1722 keeps its line number; the existing cross-references in plans (`ci.yml:139-141`, `:164-259`, `:326-385`, `:648-696`, `:892`, `:954-959`, `:621-625`) are all below 1723 and stay valid.

Conventions copied from the file (reason in parentheses):
* `runs-on: ubuntu-24.04`, `actions/checkout@v4`, `dtolnay/rust-toolchain@master` with `toolchain: nightly-2026-05-01` and `components: rust-src` (all jobs; the pin mirrors `rust-toolchain.toml`).
* `Swatinem/rust-cache@v2` with `cache-on-failure: true` and a job-named `key:` (`shell-utils` `ci.yml:1764-1767`, `rust-std-lane` `:377-380`).
* A top comment stating why the job exists (`rust-std-lane` `:357-360`, `boot-suite` `:1793-1824`), `timeout-minutes` (`rust-std-lane` 45, `boot-suite` 60), no `needs:` and no path filter (only `qemu-*`/`attest-catalog` use `needs`; the workflow `on:` has no `paths`).
* Host tests as `cargo test -p … --target x86_64-unknown-linux-gnu` with `--lib` where a bin cannot host-test (`ci.yml:1683-1684,1716-1718`).
* Cell builds as `cargo build --release -p … --target x86_64-unknown-none -Z build-std=core,alloc` with the step-level `CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS: "-C relocation-model=pic"` that overrides the workflow-level kernel flags (`ci.yml:260-266`).
* No new apt/pip installs: the checker and its tests are stdlib-only (F6, F7). No `--locked` (no existing leg uses it).

Exact diff (generated with `git diff --no-index` against the current file; the patched copy parses with `yaml.safe_load`, 27 jobs, the run scripts pass `bash -n`; checker matrix logic exercised, section 5):

```diff
diff --git a/.github/workflows/ci.yml b/.github/workflows/ci.yml
index 95c73a9a5..b475a8f3e 100644
--- a/.github/workflows/ci.yml
+++ b/.github/workflows/ci.yml
@@ -1721,6 +1721,95 @@ jobs:
           # --lib flag; `driver-disk` ships a staticlib and links std on hosts.
           cargo test -p service-vfs --target x86_64-unknown-linux-gnu
 
+  # KSSH (KMS SSH key custody): `ssh-dev-custody` and `ssh-dev-persist` are opt-in
+  # DEV_POSTURE features that are OFF in every other job, so no other job compiles their
+  # code, runs their ~190 host tests, or proves that the default KMS binary stays free
+  # of their markers. The `unit-tests` job already runs the default `-p service-kms`
+  # suite; this job adds the two feature suites and the artifact proof.
+  #
+  # `ssh-dev-persist` needs `--lib`: `lib.rs` refuses a non-test build of it on any
+  # target but x86_64 bare metal, and a bare `cargo test` also compiles the non-test
+  # lib on the host (K4b CP2 dossier §7.1).
+  #
+  # The artifact step builds the three variants for x86_64-unknown-none (the only
+  # target `ssh-dev-persist` is valid on) and runs scripts/check-ssh-dev-persist-
+  # exclusion.py on each in both directions: the `none` build, made under
+  # CELLOS_PRODUCTION=1 like a production-like candidate, must carry no marker, and the
+  # custody/persist builds must carry exactly their own. It is a sibling of
+  # scripts/check-production-relay-image.py (Phase 02 owns that file); it does not
+  # replace it.
+  kms-ssh-dev-posture:
+    name: KMS SSH dev custody (host tests + variant markers)
+    runs-on: ubuntu-24.04
+    timeout-minutes: 30
+    steps:
+      - uses: actions/checkout@v4
+
+      - name: Install Rust toolchain
+        uses: dtolnay/rust-toolchain@master
+        with:
+          toolchain: nightly-2026-05-01
+          components: rust-src
+
+      - uses: Swatinem/rust-cache@v2
+        with:
+          cache-on-failure: true
+          key: kms-ssh-dev-posture
+
+      # Standard library only; no pip install.
+      - name: Unit-test the KMS SSH exclusion checker
+        run: python3 scripts/test_check_ssh_dev_persist_exclusion.py
+
+      - name: Host tests (ssh-dev-custody, ssh-dev-persist)
+        run: |
+          cargo test -p service-kms --features ssh-dev-custody \
+            --target x86_64-unknown-linux-gnu
+          cargo test -p service-kms --features ssh-dev-persist --lib \
+            --target x86_64-unknown-linux-gnu
+
+      - name: Build the KMS variants and prove which one each is (x86_64-unknown-none)
+        env:
+          # Cell codegen, as the Build job's cells step: only the kernel is
+          # static/code-model=kernel. The markers are .rodata strings, so the flag set
+          # does not change the verdict.
+          CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS: "-C relocation-model=pic"
+        run: |
+          set -euo pipefail
+          out="$RUNNER_TEMP/kms-variants"
+          mkdir -p "$out"
+          built=target/x86_64-unknown-none/release/service-kms
+
+          CELLOS_PRODUCTION=1 cargo build --release -p service-kms \
+            --target x86_64-unknown-none -Z build-std=core,alloc
+          cp "$built" "$out/none"
+          cargo build --release -p service-kms --features ssh-dev-custody \
+            --target x86_64-unknown-none -Z build-std=core,alloc
+          cp "$built" "$out/custody"
+          cargo build --release -p service-kms --features ssh-dev-persist \
+            --target x86_64-unknown-none -Z build-std=core,alloc
+          cp "$built" "$out/persist"
+
+          # expect VARIANT FEATURES EXPECT WANTED_EXIT. Both directions are asserted, so a
+          # checker that stopped discriminating fails here instead of passing everything.
+          expect() {
+            local rc=0
+            python3 scripts/check-ssh-dev-persist-exclusion.py \
+              --kms-features "$2" --kms-artifact "$out/$1" --expect "$3" || rc=$?
+            if [ "$rc" -ne "$4" ]; then
+              echo "FAIL: $1 build, --expect $3 exited $rc (wanted $4)" >&2
+              exit 1
+            fi
+          }
+          expect none    ""              none    0
+          expect none    ""              custody 1
+          expect none    ""              persist 1
+          expect custody ssh-dev-custody none    1
+          expect custody ssh-dev-custody custody 0
+          expect custody ssh-dev-custody persist 1
+          expect persist ssh-dev-persist none    1
+          expect persist ssh-dev-persist custody 1
+          expect persist ssh-dev-persist persist 0
+
   f1-admission:
     # Tier-1 admission gate (Spec 18 §2.1): the same `cellos-sign --check` that
     # the image scripts must pass before anything is signed. Replaces the
```

Design notes:
* The three `cargo test`/`cargo build` forms are exactly the ones the K4b/K5 dossiers record as green: custody `122 passed, 0 failed, 1 ignored` (`k4b-cp2-dossier.md:97`), persist `--lib` `191 passed, 0 failed, 1 ignored` (`:98`; the 1 ignored is the KDF timing measurement and stays ignored). The default suite is not repeated (already `ci.yml:1689`). No `--release` test run (the 64 KiB stack tests are also green in debug on a 256 KiB thread, `k4b-cp2-dossier.md:86`).
* The `none` variant is built under `CELLOS_PRODUCTION=1` so the artifact is the production-like default build and the `build.rs` refusal path is armed; K5/K4b dossiers record that this default build passes (`k5-cp2-dossier.md:127`). The other two builds omit it (K8 does the same, `scripts/qemu-phase09-ssh-test.sh:244-247` `env -u CELLOS_PRODUCTION`).
* Each variant is a separate `cargo build` invocation so features are never unified across variants (the K8 note at `scripts/qemu-phase09-ssh-test.sh:242-244`); `cp` after each build is required because cargo overwrites `release/service-kms`.
* Expected matrix (nine rows) equals the observed K4b result `reports/k4b-cp2-dossier.md:127` and was re-run now on the stored K4b artifacts: default none 0 / custody 1 / persist 1; custody 1 / 0 / 1; persist 1 / 1 / 0.
* Negative control: the `expect` helper asserts the **failing** direction too (exit 1, not merely non-zero, so a usage exit 2 cannot masquerade as a rejection).
* Not added (duplicates source-level proof or has unmeasured cost): negative builds of `CELLOS_PRODUCTION=1 --features ssh-dev-custody`, `hardware-relay-provider` + custody, and persist on riscv64/aarch64 (these need extra `-Z build-std` targets; the textual guards are asserted by `boundary.rs:135-155`). Offered as D-CI-4.

### 2.2 Part B — production-image marker hand-off (text now; edit only if the owner rules)

Closed ASCII names to hand to Phase 02 (the five of `scripts/check-ssh-dev-persist-exclusion.py:32-42`): `CELLOS-KSSH-T1-DEV-ROOT-v1`, `DEV_POSTURE ssh-dev-persist`, `DEV_POSTURE ssh-dev-custody`, `ssh-dev-persist`, `ssh-dev-custody`. The banners printed at `cells/services/kms/src/main.rs:19-24` begin with the two `DEV_POSTURE …` names; the root label is `cells/services/kms/src/ssh/persist/devroot.rs:11`.

**Option B1 (exact edit, for the Phase 02 owner; NOT in the CI closure).** One hunk in `scripts/check-production-relay-image.py`, inserting the five names into `FROZEN_DEV_MARKERS` in ASCII order (this gives both feature-list rejection through `DEV_MARKER_NAMES`, `:86`, and artifact-content rejection through `DEV_MARKERS`, `:94`, so `KMS_FORBIDDEN` need not change):

```diff
--- a/scripts/check-production-relay-image.py
+++ b/scripts/check-production-relay-image.py
@@ -40,4 +40,7 @@ FORBIDDEN_MARKERS = tuple(
 FROZEN_DEV_MARKERS = [
     "AWS_DEV_SIGNED_TIME",
+    "CELLOS-KSSH-T1-DEV-ROOT-v1",
+    "DEV_POSTURE ssh-dev-custody",
+    "DEV_POSTURE ssh-dev-persist",
     "DEV_REFERENCE",
     "SLB9672",
@@ -52,4 +55,6 @@ FROZEN_DEV_MARKERS = [
     "root-stream",
     "slb9672-dev-anchor",
+    "ssh-dev-custody",
+    "ssh-dev-persist",
     "stm32h573i-dk-dev-authority",
     "vf2-root-stream",
```

Observed on a `/tmp` copy of the two Phase 02 scripts (repo untouched): `test_check_production_relay_image.py` 8 tests OK (it iterates the list, F10); with the same candidate inputs as F8 the patched checker exits **1** for the custody artifact (`contains forbidden DEV marker: DEV_POSTURE ssh-dev-custody`), exits **1** for the persist artifact (`…CELLOS-KSSH-T1-DEV-ROOT-v1`), exits **1** for `--kms-features hardware-relay-provider,ssh-dev-persist` (`KMS forbidden DEV marker: ssh-dev-persist`), and still exits **3** with the unchanged ADR text for the default artifact. Resulting file would hash `b0b97346…6cd29` (`b0b973467527c3d05388cb1252dfbe0d8c43dd602e477df45f9f8a256876cd29`) on top of the current `5cb07d35…`. The test file and `build-production-relay-image.sh` are unchanged.

**Option B0 (no Phase 02 edit).** Keep K4b's text-only hand-off, and rely on (i) the CI `none` variant (A), which fails if the default, `CELLOS_PRODUCTION=1` KMS build ever carries a marker, and (ii) a requirement recorded for the later production builder (Phase 6 / ADR-0006 unblock): after the production checker validates its inputs, call `scripts/check-ssh-dev-persist-exclusion.py --kms-features "$KMS_FEATURES" --kms-artifact "$KMS_ARTIFACT"` (default `--expect none`) and abort on a non-zero exit **before** the BLOCKED message path. That stub cannot host the call now: `build-production-relay-image.sh` has no artifact inputs and its test pins exit 3 and the exact stderr (F9). B0 leaves F8 open for real production candidates until Phase 02 acts.

### 2.3 Part C — changelog record

`CHANGELOG.md` (current sha256 `5c2022ac…eceb`): one bullet after the KSSH K8 RT1 bullet (`CHANGELOG.md:1146`), same section, text:

> - KSSH CI: new job `kms-ssh-dev-posture` runs the `ssh-dev-custody` and `ssh-dev-persist` (`--lib`) host suites, the exclusion checker's tests, and builds the default (`CELLOS_PRODUCTION=1`), custody and persist `service-kms` for `x86_64-unknown-none`, asserting `scripts/check-ssh-dev-persist-exclusion.py --expect none|custody|persist` in both directions. DEV_POSTURE tooling only; no source or ABI change. Production-checker marker hand-off recorded in the K4b/K5 dossiers and `.agents/261030-1000-kms-ssh-key-custody/cleanup/ci.yml.md`.

`docs/code-standards.md` (KMS SSH custody bullet, `:87-98`) needs no edit; no behaviour changes.

## 3. Exact file closure (current SHA-256 at HEAD `ca4b872e6`)

**CI slice (A + C) — edited:**

| Path | SHA-256 now | After (expected) |
|---|---|---|
| `.github/workflows/ci.yml` | `0d77f752b62a2c92e1b89bdde366caf911ca553ca7bb41638942e7fc0def9a79` | `731e5ffa99edb338222a0c9bc153069217dc89292821987dc9b8f1c682e62a9f` (exactly the diff above applied to the current file; 2164 → 2253 lines) |
| `CHANGELOG.md` | `5c2022ac045b81cae884e8204994d0ffd4f2b116633d4f1181d89ccb87dceeb1` | changes with the bullet (main agent) |

**Read-only inputs that must stay byte-identical (stop if they drift):**

| Path | SHA-256 |
|---|---|
| `scripts/check-ssh-dev-persist-exclusion.py` | `71ceab71e8f04429a72dc70be8d312c987b8c0fd0c5b9c19ffbfc00729548735` |
| `scripts/test_check_ssh_dev_persist_exclusion.py` | `d2d2231a5d11d9a6a0cacf0754c8f601416911c4b1d1539705f3b4a3fec5c6d3` |
| `cells/services/kms/Cargo.toml` | `6384bcf565520dd99d43c88da69d2ca3cb5a10b51616f2d313e237cf17e55ca0` |
| `cells/services/kms/build.rs` | `792505f3f152be9c322eb27666105777a61a1b0e3604254e882d76711373e900` |
| `cells/services/kms/src/lib.rs` | `5023526eda64557c6688c4f3bf69ecaef24874d8fc5b31acce30b7b97d065c95` |
| `cells/services/kms/src/main.rs` | `7fc9a2ba634a020af73efa3f43c214761acb82d4e7fe2e3ccb1eb281d4bdf516` |
| `cells/tools/kms-ssh-admin/Cargo.toml` | `0ac4dc992c38af05e122ea38351f8d0da628cf3f241193e7a09a9632c59239be` |
| `Cargo.lock` | `5181251ccc712f27f71db9861b1b4feab0a8fec910e43716a7b744a4f8c81130` |
| `rust-toolchain.toml` | `b4b678c357f82bd2a267cfc27cb005562e054ae4aebb072bda9a0af14847edb1` |
| `scripts/qemu-phase09-ssh-test.sh` | `89b7d76062e0fd20741acde18e97e0bc764013b05362a824d060911af9bd0fc7` |
| `docs/code-standards.md` | `9cb004adc0db8f4b1da77b78419507c3c936d76b23370e68752be54007da1185` |

**Phase 02 hand-off (B1 only; NOT in the CI closure; edited only with the Phase 02 owner's ruling):**

| Path | SHA-256 now |
|---|---|
| `scripts/check-production-relay-image.py` | `5cb07d35ee48434e54aa6210590c74e473a2ea6da54c7640136af33f29af7d65` |
| `scripts/test_check_production_relay_image.py` (unchanged) | `b13e1c298fb6581fbd7dc33840fc08d241b74f1ed9f3608c16be3eadf8e74304` |
| `scripts/build-production-relay-image.sh` (unchanged) | `3ee7736b91a8a9478e03257c488082ebfe3fda2ffe5288dbcbb88f96fa560cda` |

No `Cargo.lock`, `Cargo.toml`, cell, `libs/**`, `kernel/**`, image, signing, `config/` or `.github/workflows/{security,perf,docs,release,dco}.yml` change.

## 4. Locks, Law-1 and overlapping plans

(grep of `.agents/cell-contract-0024-locks.md` and `.agents/plan-portfolio.md` for `ci.yml`, `.github`, `check-production-relay` returned **no hit** in either file; "inventory is path-silent for CI" is therefore a finding, not an exhaustive clearance — [INFERENCE] where marked.)

| Item | Relation to these paths | Needed |
|---|---|---|
| **Law 1** (`libs/api`, `libs/types` append-only) | None: no `libs/**` file in either closure. | Not applicable; no confirmation text. |
| **CELL-CONTRACT-0024** adopted slices `locks.md:113` (Cellbox Phase02), `:115` (K2 CP2), `:117` (K3), `:119` (K5), `:121` (K7), `:123` (K4b) | **None lists `.github/workflows/ci.yml`.** K4b CP1 §13 and D-4b.15 put CI outside its closure and leave "one combined leg with G-6" to the owner; K5 CP1 G-6 (`k5-cp1.md:34`) is the same pending item; `k8-rt1-amendment.md:354` repeats it ("owner / main agent"). | This edit is **not** covered by any adopted slice: it needs its own owner approval (a CP1 approval of this text, or a one-line lock-handoff naming the single file). It is not a source edit of any locked plan's scope. |
| `260825-1726-kms-silo-production-root` (`locks.md:155`, in-progress), `260826-1605-phase4-dev-reference-authority` (`locks.md:157`, `status: blocked`, `executionStatus: blocked`) | Phase 02 sole-owner rule for `scripts/check-production-relay-image.py` and its test (`phase-02:46,74`; `plan.md:57`; Phase 04 `:245`, Phase 08 context links `:20`). | **B1 conflicts with the ownership rule unless the owner rules**; A does not touch those files. Part A only *runs* the sibling script. |
| Locked plans whose documents mention `ci.yml` (`locks.md:127` 260528-2016, `:138` 260712-0800, `:145` 260727-2101, `:163` 261004-1957) | Mentions are historical job recipes / line references (`260727-2101` phase 11 created `f1-admission`; `261004-1957` cites the job at `ci.yml:892`); none reserves the insertion point. All cited line numbers are < 1723 and do not move. | None; [INFERENCE] not exhaustive (only `git grep -l "workflows/ci\.yml"` over `.agents/` plan files was read). |
| `261011-1200-cell-contract-authority` scoped G1/G2 authorization (`scoped-g1-g2-authorization.md:36`, `.json:94`) | Lists `.github/workflows/ci.yml` as an allowed path for CellosFS fixture compatibility, with a pinned source baseline (`.json:125-127`, `8ddd3696…`) that has **already drifted** (current `0d77f752…`). | A second owner edit lands on a path another authorization names; the G1/G2 owner must rebase on the post-edit hash `731e5ffa…`. Not a path conflict at the job level (different region). |
| `260914-ci-gate-restoration` (`locks.md:295`, baseline, "no new lock") | Closed (all 23 jobs green at `2bb82e50a`, 2026-09-15). Adding a job re-opens nothing there. | None. |
| `261020-1750-cellbox-cli` | Phase 07 precedent (`phase-07-s0-admission.md:214`): a CI line "outside the approved file set" was left to the owner rather than edited — same posture as D-4b.15. | None. |
| `plan-portfolio.md:126-128` | KSSH state paragraph; no CI claim. | Optional one-clause update when A lands (main agent). |
| S10 census (K8 gate G-6) | The census lists `build/**` artifacts (546 paths, 470 present), not repo sources; `ci.yml` and the scripts are absent from it. | None. |

## 5. Verification

**Run now, read-only (this proposal's evidence):**
* `python3 scripts/test_check_ssh_dev_persist_exclusion.py` → 13 OK; `python3 scripts/test_check_production_relay_image.py` → 8 OK.
* `python3 scripts/cellos-sign --check --strict` → F5 OK, F1 FAIL (7 pre-existing items, F13).
* Checker matrix on the stored K4b CP2 x86_64 artifacts (not rebuilt now): the nine rows of 2.1 reproduce (0/1/1, 1/0/1, 1/1/0). The proposal's `expect` block (extracted from the patched YAML) exits 0 on them and exits 1 with its message when one wanted value is flipped.
* Patched `ci.yml` copy in `/tmp/c3ci`: `yaml.safe_load` OK (27 jobs), run scripts `bash -n` OK, `check-cargo-config-parity` workflow flags equal before/after, 0 placeholder-board names.
* `cargo tree --offline --locked` for the feature/target graph (F7). `git status --short` unchanged by any of the above.

**To run at CP2 (hosted CI cannot be run locally; local equivalents):**
1. Apply the diff; `git diff --stat` = one file, +89.
2. `python3 -c 'import yaml;yaml.safe_load(open(".github/workflows/ci.yml"))'`; `python3 scripts/check-cargo-config-parity.py`; `bash scripts/check-board-configs.sh` (the last two read `ci.yml`).
3. Local mirrors of the job (private `CARGO_TARGET_DIR`): `cargo test -p service-kms --features ssh-dev-custody --target x86_64-unknown-linux-gnu` (expect 122 passed, 1 ignored); `cargo test -p service-kms --features ssh-dev-persist --lib --target x86_64-unknown-linux-gnu` (expect 191 passed, 1 ignored); the three `cargo build --release -p service-kms … --target x86_64-unknown-none -Z build-std=core,alloc` builds with the step's flag set, then the nine `expect` rows.
4. First hosted run: the new job green; the existing `unit-tests` default KMS leg unchanged; record job duration for D-CI-5.
5. Prove the job can fail: on a throwaway branch (not committed here) delete the persist banner line `main.rs:21-24` or flip one expected exit, and observe the job fail with the `FAIL:` line.
6. B1 only: `python3 scripts/test_check_production_relay_image.py` (8 OK expected) and the F8 demonstration with the three artifacts (`1`, `1`, `3` for custody, persist, default).

## 6. Risks

1. **Runner minutes / build-std cost.** Three release builds with `-Z build-std` plus two host test compiles (`ed25519-dalek`, `aes`, `bcrypt-pbkdf`); not measured. Mitigation: dedicated cache key, `timeout-minutes: 30`, no `needs`; split into two jobs if D-CI-5 data says so.
2. **First hosted run may expose an environment difference** not seen locally (K4b built on the author's machine; CI has no `.cargo/config.toml`). Mitigation: the step sets the same per-target flag env the Build job uses; F7 shows no C toolchain is required [INFERENCE for runtime].
3. **`--lib` rationale is observed, mechanism inferred** (F3). If a later change makes bare `cargo test` work, `--lib` is merely redundant.
4. **Marker-string brittleness.** The `none` leg fails if any future default code path prints or embeds one of the five names (for example a doc string in `.rodata`). That is the intended behaviour; the fix then is to rename the string, not relax the checker.
5. **Checker proves identity, not safety** (its own docstring `:15-16`): a green job shows the three variants are distinguishable; it says nothing about a different binary or about key custody. `DEV_POSTURE` ceiling unchanged.
6. **B1 edits a Phase 02 file** whose plan is `blocked`; the ruling must say it is a closed-name hand-off, not a checker redesign. Adding the two feature names also makes the production checker reject them in net/kernel feature lists (harmless; none exist there).
7. **Job name/count drift.** Docs that quote "N jobs" or job lists would need updating if any exist ([INFERENCE]; none found by the `ci.yml` greps above).
8. **Evidence bundle.** `ci.yml` is an attested input (`ci.yml:2134`); the new digest enters the next bundle automatically; no stored expectation needs updating (F12) except the already-stale G1/G2 pin.
9. **Red neighbour jobs** (F13) can be mistaken for this job's failures in the Actions summary; the job name and the dedicated cache key keep them separable.

## 7. Decision table

| ID | Decision | Options | Recommendation |
|---|---|---|---|
| D-CI-1 | Approve adding `.github/workflows/ci.yml` (one file, +89 lines) as the single closure of this slice | Approve / defer | **Approve** (resolves K5 G-6 and K4b D-4b.15 "owner wires one combined leg") |
| D-CI-2 | Job shape | (a) new job `kms-ssh-dev-posture`; (b) append the two `cargo test` lines to `unit-tests` and skip the artifact step | **(a)**: failure attribution, own cache, the artifact step needs a different env and rust-src build that `unit-tests` does not have |
| D-CI-3 | Production checker marker hand-off | **B1** owner-approved 5-line edit of `scripts/check-production-relay-image.py` / **B0** text hand-off + CI `none` leg | **B1** if the Phase 02 owner accepts a closed-name edit (closes F8 now; test file needs no change); otherwise **B0** with F8 recorded as open until Phase 6 |
| D-CI-4 | Add negative build steps (`CELLOS_PRODUCTION=1 --features ssh-dev-custody`; custody + `hardware-relay-provider`; persist on riscv64/aarch64) | add / omit | **Omit**: textual guards are already asserted by `boundary.rs:135-155`; negative builds need extra build-std targets. Add later if a guard regresses |
| D-CI-5 | Single job vs split host-tests / artifact | single / two jobs | **Single** now; split only if the first run exceeds ~20 minutes (unmeasured) |
| D-CI-6 | Also run `cargo test -p app-kms-ssh-admin --target x86_64-unknown-linux-gnu` (71 tests, K7) in this job | add / out of scope | **Out of scope** for this item (separate decision; it needs the dev-dependency feature unification and has no CI coverage today); one line can be added to the same step if the owner wants it |
| D-CI-7 | Also run `python3 scripts/test_check_production_relay_image.py` (8 tests, green now, never run in CI) | add to this job / leave to Phase 02 | **Leave to Phase 02** (it owns the file); one line if the owner prefers |
| D-CI-8 | Changelog bullet | add / omit | **Add** (main agent; see 2.3) |
| D-CI-9 | Lock bookkeeping | record in the locks file / none | **Record** one sentence next to the K4b paragraph (`locks.md:123`) once approved, since no adopted slice names `ci.yml` |

## 8. Not done / not observed

No hosted CI run; no `cargo test`/`cargo build` was executed for this document (build counts and artifact hashes above are the K4b/K5 dossiers' and the stored `/tmp/k4b-art` files, which predate the final K4b commit only in the sense that they are the CP2 evidence builds, not re-taken at HEAD). Job duration and cache behaviour on a hosted runner are unknown. The root-label presence in a fresh persist build is taken from `k4b-cp2-dossier.md:128` and from the stored artifact (exit 0 for `--expect persist`). Line-number evidence in the Phase 02 and lock files was read at the SHA-256s listed in section 3 plus: locks file `258e920e384a0c939657b318cf737ffc7d22bb4c4b12d1b2a736025512311499`, `plan-portfolio.md` `baa97c210d3320d3c40ecfeea0316face1d592eb870ccbc0c31a9e90fa801f1b`, Phase 02 file `ebc1ec1b658faf04e088e9e9de5ebe64922e934bcfde187e2001ecd9ef469e88`, Phase 4 plan `6e85c9069d992b6bf7b163a5173c0d9f16a54f7a68f7c2c1629bf0a91561ab38`. Throwaway work copies are under `/tmp/c3ci` (outside the repository).
