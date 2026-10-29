# Phase 04 S1 / S2-lite / S2c-min / S3 launch edge — CP2 mini-dossier (hash-only)

**Status: CP2 APPROVED by the user on 2026-10-25** for S1, S2-lite, S2c-min and the S3 launch edge, including the seven deviations in §6, at the whole-file hashes in §4. The approved bytes of this dossier, before this status amendment, have SHA-256 `8e84ead72f284d5f4b902c090888dd47774f20db3c8056902dfd90c47b5dcd1f`. On the same day the user also confirmed the catalog as frozen at `command-catalog.md` SHA-256 `f2a82fce61101f9f1dd6d62a75bece735e3eb9129a2295b187c2f49209485339`. The user approved the cleanup of the `profiles.rs:148-150` comment (§6 item 4) and the `git rm` of the staged `snapshot_client.rs` (§6 item 3) as separate actions; this record does not perform them. Rollout, canonical packaging (Phase 04 criterion 4) and Phase 10 remain blocked under [rollout containment](reports/rollout-containment.md).

Prepared 2026-10-25 for [Phase 01 closure dossier](phase-01-closure-dossier.md) gap C2-a, at the user's request (decision of 2026-10-25: build a hash-only CP2 mini-dossier for approval). This document changes no source. It ran no build and no QEMU. Its only inputs are `sha256sum` and reads of source and saved evidence.

## 1. Why this request exists

These four Phase 04 slices each have a user CP1, but none has a CP2 record:

- the [S4a CP2](phase-04-s4a-cp2-dossier.md):153 explicitly excludes "S1, … S2-lite, S2c … S3 … hunks";
- the [S3-output CP2](phase-04-s3-output-cp2-dossier.md):41 hashes `profiles.rs` whole, but its approval covers S3-output only.

[plan.md](plan.md) row 04 and [phase-04](phase-04-multicall-cutover.md):96 said "every Phase 04 delta CP2-approved". Both were corrected on 2026-10-25 to point here. S1 and S2c-min are authority deltas: the boot identity, the kernel raw-502 gate and the supervisor snapshot caller names.

## 2. Authority and owner

| Slice | CP1 record | Ratified deviations |
|---|---|---|
| S1 serial shell cutover / boot identity | [entry dossier](phase-04-entry-dossier.md):3 "Duyệt S1" (2026-10-21), scope §3, gates §4 | S1 results items 1–5, ratified 2026-10-21 ("1", `:44`). The `kernel_fs.img` anomaly is recorded but not ratified as safe |
| S2-lite ledger + denial proof | [S2 authority proposal](phase-04-s2-authority-proposal.md):3 "Duyệt S2-lite" (2026-10-21), closure §4 | none recorded |
| S2c-min lifecycle (Option B) | [S2c proposal](phase-04-s2c-lifecycle-proposal.md):3 "Duyệt Option B" (2026-10-21), closure §4 | none recorded; deviations are listed in §5 below |
| S3 launch edge (`terminal → /bin/cellbox`) | [S3 proposal](phase-04-s3-gui-terminal-proposal.md):3 "Duyệt S3 (userspace)" (2026-10-21) | the TerminalHost part was superseded by S5-3c, which is CP2-approved in the [P05 dossier](phase-05-cp2-dossier.md) §16 |

**Owner.** The user confirmed on 2026-10-25, retroactively, that the assistant was the integrating owner for the Phase 03–05, 08 and 09 file sets under the recorded CP1s. That confirmation covers these Phase 04 slices. The user remains the approver.

## 3. Scope as built (current tree)

**S1 boot identity.** Every exact `/bin/shell` row became `/bin/cellbox` with an identical ceiling:

- `kernel/src/loader/boot_ceiling.rs:119` (`"/bin/cellbox" => LEGACY`);
- `kernel/src/loader.rs:71`;
- `kernel/src/policy.rs:828,1198`;
- `kernel/src/loader/early.rs:40`;
- `kernel/src/loader/elf_tests.rs:212`;
- the init launch row at `kernel/src/loader/launch_profile/profiles.rs:27`;
- the boot-ceiling self-test at `kernel/src/loader/boot_ceiling/selftest.rs:53-58,172,204`;
- init autoload at `config/autoload.toml:23-25` (`/bin/cellbox`, `args = ["sh"]`).

The cellbox ELF's syscall list is the retired shell's list. The `G-S1a` ceiling diff (`scripts/cellbox-ceiling-diff.py`) reports PASS against HEAD's `cells/tools/shell/src/main.rs`, with two noted additions: `Write` (pre-S1, allow-listed) and `GetRandom` (P09 D79-9).

