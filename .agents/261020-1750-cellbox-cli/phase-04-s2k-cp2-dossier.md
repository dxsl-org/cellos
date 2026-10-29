# S2-K CP2 dossier: implementation review request

**Status: CP2 APPROVED by the user on 2026-10-22 (“duyệt bốn CP2”) for dossier SHA-256 `4447cc50fe0614bb109078f2f114addba8f16bddbc9d25bc4e3d2ef8ea1b128f` (digest before this status amendment).** CP2 would make only the S2-K delta's source (secret-namespace denial and the fail-closed BootFS fallback) the baseline. Quarantine, canonical packaging, release signing and rollout stay blocked.

Prepared 2026-10-22 under S4b of the [S4 retirement proposal](phase-04-s4-retirement-proposal.md). Sources: saved evidence and current disk bytes. The only new commands were `sha256sum` and read-only `git`/`grep`. Nothing was built, tested or booted.

## Authority

- [S2 rights CP1](phase-04-s2-rights-proposal.md): **S2-K** approved by the user on 2026-10-22 (“S2-K”). At entry the user narrowed step 1 to “Bước 2 + fail-closed” (step 2 plus fail-closed): keep the raw kernel BootFS fallback and gate it, rather than remove it. S2-N, S2-T, S2-T-min and S2-W were not part of that approval and have separate dossiers.
- Previous CP2 baselines touching this closure: [S3-gui](phase-04-s3-gui-cp2-dossier.md) (`executor.rs`) and [S3-output](phase-04-s3-output-cp2-dossier.md) (Phase04 driver and runner). Their approvals cover their recorded hashes only.

## Implemented delta

**Bypass this fixes.** The raw kernel route `sys_open`/`sys_read` (VIFS1/BootFS) skips VFS caller/path policy. Before the fix, a private fixture at `/srv/cellos/p04-secret` was refused by VFS, yet the shell served it through the fallback. VFS reserves `/srv/cellos/**` to the live KMS.

**Fix.** `cmd_fs::bootfs_fallback_allowed(path)` (`cmd_fs.rs:34-41`) resolves the shell path and asks VFS to `stat` it. The raw route is used only when VFS permits the stat (`Ok`) or answers "no such entry" (`Err(ViError::IO)`). A VFS refusal, or no VFS answer at all, never falls through. The gate is applied before every raw `sys_open` of file bytes in cellbox:
- `cmd_fs.rs:45-47` in `read_file_bytes`;
- `cmd_fs.rs:847-849` in `read_path_bytes_exact`;
- `commands.rs:216` in the `ls` BootFS branch;
- `commands.rs:389-396` in the `cat` fallback;
- `executor.rs:2381-2383` in `read_path_bytes`;
- `text_tools/grep/io/read.rs:30-32`.

**Status fixes.** Refusals now report status 1 where they used to report 0:
- `cp` when any copy fails (`cmd_fs.rs:653-695`);
- `ls` cannot access (`commands.rs:321-326`);
- the `echo >`/`>>` fast path, which previously left `$?` unchanged (`executor.rs:1544-1589`).

The Results list `applets/sys.rs` (non-default feature) among the gated readers. **That file no longer exists**: S4a deleted `cells/tools/cellbox/src/{dispatch.rs,applets/}`, which were untracked, and the `applets` feature. No bytes remain to hash or approve.

No kernel, ABI, service, launch-profile or manifest change. `Open`/`Read` stay in the cellbox ceiling because the gated fallback still needs them. Runner line 6: `G-S1a PASS: declare_manifest and declare_syscalls sets are identical`; no addition and no reduction.

## File inventory: current disk SHA-256

