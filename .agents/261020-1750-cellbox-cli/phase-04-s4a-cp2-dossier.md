# S4a CP2 dossier — applets retirement, recipe/doc fixes, `cat` exit status

**Status: CP2 APPROVED by the user on 2026-10-22 (“duyệt 2 CP2”) for dossier SHA-256 `901ae31a8987289b78b170d61f704e6699ed147da29bc82d8e03109b4d2c02b4` (digest before this status amendment).** CP2 would authorize the S4a source as the S4a implementation baseline **only**: the deletions and the hunks attributed to S4a below, at the exact file hashes listed. Quarantine, canonical packaging, release signing and rollout stay **BLOCKED**.

Prepared 2026-10-22 under D-CL2 of the [closure review](phase-04-closure-review.md) ("S4a … covered by the S4 CP1 only"), from:
- the current source bytes;
- read-only `git diff` against the index/`HEAD` (`3471028bc`); the index copies of `cells/tools/cellbox/src/*` are the S1 `git mv` of `HEAD:cells/tools/shell/src/*` (identical blobs);
- the saved logs in `reports/s4-final-evidence/` and `reports/c5-evidence/{phase04-terminal.log,phase03.log,phase02.log}`.

The only new execution was `sha256sum`/`stat`/`find -newermt` and read-only git/grep. No build, test or QEMU run was performed for this dossier.

## Authority

- [S4 retirement proposal](phase-04-s4-retirement-proposal.md): **CP1 APPROVED** 2026-10-22 ("xoá applets, sửa exit status của cat, làm cả S4a + S4b"): D-S4-1 delete `applets`, D-S4-2 fix `cat` in S4, D-S4-3 S4a + S4b.
- Results are in the proposal's §Results S4a. This dossier adds no scope.
- The four S4b CP2s (OOM, S2-K, S2-N, S2-T/W) are approved for their own hunks only. Three files below are shared with them; their approvals do not extend to S4a hunks ([cutover log](phase-04-multicall-cutover.md) line 88: "S4a hunks are covered by the S4 CP1 only (no separate CP2)").

## Delta summary

1. **`applets` retirement (D-S4-1).** Deleted `cells/tools/cellbox/src/dispatch.rs` and `cells/tools/cellbox/src/applets/**`, the non-default `applets` feature in `cells/tools/cellbox/Cargo.toml`, and their `cfg` hooks in `main.rs`. The module was unreachable: `main()` always ran `run_shell` and nothing called `dispatch_applet` (proposal §1). Ledger tests and catalog §3.1 wording were updated.
2. **Recipes.** `scripts/gen-disk-ci.sh`: the `kernel_fs.img` FAT assertion requires `LFN 'cellbox'` instead of `LFN 'shell'`. `scripts/make-hypervisor-fs.sh`: the embedded-cell loop probes `cellbox`, with an explicit `$BIN_DIR/cellbox` case.
3. **Docs.** The `state_transfer.rs` module doc and four docs (`getting-started`, `project-overview-pdr`, `project-roadmap-legacy`, `guides/tier1b-lua`) now name `/bin/cellbox` / `/bin/cellbox ["sh"]` instead of `/bin/shell`.
4. **`cat` exit status (D-S4-2).** `commands::cmd_cat` diagnoses a missing or denied operand, continues with later operands, and returns the first failure (`NotFound` / `PermissionDenied`), so the status is non-zero (POSIX). The Phase04 driver's `sandbox-no-file` check is status-based again.

**Unchanged:** syscall ceiling (G-S1a), manifest, kernel, ABI, launch profiles (`profiles.rs` was edited and reverted; see deviation 3), `Cargo.lock`, canonical images and signing.

## Evidence

Two saved runs exercised the **combined** tree (every earlier delta plus S4a). `s4` = `reports/s4-final-evidence/` (the S4a Results run). `c5` = `reports/c5-evidence/` (later run on the same S4a bytes plus C5).