**S2-lite ledger.** The ledger is `cells/tools/cellbox/src/ledger.rs`:

- `LEDGER` starts at `:64` and now has 77 entries: 60 builtins (the 56 of S2-lite plus `edit`/`ssh`/`completion`/`rg`, added under approved P07–P09 CP2s), 11 sys-tools and 6 net-tools;
- `DENIAL_CHECKS` is at `:149` and `REFUSAL_CHECKS` at `:156`;
- host tests compare the builtin entries with `BUILTINS` (`:230-234`) and require the catalog block to be generated from the ledger (`:317-338`).

The denial checks are in `scripts/qemu-phase03-pipeline-test.sh:869-907`: `s2-kill-builtin-refused-no-ipc` (`:873`), `s2-blktest-raw-block-denied` (`:881`), `s2-snapshot-builtin-refused-no-ipc` (`:887`), `s2-unreviewed-path-spawn-refused` (`:892`), the forged-argv exec refusals (`:894-895`), `s2-no-effect-process-set-unchanged` (`:897`) and `s2-builtin-shutdown-denied-shell-survives` (`:904`). The final probe `s2-final-shutdown-worker-launches-and-halts-vm` is at `:940`.

**S2c-min lifecycle.**

- **Kernel gate (K1).** `RAW_SHUTDOWN_CALLERS = &["shutdown"]` at `kernel/src/task/syscall.rs:1858`, with `raw_shutdown_permitted` at `:1860-1862`. The gate `authorize_raw_shutdown` (`:1864-1873`) logs `raw opcode 502 denied for <name>` and returns `PermissionDenied`. The AP-PZ test hook starts at `:1875`, behind `test-hooks`, and is registered at `kernel/src/loader/atomic_publication_tests/denials.rs:132`.
- **Launch row (K2).** `cellbox → /bin/shutdown`, Path only, at `profiles.rs:66-69`. The ceiling is now `LEGACY`; see §6 item 4.
- **Shell refusals.**
  - builtin `shutdown` calls `sys_shutdown_denied()` and prints `shutdown: use /bin/shutdown` (`cells/tools/cellbox/src/cmd_sys.rs:112-113`; `libs/ostd/src/syscall.rs:229-230`);
  - `kill` and `snapshot` refuse (`cells/tools/cellbox/src/commands.rs:616,623`).
- **Supervisor.** `SNAPSHOT_CALLER_NAMES: &[&str] = &[]` at `cells/services/supervisor/src/main.rs:40`, checked at `:86`.

**S3 launch edge only.**

- `terminal_profile`: Path route to `/bin/cellbox` only, `LEGACY` ceiling (`profiles.rs:148-156`).
- Host tests: `tests.rs:546-558` (Elf/Mem/Pinned denied).
- The private fixture edge `phase04-terminal-driver → /bin/cellbox` exists only under kernel feature `phase04-driver-profile` (`profiles.rs:158-175`; `tests.rs:589-630`).

TerminalHost and the GUI Terminal code are **out of scope**. S3-output, S3-gui and P05 S5-3c already approved them.

## 4. Current-disk inventory (SHA-256, recorded 2026-10-25)

The table hashes each file whole, so a hash covers every delta's hunks in that file. For shared files, an approval would cover only the S1/S2-lite/S2c-min/S3-edge hunks named in §3. The other hunks keep their own CP2s:

- `profiles.rs`/`targets.rs`: S2-N, S2-T/W, P09 S9-2;
- `syscall.rs`: P02, P06, P09 and others;
- `main.rs`: P09 `GetRandom`/ssh entry.