| Path | Bytes | SHA-256 | S2-K regions (current lines) | Shared with |
|---|---:|---|---|---|
| `cells/tools/cellbox/src/cmd_fs.rs` | 45401 | `47f01e343c29e75b2ba4aec91ce5bfad0471db4f9407c0d671c2767e950d1981` | module doc 4–5; `bootfs_fallback_allowed` 34–41; gate 45–47; `cmd_cp` status 653–695; gate 847–849 | earlier Phase02–04 slices (stderr routing, S2-lite); **no prior CP2 hash** |
| `cells/tools/cellbox/src/commands.rs` | 24162 | `2e2fe670247b31407e054c24d2979243d7a52b9501f3ffd3d4a115d14f1048fd` | `ls` gate 216; `ls` status 321–326; `cat` gate 389–396 | **S4a D-S4-2** (`cat` continues past missing/denied operands and exits 1; `failure` tracking around 394); **no prior CP2 hash** |
| `cells/tools/cellbox/src/executor.rs` | 142801 | `2ef99fa14d0ae9dc5f581925dc6a60cbfcd71c5bd0c445cda24bd841a5430531` | `echo >`/`>>` status 1544–1589; gate 2381–2383 | **OOM** (`LocalOutcome`/`TerminalSink`/streaming dispatch, 1125–1475); S3-gui approved base |
| `cells/tools/cellbox/src/text_tools/grep/io/read.rs` | 1793 | `5bdded833c31ffa42187ab196786b22fa592df492ceabb859ea3da9e3d9d5874` | gate 30–32 (exact diff below) | none |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 53581 | `0139d5cc3404d092d848ba756d73c680bbeeaa0eeea59c912625cf7166d8fe61` | `SECRET_TOKEN` + `secret_namespace` 430–478; call at 328 | **OOM** (`endless_local_sink`), S2-N, S2-T/W, S4a; S3-output approved base |
| `scripts/qemu-phase04-terminal-test.sh` | 42941 | `d4cdaadaffe48949d3122ca9ab81863b3732b8bce7f7de4d2dab382a0ee884a6` | fixture 316–321, packed by `tools/mkfat32.py` into the private `kernel_fs.img` (389–390); markers 549–550; UART token check 665 | **OOM** (547–548, 658–664), S2-N, S2-T/W (digest pins); S3-output approved base |
| ~~`cells/tools/cellbox/src/applets/sys.rs`~~ | — | deleted by S4a | gate existed per Results | S4a removed it |

Consumer only, not changed by S2-K: `native_rows.rs` (`605ff51d…518d`) and `text_tools/grep/io.rs` (`ce916c51…b02`, unchanged from its pre-cellbox hash in `service-x86-provenance.json`).

### Exact diff where it can be shown

- **`text_tools/grep/io/read.rs`: exact.** Against `HEAD:cells/tools/shell/src/text_tools/grep/io/read.rs` (blob `f966dd84e`, now `2435a7069`), the rename has similarity 94% and the only content change is:
  ```diff
  @@ -27,6 +27,9 @@ pub(super) fn read_path_bytes(path: &str) -> Result<Vec<u8>, ReadError> {
           return Err(ReadError::Io);
       }

  +    if !crate::cmd_fs::bootfs_fallback_allowed(path) {
  +        return Err(ReadError::Io);
  +    }
       let fd = syscall::sys_open(path).map_err(|_| ReadError::Io)?;
  ```
- **`cmd_fs.rs` and `commands.rs`: not separable.** `HEAD` (`cells/tools/shell/src/…`) contains no `bootfs_fallback_allowed`. Their index-to-worktree diffs (233 and 276 lines) mix S2-K with earlier Phase04 slices and, for `commands.rs`, S4a. No pre-S2-K copy exists, and neither file has a prior CP2 hash.
- **`executor.rs`, driver, runner: not separable.** Their bytes changed since their S3-gui / S3-output hashes because of OOM, S2-K, S2-N, S2-T/W and S4a. See the [OOM dossier](phase-04-oom-cp2-dossier.md) for the executor's `e123054d…88c1` → current history.

## Caller census

Gated reader → who reaches it:

| Gated site | Callers |
|---|---|
| `cmd_fs::read_file_bytes` (45) | `wc` 103, `head` 147, `tail` 183, `uniq` 1010, `sort` 1045, `awk` (legacy) 1177; `copy_single_file` 509 ← `cmd_cp` |
| `cmd_fs::read_path_bytes_exact` (847) | `native_rows::read_file_bytes_exact` 50 ← `executor.rs` `ByteHead::Cat` 1266, `Head::Cat`/`Head::FromFile` 1303–1304 (`cat F \| …`, `from <fmt> < F`); `shell_test.rs:752` |
| `commands::ls_collect` BootFS branch (216) | `cmd_ls` 321; `native_rows` `ls --records` 179 |
| `commands::cmd_cat` fallback (390) | `cat` built-in |
| `executor::read_path_bytes` (2381) | `read_utility_text` 2312 ← `sed` 2243, `awk` 2286 |
| `grep/io/read.rs::read_path_bytes` (30) | `io::read_text_file` 68 ← grep file operands (`io.rs:55,92,100`) and pattern file (`grep.rs:19`) |

`bootfs_fallback_allowed` is `pub(crate)`. Its six call sites above are its only users; a repository `grep` over `cells/` found none outside cellbox.

Raw BootFS entry points that remain **ungated**, by design or recorded:
- `ls /` merges root-level BootFS **names and sizes** (`commands.rs:186-202`); `/` is read-all under VFS policy.
- The `exec` KERNEL-FS ELF load (Results: `commands.rs:47-60`).
- The `sys_open_cap` app-manifest existence probes (`executor.rs:2142-2167`).

None of the three returns file bytes to a stream.

## Evidence

Final logs are `reports/s4-final-evidence/` (SHA-256 below). That run is the post-S4a tree, so it carries every later delta. The logs hold no source hashes. That this evidence matches the hashes above is `[INFERENCE]` from two facts: `Cargo.lock` equals the run's `cargo_lock_sha256` `e58e780a…18f6` (terminal log 486), and every source file's mtime precedes the archived logs. The fixture token is `p04-kms-namespace-fixture-token` (runner 318, driver 433).

Each `secret-denied-N` check requires all of: the command ran, the following `echo sk=$?` printed a non-`sk=0` value, and the token appeared on neither stdout nor stderr (driver 451–461).

| Command (driver index) | Final log lines |
|---|---|
| 0 `cat /srv/cellos/p04-secret` | terminal 568; serial 1514 `cat: /srv/cellos/p04-secret: Permission denied`, 1515 |
| 1 `grep p04 /srv/cellos/p04-secret` | terminal 569; serial 1516 (no stderr diagnostic logged) |
| 2 `head -n 1 …` | terminal 570; serial 1517 `head: cannot open '/srv/cellos/p04-secret'`, 1518 |
| 3 `wc …` | terminal 571; serial 1519–1520 |
| 4 `sort …` | terminal 572; serial 1521–1522 |
| 5 `from jsonl < … \| to jsonl` | terminal 573; serial 1523 `cat: /srv/cellos/p04-secret: Permission denied`, 1524 |
| 6 `cp … /tmp/p04-leak` | terminal 574; serial 1525 `cp: cannot copy …`, 1526 |
| 7 `ls /srv/cellos` | terminal 575; serial 1527 `ls: cannot access '/srv/cellos': No such file or directory`, 1528 |
| 8 `echo p04-w > /srv/cellos/p04-w` | terminal 576; serial 1529 `echo: cannot write '/srv/cellos/p04-w'`, 1530 |
| refused copy left nothing (`cat /tmp/p04-leak`) | terminal 577 `PASS marker:secret-no-copy`; serial 1531–1532 |
| token on no shell stream across all of the above | terminal 578 `PASS marker:secret-never-shown`; serial 1533 |
| token never in raw UART bytes | terminal 858 `PASS secret-token-not-on-uart` |
| positive control: readable fixture still reads through the fallback (`wc /p04-list/alpha-entry`, `sw=0`) | terminal 579 `PASS marker:ramdisk-read-still-works`; serial 1534 |
| whole Phase04 terminal run | terminal 878 `PASS: 353 Phase04 S3-output runtime checks (87 driver markers)`; serial 1733 `P04-TERMINAL: PASS checks=87`; 353 `PASS ` lines, 0 `FAIL` |
| ceiling unchanged | terminal 6 `G-S1a PASS` (cellbox); 7–11 are the informational Terminal `SetTimer` delta approved earlier |
| Phase03 regression | phase03.log 397–456: 60 `PASS ` lines, 0 `FAIL` |
| Phase02 regression | phase02.log 407 `passes: 48` |