| Check | Saved log lines | Result / note |
|---|---|---|
| Cellbox ceiling unchanged | s4 `phase04-terminal.log:4-6`, c5 `phase04-terminal.log:4-6` `G-S1a PASS: declare_manifest and declare_syscalls sets are identical` | The 32→33 count is the pre-S1 `Write` delta noted at `:3`; not S4a |
| Default cellbox build after the deletion | s4 `terminal:44` `Compiling app-cellbox v0.1.0`, `:227` `generated 2 warnings`; c5 `:46`, `:228` | The two warnings (`executor.rs:415` `paste`, `parser.rs:128` `BackgroundPipeline` field) are the same two seen before S4a (`reports/s3-gui-tty-evidence/s3.runner.log:101-113`). S4a introduced no new warning in this build. The runners build default features only (runner 245-248), so `applets` was never in any image; the runtime runs show no regression of the default build, not the deletion itself |
| `cat` of a missing file exits non-zero (`sandbox-no-file`) | s4 `terminal:593` `PASS marker:sandbox-no-file`; s4 `phase04-serial.log:1577` `P04 INFO stderr cmd=cat /tmp/s2tw-probe-sb text=cat: /tmp/s2tw-probe-sb: No such file or directory`, `:1578` `P04 PASS sandbox-no-file`; c5 `terminal:595` | The check requires `stsbcat=` ≠ `stsbcat=0` (current driver 896, 904). **The status value itself is not printed in any log**; only the PASS of the condition is |
| `cat` of a denied operand exits non-zero | s4 `terminal:568` `PASS marker:secret-denied-0`; serial `:1514` `cat: /srv/cellos/p04-secret: Permission denied`, `:1515`; c5 `terminal:570` | `secret-denied-0` requires `sk=` ≠ `sk=0` (S2-K check). Exercises the `failure.get_or_insert(PermissionDenied)` path |
| Successful `cat` still exits 0 | s4 `terminal:598` `PASS marker:control-file-created`, `:535` `file-cat-roundtrip`; c5 `:600`, `:537` | `control-file-created` requires `stokcat=0` (driver 918-925) |
| Whole runner | s4 `terminal:871-878`, `:878` `PASS: 353 Phase04 S3-output runtime checks (87 driver markers)`; c5 `:873-880`, `:880` `PASS: 354 … (88 driver markers)` | 0 `FAIL`/`GAP` lines in both. c5's extra check is C5's `headless-no-ui-tasks` (`:533`), not S4a |
| Phase03 regression | s4 and c5 `phase03.log:397-456` (60 `PASS`), `:457` completion | 60/60 both |
| Phase02 regression | s4 and c5 `phase02.log:407` `passes: 48`, `:409` completion | The logs print no exit status; "exit 0" comes from the S4a and C5 Results |
| Host `app-cellbox` 89/89, `x86_64-unknown-none` check (+`shell_test`), `bash -n` on both scripts | **No raw log saved.** Recorded only in proposal §Results S4a | Not re-run here |
| `gen-disk-ci.sh` FAT assertion on a scratch image (passes with `/bin/cellbox`; fails with `/bin/shell` or no cellbox) | **No raw log saved.** Recorded only in proposal §Results S4a | Not re-run here; no canonical RV64 image was produced |
| `make-hypervisor-fs.sh` | **Not exercised** beyond `bash -n` (Results) | No AArch64 hypervisor image was built |
| Docs | No check log | Read-only diff only (below) |

Evidence file SHA-256:

| File | SHA-256 |
|---|---|
| `s4-final-evidence/phase04-terminal.log` | `ad492a2f74e63e4a8c059e2bfba7d16bc3d7ce44444d89bca7ca3c051a5dbd43` |
| `s4-final-evidence/phase04-serial.log` | `7153285123116ffebe2f919fa074d7c5f1b560830ffe21a99f13e8c962406f93` |
| `s4-final-evidence/phase03.log` | `c46d7e3f79d1c203df26fe4279725818ad8879442609f71b853eb2659e500ec7` |
| `s4-final-evidence/phase02.log` | `592a99e3d6236f7cbfb7f3546424a2c6f5381326f4445a9041b6f5486cfc69f2` |
| `c5-evidence/phase04-terminal.log` | `381c6cab923531e7c1a180a2f870e7275508d7bf06b4f72bb5505343ccaa8898` |
| `c5-evidence/phase03.log` | `c4b3fc5ac3827c10057b80db40ae73857e4f218a3a5104b11b4bf2d777efd01f` |
| `c5-evidence/phase02.log` | `b989ebfce90d5404efc4dc17e14c9860de3d5afb31992c5f479f4c5822735ad1` |