| Path | Bytes | SHA-256 |
|---|---:|---|
| `kernel/src/loader/launch_profile/profiles.rs` | 14356 | `ded1ea57b6e042f150ff9efc11e42908ceb2edd110e4710e0d8b9ef87ca608bd` |
| `kernel/src/loader/launch_profile/tests.rs` | 35040 | `25b47b3f77e8ef230daa59730a775f989ad5a71887c22ee45e44363142c7922a` |
| `kernel/src/loader/launch_profile/targets.rs` | 5480 | `45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f` |
| `kernel/src/loader/launch_profile/mod.rs` | 4459 | `b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c` |
| `kernel/src/loader/boot_ceiling.rs` | 13369 | `cebbb73af1bb1525f73de3510173b0f846a4ba27a3eae63fa4a70d2bbdae22d0` |
| `kernel/src/loader/boot_ceiling/selftest.rs` | 11524 | `b2d05caecd1d4b0fd3e85bd4fa8479fe94a935de4d54c5616ea21932a0b281e8` |
| `kernel/src/loader.rs` | 5730 | `436f892d0fc545c0c7bb8e70fb3e285f5931e43f8156153855057836205c6b2d` |
| `kernel/src/policy.rs` | 50520 | `b34b9af9f8f88f0cddd2e3c532b59e4ac7036d1b3b0f7869e3a576fed78e0196` |
| `kernel/src/loader/early.rs` | 10704 | `a9eb5d476cbedd03341553cc7ae9d1024c062559a73dc0035d01eee5fab52075` |
| `kernel/src/loader/elf_tests.rs` | 37045 | `3868a16e8d6d1e9d78fd4859b18ddf3cd2e055021437a252b6bcc7ff55fac823` |
| `kernel/src/task/syscall.rs` | 421770 | `c56d500f72ac1adcf68a3cd422cf7c77702ba01449c1992391f757714f170af4` |
| `kernel/src/loader/atomic_publication_tests/denials.rs` | 61388 | `9008724f3555009a76257bad47f9fb559d03644f599b22124752afaa015f7fab` |
| `cells/services/supervisor/src/main.rs` | 6362 | `d7e30e84bafceb3dd3d981ab47f1b543f2e7e80220681cefa16d33658981e0c3` |
| `libs/ostd/src/syscall.rs` | 103035 | `95e4beb7e60561a061a6aed841bf2fc981a7a99e0c54716fcd3db4df7708c944` |
| `cells/tools/cellbox/src/ledger.rs` | 13468 | `e0d6f0842652484383b1cb0f0aabca0b71016e83a9b8ac4819a75f02f27820e5` |
| `cells/tools/cellbox/src/cmd_sys.rs` | 11107 | `1a9a3180eff37113a85363ca1b36f41d3b43d26643e59621546f0a7e37ce6bd4` |
| `cells/tools/cellbox/src/commands.rs` | 26891 | `fc0f506aa20f640076ed5d22a6883cb58fed311c6c935f2a5e08fd638be61a0e` |
| `cells/tools/cellbox/src/main.rs` | 8576 | `f1590c9cfcd1edb9f2b2f109511414c776d0a9e98dae9d3c209feb48af371716` |
| `cells/tools/cellbox/Cargo.toml` | 2076 | `b1481462e4145d986d9dd444c3b6ac42adcaf3c7eb45cfaf92d8ab36448f43bd` |
| `config/autoload.toml` | 449 | `85661e78ac6020cb60dec9bf32c6db503ba3c2fca5265c3913276b7e8885985f` |
| `scripts/qemu-phase03-pipeline-test.sh` | 55670 | `61b5d9285a33c580d6c6139ac014e66be326845bf9ecf96869f4847d0b6320e5` |
| `scripts/cellbox-ceiling-diff.py` | 5728 | `34f2a939f29d61208ceebddc34b3da40310960a4c6fa1b18bdda78a0ec331c71` |

`cells/tools/cellbox/Cargo.toml` already includes the user-approved `version` fields on the path dependencies (lines 29, 34, 36, 38), added for the cargo-deny bans fix (closure dossier C3-a). Those lines are not part of this request.

CP1 documents (the binding text), current SHA-256:

| Document | SHA-256 |
|---|---|
| [phase-04-entry-dossier.md](phase-04-entry-dossier.md) | `d2b2a72a5fbd2793443bcf6611afbe5aa642f296db4ca9cf7eb08ca7aec13bf9` |
| [phase-04-s2-authority-proposal.md](phase-04-s2-authority-proposal.md) | `c6ab2db84cc0b804b8d3241ea971d9a53d68bdf4e0d575c829a894bbd3aa3f9a` |
| [phase-04-s2c-lifecycle-proposal.md](phase-04-s2c-lifecycle-proposal.md) | `c952294f1ee8b9924099a8fc3a8858e33ae126e1d81db9767fe96d2084220002` |
| [phase-04-s3-gui-terminal-proposal.md](phase-04-s3-gui-terminal-proposal.md) | `212cde87ae21cc7434233fa895825f8c143bd8c25e49a04dfbb697039450acee` |

## 5. Evidence

**Slice-time evidence (2026-10-21, already recorded):**