**Bypass reproduced before the fix:** [s2k-baseline-before-fix.log](reports/s2k-baseline-before-fix.log), line 741.
- `cat`, `head` and `sort` reported `st=Ok(["sk=0"]) shown=true`.
- `wc` reported `sk=0`.
- `cp` reported `sk=0`, and `cat /tmp/p04-leak` returned the token (`secret-no-copy` failure).
- `ls` and `echo >` reported `sk=0`.
- Line 735 `FAIL secret-token-not-on-uart`; line 748 lists 13 failed checks, all S2-K related.
- Only `from jsonl <` (old index 4, line 468) already passed.

The logs contain no process exit codes. "Exit 0" for the Phase04, Phase03 and Phase02 runners comes from the S2-K and [S4a](phase-04-s4-retirement-proposal.md) Results. **Host suite:** the Results report `app-cellbox` 89/89, the same count as the OOM Results. `[INFERENCE]`: no host test was added for the gate. No raw host log is archived.

| Evidence file | SHA-256 |
|---|---|
| `reports/s4-final-evidence/phase04-terminal.log` | `ad492a2f74e63e4a8c059e2bfba7d16bc3d7ce44444d89bca7ca3c051a5dbd43` |
| `reports/s4-final-evidence/phase04-serial.log` | `7153285123116ffebe2f919fa074d7c5f1b560830ffe21a99f13e8c962406f93` |
| `reports/s4-final-evidence/phase03.log` | `c46d7e3f79d1c203df26fe4279725818ad8879442609f71b853eb2659e500ec7` |
| `reports/s4-final-evidence/phase02.log` | `592a99e3d6236f7cbfb7f3546424a2c6f5381326f4445a9041b6f5486cfc69f2` |
| `reports/s2k-baseline-before-fix.log` | `ec61771422174f68e18cbd723c521e970439a7a592e9a6766d7e0fc27ba88ec3` |

## Known limitations and what was not exercised

1. **No KMS IPC request** was sent from the shell; no in-suite command issues one. The G-K2 KMS-`auth` refusal item is therefore unproven.
2. **`/etc/cellos/cluster.key` RAMFS fixture not exercised.** Under the current VFS `/` read-all rule that file would be readable by any caller *through VFS*. That is a VFS policy finding outside this closure; it is recorded, not fixed.
3. **Gated in source but not image-exercised against the fixture:**
   - `tail`, `uniq`, legacy `awk`, `sed`/`awk` with file operands (`executor::read_path_bytes`);
   - `cat F | …` byte heads;
   - `ls --records`;
   - `echo >>`;
   - grep `-f` pattern files.
4. **Evidence covers one refused namespace.** The gate treats VFS `Err(IO)` as "absent" and falls through. Only `/srv/cellos/p04-secret` is evidenced as answering with a refusal rather than `IO`. `[INFERENCE]`: a protected namespace for which VFS returned `IO` would not be closed by this gate; no other namespace was probed.
5. **Relative operands not exercised.** The gate checks `resolve_shell_path(path)`. In `read_file_bytes`, `executor::read_path_bytes` and grep's `read_path_bytes`, the raw `sys_open` receives the operand as typed. Every image check used absolute paths.
6. **Diagnostics do not separate refusal from absence:**
   - `ls` prints `No such file or directory` for the refused `/srv/cellos` (serial 1527; `commands.rs:323-325`);
   - `from jsonl <` reports under a `cat:` prefix (serial 1523);
   - grep's refusal logged no stderr line (serial 1516); only nonzero status and absence of the token were observed;
   - `head`/`wc`/`sort`/`cp`/`echo` print generic "cannot open/copy/write".