**Evidence ↔ current source.**
- Both runs' `cargo_lock_sha256` (s4 `terminal:486`, c5 `:487`) equal the current `Cargo.lock` (`e58e780a…18f6`).
- The runner pins `launch_profile/{mod,profiles,targets,tests}.rs` (runner 202-215, fails before key generation). Both runs reached key generation (s4 `:12`, c5 `:13`), and the current `profiles.rs` equals its pin `3140c61b…`.
- No other file has a per-file hash in the evidence. Saying the runs used exactly these bytes is `[INFERENCE]`. Every S4a file's mtime (18:08:30–18:14:11; the `src/` directory entry change at 18:08:30 marks the deletion) precedes the s4 log copies (18:29:51) and the c5 log copies (20:48–21:11).
- **Driver bytes differ between the runs.** The s4 run used the driver hashed by the S4b dossiers, `0139d5cc…` (53581 B) `[INFERENCE]`: those dossiers were hashed after 18:29 and before the 19:57 C5 edit. The c5 run used the current driver `00324c71…` (53963 B, mtime 19:57) `[INFERENCE]`. Both carry the status-based `sandbox-no-file` condition: the S2-T/W dossier records it at line 896 of `0139d5cc…`; it is at line 904 now.

## Proposed implementation inventory — current disk SHA-256

**Deleted (no bytes remain):** `cells/tools/cellbox/src/dispatch.rs` and `cells/tools/cellbox/src/applets/**`, including the `applets/sys.rs` (`cmd_cat`, text helpers) and `applets/ssh.rs` (argv-only, [phase-09-ssh-client.md](phase-09-ssh-client.md) line 50) sources. They were untracked, so git holds no copy and no hash of them; their absence is the reviewed state. Grep finds no `dispatch_applet`, `mod dispatch`, `applets` feature or `--features applets` anywhere outside `.agents/` history.

**Exclusive to S4a** (every changed hunk is S4a):

| Path | State | Bytes | SHA-256 | S4a hunk |
|---|---|---:|---|---|
| `cells/tools/cellbox/src/state_transfer.rs` | modified | 6409 | `85b03299c9ab3044a3307b49ac85a7991e13ba47792d1737ce6bc0d6ed188c77` | module doc 1, 6-10 (only diff vs index) |
| `docs/getting-started.md` | modified | 18797 | `37665656d7ced81cb4a32f9cbb03ec710d1d7270e6dfffd3f6a67e13e18ad8c8` | autoload example 149-151 (`cellbox`, `/bin/cellbox`, `args = ["sh"]`) |
| `docs/project-overview-pdr.md` | modified | 28851 | `f577e49a62cb37e55007e0ae4a371713c680ab72cf4bfcd1d017e4db75f0109b` | 213 |
| `docs/project-roadmap-legacy.md` | modified | 154787 | `e02911d79d8b3b6948171dd3d7d9b9517ed33bb89c4bfddc5067e70c9aee34de` | 868 |
| `docs/guides/tier1b-lua.md` | modified | 5835 | `d66c298080ac5465e1490742a8753bc76ed32615e861bd9c80d7c6aa9d8d3eb4` | 56 |

**Shared files.** Each is hashed once, as the whole file, and the hash covers every delta's hunks. Approving this dossier covers only the S4a hunks named here.