- **S1:** G-S1a PASS; kernel AP lane 36/36; P02 48/48; P03 47/47; G-S1d launch-edge refusals observed ([entry dossier](phase-04-entry-dossier.md):34-42). Saved files:
  - `reports/phase-04-evidence/s1-pipeline-commands.json` `57e55fedcdb344c1eb6a24070fe9e15e930aecbc71d40988a8d1bad3cf6a0d51`;
  - `s1-provenance.json` `9b6ca6c297666c284f0905b7eb7efdee97874065ff861765ff30914940bbbb59`.
- **S2-lite:** host 74 (76 with `applets`), ceiling diff PASS, P03 59/59, P02 48/48 ([proposal](phase-04-s2-authority-proposal.md):31-35).
- **S2c-min:** P03 60/60, P02 48/48, ceiling diff PASS, host `app-cellbox` 70 (72 with `applets`) ([proposal](phase-04-s2c-lifecycle-proposal.md):33-37).
- **S3:** P04 runner 53 PASS, P02 48/48, P03 60/60 ([proposal](phase-04-s3-gui-terminal-proposal.md):31). Saved files:
  - `reports/phase-04-evidence/s3-driver-markers.txt` `1d04f83f19ca4459d0a642814cd80104e44beea25634b86add77baa1b66bdf42`;
  - `s3-provenance.json` `972f45602d2860f570b9c98d88c761691dfe26264b47b3cc0b629dccd8266c73`.

**Current evidence (2026-10-25, private TCG workdirs, not archived in `reports/`):**

| Lane | Workdir / file | Result | SHA-256 |
|---|---|---|---|
| P04 terminal (`scripts/qemu-phase04-terminal-test.sh`), the latest named by the user | `/tmp/cellbox-phase04-z1mR/evidence/verdict.json` (mtime 19:34:51) | **459/459** `pass: true`; includes `marker:shell-prompt`, `unknown-program-denied`, `no-unexpected-launch-deny`, `marker:relaunch-after-exit` | `0c6152ffc67256d39df9b3fb5d7a385cda0c1ea4c058e9fb09227bd359a548b9` |
| same | `…/z1mR/evidence/cellbox-ceiling.txt` | `G-S1a PASS: declare_manifest and declare_syscalls sets are identical`; notes `GetRandom` and `Write` as added by approved deltas (34 vs 32 syscalls) | `abbf0b63dc2da0f51770db79d477535144de3c70d8aaeb4c12dfd318bd9a80d9` |
| same | `…/z1mR/evidence/provenance.json` | private fixture; source head `3471028bc`; signed `cellbox` `24bcfcdc…3609`; kernel features include `phase04-driver-profile` | `f320cd82a175e66c59c31565ecd53c6c7217a6e10ab386a848277ff1d25837dd` |
| P04 terminal, later run | `/tmp/cellbox-phase04-9FGz/evidence/verdict.json` (mtime 21:18:50) | 459/459; signed `cellbox` `3b4f858c…0b89` | `35f39293ecfe005a6f8c675b37db5c574adb1039ff869b47ac8e1c19a7596072` |
| P04 GUI (terminal → `/bin/cellbox` edge) | `/tmp/cellbox-phase04-gui-uyuX/evidence/verdict.json` (19:53) | **290/290**, including `GUI-2-prompt-matches-exactly` and `no-launch-deny` | `26cfb6cfd64b0a8c73350219007d60677742b0812f62c4a7408e0458891038f8` |
| P03 pipeline (S2-lite/S2c-min checks) | `/tmp/cellbox-phase03-Qaov/evidence/serial.log` (19:52) | Serial lines matched by the four checks: `raw opcode 502 denied for cellbox` ×1, `raw block opcode 500 denied` ×1, `kill: not available from the suite shell` ×1, `snapshot: not available from the suite shell` ×1, `shutdown: use /bin/shutdown` ×1, `s2-shutdown-status=1` ×1, `ALLOWED` ×0 | `d7693840228d9dabcd2fb14a53d86e25d408221ab4192a12622c5ec2c33d9820` |
| same | `…/Qaov/evidence/provenance.json` | signed `cellbox` `83c204c6…edeb` | `2474894462ace6b7f0bede965e74f0ae6403ca7b6ad4c52ac3c6162ae97352dc` |

**Limit.** The P03 runner prints its check tally to stdout and writes no verdict file, so no saved pass count exists for `Qaov`. The serial lines above are what the checks match (`scripts/qemu-phase03-pipeline-test.sh:869-907`), not the runner's verdict. The latest recorded P03 count is 67 ([plan.md](plan.md) rows 07–08). The kernel AP-PZ lane and the ledger host tests were not run for this dossier.