7. **Remaining raw entry points** (`ls /` names, `exec` ELF load, `sys_open_cap` probes) are ungated as listed in the caller census.
8. **Scope and runners:**
   - Criterion 3: K is proven for the shell's read and copy paths only; N, T and W are separate.
   - TCG private image only.
   - GUI 6×103 runner not rerun after these `executor.rs` changes.

## Deviations from the CP1

- **Step 1 re-scoped by the user.** The fallback was kept and gated, not removed. `Open`/`Read` were therefore not dropped, and the ceiling records no reduction (G-K3 expected "no addition"; met).
- **Status semantics changed beyond read gating:** `cp`, `ls` and the `echo >`/`>>` fast path now return 1 on failure. This is disclosed in the Results.
- **G-K1:** the behavioral proof of the read path comes from the image. No dedicated host test is evidenced (see the host-suite note).
- **G-K4:** partially met. The criterion-3 progress note exists in [phase-04-multicall-cutover.md](phase-04-multicall-cutover.md) (line 72). `ledger.rs` has no K/KMS/BootFS entry, and the catalog was not updated for the gate. The CP1 closure named `ledger.rs`, but the Results do not claim a ledger change.
- **`applets/sys.rs`:** gated per Results, then deleted by S4a. No file remains in the closure.
- **Driver list changed between runs.** The before-fix driver ran 8 commands with no `grep`, so marker 8 was "missing" (before-fix log 472). The final driver runs 9, with `grep` at index 1, which shifts the later indices. grep's pre-fix behavior was not reproduced.
- **Fixture placement.** The driver comment says "boot ramdisk (VIFS1)". The runner adds the file to the private `kernel_fs.img` through `FAT_ARGS`/`tools/mkfat32.py` (316–321, 389–390). It exists only on the private image.

## What CP2 would authorize

Accept as the secret-namespace / fail-closed BootFS baseline:
- `bootfs_fallback_allowed` and its six call sites;
- the `cp`/`ls`/`echo` status changes;
- the grep `read.rs` gate;
- the driver's `secret_namespace` and the runner's fixture and checks.

The current whole-file hashes above are recorded as the reviewed bytes. Shared files (`executor.rs`, `commands.rs`, driver, runner) become a whole-file CP2 baseline only when the CP2s of the other S4b deltas in those files are also approved: OOM, and S2-N/S2-T/W for the driver and runner. `commands.rs` and the driver also carry S4a changes (`cat` exit 1; `sandbox-no-file`). S4b has no dossier for S4a; S4a source rests on the S4 CP1 approval, and the user must decide whether that is enough for a whole-file baseline. Approving this dossier alone authorizes the S2-K regions, not those deltas' bytes. This CP2 does **not**:
- lift quarantine;
- authorize canonical packaging, release signing or rollout;
- accept the VFS `/` read-all finding or the unexercised KMS IPC item;
- close criterion 3 (N/T/W).

## Options

A. **Approve this CP2** for the S2-K regions at the hashes above.

C. **Not yet**: keep the implementation and evidence and withhold CP2. For example, a KMS IPC denial check or a relative-path check could be requested first.

No approval has been recorded.

## Rollout and rollback

Rollout stays **BLOCKED** under [containment](reports/rollout-containment.md). Nothing is unblocked.

Every S2-K file except `read.rs` is shared, so a whole-file revert is not available. A rollback removes the S2-K regions **as a matched set**:
- the gate function and all six call sites;
- the status changes;
- `secret_namespace` with its call;
- the runner fixture, markers 549–550 and check 665.

Removing the gate without the driver/runner checks fails the runner. Removing the checks without the gate silently reopens the bypass demonstrated in the before-fix log. Rolling back reinstates that reproduced leak: the fixture bytes reach `cat`/`head`/`sort` output, a `/tmp` copy, and the UART, all with `$?=0`. There is no persisted state; the fixture exists only on private images.