| Path | Bytes | SHA-256 | S4a hunks (current lines) | Other deltas in the same file |
|---|---:|---|---|---|
| `cells/tools/cellbox/Cargo.toml` (untracked) | 987 | `610860cd9eea41cf58d0f59540edc62074ae15d0d0b18a638c938297113ee082` | absence of the `applets` feature in `[features]` (11-17 now holds only `shell_test`) | S1 package rename, `[[bin]] cellbox`, `shell_test` comment; Phase02 `terminal-sdk` dependency |
| `cells/tools/cellbox/src/main.rs` | 7759 | `0396292e55230c97434e27d09990dedf28dd1220346657c1c6baa6fd00966806` | absence of `mod dispatch` / `#[cfg(feature = "applets")]` hooks; entry docs 108, 115-116 (by symbol) | S1 rename comments (7-9, 20-23); Phase02 `Write` syscall, `context_session`, `run_shell`/serial session; S2-lite `ledger` module (73-75); Phase03 `native_rows`/`typed`. The index diff (S1 → now) mixes all of them |
| `cells/tools/cellbox/src/ledger.rs` (untracked) | 13170 | `9d00bcf83af1c8dac118df83ee92376e86b0593e91d5dc98e793f9db1c80fc70` | updated tests (Results); no `applet` reference remains | S2-lite ledger, S2c refused entries/tests |
| `cells/tools/cellbox/src/commands.rs` | 24162 | `2e2fe670247b31407e054c24d2979243d7a52b9501f3ffd3d4a115d14f1048fd` | `cmd_cat`: POSIX comment and `failure` 366-368; `failure.get_or_insert(PermissionDenied)` 394 (inside the S2-K gate); `failure.get_or_insert(NotFound)` 454; `match failure` return 458-461 | **S2-K** `cat` gate 389-396, `ls` gate/status; S2c refused `kill`/`snapshot`; S3 stdout/stderr routing; `ls_collect` |
| `.agents/261020-1750-cellbox-cli/command-catalog.md` (untracked) | 63329 | `68e259e6da0204c8c825191c01203800de1c733244894055923be3b9fe9303c8` | §3.1 sentence at 127 ("The non-default `applets` multicall module … was deleted in Phase 04 S4a; the suite's only entry is `cellbox sh`.") | Phase01 catalog; S2-lite §3.1 ledger table (generated, byte-checked by the ledger host test) |
| `scripts/gen-disk-ci.sh` | 30614 | `e3bd10b70798e3ef5e7007af49d10ec61288b7a7d916002d2f37e36711c24a61` | FAT assertion `LFN 'cellbox'` (453) | `app-shell`→`cellbox` renames (39, 153, 209, 232-238, 246, 267, 307, 357), already in place at the S4 census ("the script packages `/bin/cellbox`"), attributed to S1 `[INFERENCE]`; S2c `/bin/shutdown` (signable list 275, row 434) |
| `scripts/make-hypervisor-fs.sh` | 10142 | `99e31279bdc1b79956a7d0ac68691f34706eeef8c07e3601cf3f47b84bb27c8a` | loop comment 111, loop list 112, explicit `cellbox` case 115-116 | Header 9, `-p app-cellbox` 86, signing list 94: not separable by git. The census (proposal §1) named only 112-115 as stale, which suggests 86/94 were already renamed by S1 `[INFERENCE]`; header 9 is unattributed |
| `cells/tests/phase04-terminal-driver/src/main.rs` (untracked) | 53963 | `00324c7114ad378fcc25a9473e6cea20dd56a9fbaf12c53baeed778dc04682da` | `sandbox_rights`: comment 891-892, `echo stsbcat=$?` 896, status condition 904, `cst` in detail 907 | **S2-T/W** `sandbox_rights` (rest of 882-926); **C5** `headless-no-ui-tasks` 400-407; S2-N, S2-K, OOM, S3 driver |

`Cargo.lock` (`e58e780a…18f6`, mtime 17:18) predates S4a and is not changed by it.

### Context: not S4a hunks, listed for completeness

- **`kernel/src/loader/launch_profile/profiles.rs`** (`3140c61b0765100bdf14eea8b8455e294c069abbffd71aa86885367377f0b0ca`, 13851 B, mtime 18:16): an implementer comment edit was made and reverted (Results). The current bytes equal the runner pin and the S2-T/W-approved hash. No S4a content.
- **`scripts/qemu-phase04-terminal-test.sh`** (`dfaf1baa1ed7e24a1be284df7e31306a96c0bd72c7394ce271df025aa05abeb0`, 43558 B, mtime 19:57): not changed by S4a. It changed after the S4b dossiers hashed it (`d4cdaada…`), through C5.

### Relationship to earlier approved hashes

No earlier approval extends to the current bytes of these files.