## 6. Deviations and changes since CP1 (disclosed for CP2)

1. **S1 deviations 1–5** were ratified on 2026-10-21 ([entry dossier](phase-04-entry-dossier.md):44). Item 1 (applets off) was later superseded by the S4a deletion of `applets`. The `kernel_fs.img` anomaly stays recorded and is not ratified. The canonical `kernel_fs.img` remains under rollout containment.
2. **S2-lite ledger classes changed after CP1.** S2-lite added a fourth class, `InSuiteEffectUngated`; S2c-min then removed it and added `InSuiteRefused`. The entry count grew from 73 to 77 through the approved P07–P09 builtins. Catalog §3.1 is still byte-generated from the ledger.
3. **S2c-min deviations** (from [proposal](phase-04-s2c-lifecycle-proposal.md):35-36):
   - (a) `libs/ostd/src/syscall.rs` gained `sys_shutdown_denied()`, outside the §4 closure, with no ABI change;
   - (b) two P03 checks that asserted the opposite behavior were retired;
   - (c) on x86 the shutdown branch halts the CPU and does not exit the VM.
   Follow-ups listed there, status today:
   - `tests/integration/tests/boot.rs:476,659` now send `/bin/shutdown`;
   - `ahci-x86.rs` and `launch-profile.rs` have no `shutdown`/`snapshot` match;
   - `docs/specs/03-runtime.md:42` quotes the new snapshot refusal text;
   - `cells/tools/shell/src/snapshot_client.rs` is still staged as `RD` in `git status` and still needs `git rm`.
4. **Ceiling labels changed after CP1 under another approved delta.** S2c-min CP1 specified `cellbox → /bin/shutdown` with `CapSet::EMPTY`, and the terminal edge's doc comment still says "empty child ceiling" (`profiles.rs:148-150`). Both rows now read `LEGACY` = `CapSet::LEGACY_OBSERVE_MUTATE`. That constant holds "the pre-annex X/M/W rights every reviewed non-sandbox edge keeps" (`profiles.rs:12-14`), and S2-T/W made those rights explicit (annex §3, D-TW1, CP2 2026-10-22, [dossier](phase-04-s2tw-cp2-dossier.md)). The change belongs to S2-T/W, not to this request. The stale doc comment is noted here and was not edited.
5. **The cellbox syscall list grew after S1.** P09 S9-2 (CP2 2026-10-25) added `GetRandom`. `Write` predates S1 and is allow-listed. G-S1a still passes.
6. **S3 scope is reduced to the launch edge.** The original TerminalHost/headless results were superseded by S3-output, S3-gui and S5-3c, which have their own CP2s.
7. **Evidence is current, not slice-time, for the four S2 checks.** The 2026-10-21 runs left no saved verdict files. Today's evidence is the P03 serial markers plus the P04/GUI verdicts above.

## 7. What CP2 would authorize

Approving would make the S1, S2-lite, S2c-min and S3-launch-edge hunks named in §3, at the whole-file hashes in §4, the CP2 baseline for those four slices. Later work could then cite them as approved.

It would **not**:

- approve other deltas' hunks in the shared files;
- lift rollout containment or canonical packaging (Phase 04 criterion 4, including `/bin/cellbox-ssh` packaging, closure dossier C1-f);
- ratify the `kernel_fs.img` anomaly;
- create a generation-bound lifecycle owner (S2c-full: `KillTarget`/`ResetSystem`/`SnapshotTarget`/`ReplaceService` stay open, and `kill`/`snapshot` stay unavailable from the CLI);
- complete any CBX row.

## 8. Options

- **A.** Approve this CP2 at the exact hashes and private-fixture evidence scope above.
- **C.** Not yet: keep the evidence and withhold CP2. In that case [plan.md](plan.md) row 04 stays as corrected ("S1/S2-lite/S2c-min/S3-edge CP2 pending").

**Decision recorded (user, 2026-10-25): option A**, including deviations 1–7.

## 9. Rollback

Revert the hunks named in §3 as a matched set with the kernel, the cellbox ELF and the image:

- S2c-min: K1 together with the K2 row and the shell refusals;
- S1: the boot identity rows together with the init autoload and the image recipes.

Rollback needs a matched kernel/policy/image and a cold boot on a private fixture. No canonical image is touched. No persisted state exists beyond private fixture files.