| File | Approved in | Approved hash | Current hash |
|---|---|---|---|
| `cells/tools/shell/Cargo.toml` → `cells/tools/cellbox/Cargo.toml` (S1 path rename) | Phase02 CP2 | `9e07bddd…` | `610860cd…` |
| `cells/tools/shell/src/main.rs` → `cells/tools/cellbox/src/main.rs` (S1 path rename) | Phase02 CP2 | `7e0be7fb…` | `0396292e…` |
| `commands.rs` | S2-K CP2 (S2-K hunks only) | `2e2fe670…` | `2e2fe670…` (same bytes; S4a hunks not covered) |
| driver | S3-output CP2; OOM/S2-K/S2-N/S2-T/W CP2s (own hunks only) | `ec167edc…`; `0139d5cc…` | `00324c71…` (changed by C5 after those approvals) |

`ledger.rs`, `state_transfer.rs`, the catalog, both recipes and the four docs have no prior CP2 hash.

## Caller census (read-only grep, 2026-10-22)

- **`applets` / `dispatch_applet` / `cellbox <applet>`:** no source, Cargo, CI (`.github/workflows/ci.yml`) or script reference remains. All runners build `-p app-cellbox` with default features (Phase04 runner 245-248, Phase03 106, Phase02 90). Remaining mentions are historical `.agents/` records (S3-gui CP2 "`--features applets` 88 pass", S2-K dossier, `phase-09-ssh-client.md:50`).
- **`commands::cmd_cat`:** one caller, `executor.rs:1735` (`"cat"` builtin via `with_legacy_parts`). **Not routed through it, so unchanged by S4a:**
  - command substitution `$(cat F)` / `$(vcat F)` (`executor.rs:721-731`);
  - the `cat F | …` byte head and `Head::Cat` (`executor.rs:1266, 1303`, `native_rows::read_file_bytes_exact`);
  - `vcat` (`cmd_fs::cmd_vcat`, `executor.rs:1749`);
  - the stock `/bin/cat` ELF (ledger `RetainedWorker`).
- **Consumers of `cat`'s status:** driver `sandbox-no-file` (non-zero), `secret-denied-0` (non-zero), `control-file-created` (zero). The integration tests in `tests/integration/tests/boot.rs` that check a missing-file status (`911`, `1425-1502`) use `vcat`, not `cat`.
- **`/bin/shell` / `app-shell` in live recipes and code:** no `app-shell`, `service-shell` or `LFN 'shell'` remains in `scripts/`, `gen_disk.ps1`, `.github/`, `kernel/src`, `cells/`. Remaining `/bin/shell` strings are:
  - negative guards in the Phase02/03/04 runners (`qemu-phase02-session-test.sh:140`, `qemu-phase03-pipeline-test.sh:156, 532, 727`, `qemu-phase04-*-test.sh`), which assert it is **not** launched or is refused;
  - an arbitrary readable path in `cells/services/vfs/src/access/tests.rs:14`;
  - the `main.rs:22` comment ("retired `/bin/shell` ceiling");
  - `cells/tools/sys-tools/src/bin/env.rs:21`, which prints `SHELL=/bin/shell` (see limitations);
  - dated history in `docs/baremetal/load-cellos.md` (243, 656), `docs/project-changelog.md` and `docs/evidence/*`.
- **`gen-disk-ci.sh` / `make-hypervisor-fs.sh` callers:** CI `ci.yml:1654-1655, 2088` (gen-disk) and `ci.yml:721-722, 826-827`, `scripts/build-image.sh:204`, `scripts/qemu-tier3-hostile-runner-arm64.sh:51`, `run-hypervisor-arm.ps1` (make-hypervisor-fs). None of these was run for S4a.

## Known limitations / not exercised

1. **No raw log** for the host suite (89/89), the `x86_64-unknown-none` and `shell_test` checks, `bash -n`, or the scratch-image FAT assertion test. They are recorded only in the S4 Results.
2. **Recipes not run end to end.** No canonical RV64 image (`gen-disk-ci.sh`) and no AArch64 hypervisor image (`make-hypervisor-fs.sh`) was produced: the containment signer refuses canonical cellbox signing.
3. **`cat` coverage.** The images exercise a single missing operand, a single denied operand and a single good operand. Multiple operands (continue after a failure, first failure reported) are not exercised by any saved check. The exact status value is never logged; only the `≠ 0` / `= 0` conditions pass.
4. **`cat` read error midway** still exits 0 (Results; outside D-S4-2).
5. **Runtime does not observe the deletion.** `applets` was never in any runtime image (non-default feature), so the image runs prove only that the default build is unaffected.
6. **Residual wording outside S4a's scope**, found by this census and not changed:
   - catalog §1 line 15 and `docs/cellbox-cli.md:36` still describe `/bin/cellbox <command>` explicit invocation as canonical (target design text; the only implemented entry is `cellbox sh`);
   - `phase-09-ssh-client.md:50` refers to the deleted `applets/ssh.rs`;
   - stock `/bin/env` prints `SHELL=/bin/shell` (`sys-tools/src/bin/env.rs:21`).
7. Not run: GUI lane for S4a specifically (the c5 GUI logs belong to the C5 dossier), KVM, the kernel AP lane, riscv64/aarch64 runtime.

## Deviations (Results, cross-checked against source)

1. **`make-hypervisor-fs.sh` explicit case.** The proposal said "replace with `cellbox`/`app-cellbox`". The package `app-cellbox` builds the binary `cellbox` (`[[bin]]`), so neither `app-cellbox` nor `service-cellbox` exists in `$BIN_DIR`; an explicit `$BIN_DIR/cellbox` case (115-116) was added.
2. **`cat` fix is wider than "missing file returns 0".** It also continues past failed operands and makes the S2-K `Permission denied` branch record the failure and continue (394-395) instead of stopping. Before S4a that branch already produced a non-zero status, because `secret-denied-0` required one when S2-K landed `[INFERENCE]`; no pre-S4a copy of `commands.rs` exists to show its exact form.
3. **`profiles.rs` comment edit made and reverted.** No net change; the pin `3140c61b…` holds (Results; current hash).
4. **Ledger test change not separable.** `ledger.rs` is untracked and was changed by S2-lite, S2c and S4a; the exact S4a test edit cannot be shown by git.

## What CP2 would authorize

Approving CP2 would make the S4a deletions and the S4a hunks identified above, at the listed whole-file hashes, the S4a baseline. Later work could rely on: no `applets`/`dispatch` source or feature in `app-cellbox`; `gen-disk-ci.sh` requiring `/bin/cellbox` in `kernel_fs.img`; `make-hypervisor-fs.sh` packaging `cellbox`; builtin `cat` exiting non-zero on a missing or denied operand.

CP2 would **not**:
- approve the S1, Phase02/03, S2-lite, S2c, S2-K, S2-N, S2-T/W, OOM, S3 or C5 hunks in the shared files;
- approve the residual wording in limitation 6;
- lift quarantine, or authorize canonical packaging, release signing or rollout;
- close criterion 4 (canonical packaging stays blocked by containment).

## Options

A. **Approve this CP2 baseline** at the exact hashes and evidence scope above.

C. **Not yet**: retain the implementation and evidence, withhold CP2.

No approval has been recorded.

## Rollout and rollback

**Rollout stays BLOCKED** under the [containment](reports/rollout-containment.md) release conditions; nothing here changes them.

**Rollback steps:**
1. Revert the S4a hunks in `state_transfer.rs`, the four docs, catalog §3.1, `gen-disk-ci.sh:453` and `make-hypervisor-fs.sh:111-116`.
2. Revert the `cmd_cat` `failure` tracking and the driver's status condition **together**. Reverting only `cat` makes `sandbox-no-file` fail (S2-T/W annex §11 records the first S2-T/W image run failing that check because of this `cat` defect); reverting only the driver hides the status again.
3. `applets`/`dispatch.rs` cannot be restored from git (they were untracked). Restoring them would need a copy from outside the repository, and a future multicall would need its own ceiling delta (proposal D-S4-1).

**Constraints on rollback:**
- `commands.rs` interleaves S4a with the approved S2-K gate (`394` sits inside it); revert by hunk, keeping the gate.
- Reverting `gen-disk-ci.sh:453` alone restores a guard that fails on every current image, since the script packages `/bin/cellbox`.

No persisted state is involved.
