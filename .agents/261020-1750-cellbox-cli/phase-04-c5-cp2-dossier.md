# C5 CP2 dossier: headless independence and desktop↔CLI backend parity

**Status: CP2 APPROVED by the user on 2026-10-22 (“duyệt 2 CP2”) for dossier SHA-256 `131ec5a5ebe002fd493023c9c2e31b2a334bdd1b107f808056430029034b81d6` (digest before this status amendment).** CP2 would authorize the C5 source as the C5 implementation baseline **only**: the hunks attributed to C5 below, at the exact file hashes listed. Quarantine, canonical packaging, release signing and rollout stay **BLOCKED**.

Prepared 2026-10-22 under D-CL2 of the [closure review](phase-04-closure-review.md) (§2, §3), from:
- the current source bytes;
- read-only `git diff`/`git status` against `HEAD` (`3471028bc`), plus the Phase 01 [baseline](baseline.md) record of already-dirty files;
- the saved logs in `reports/c5-evidence/`.

The only new execution was `sha256sum`/`stat`/`wc` and read-only git/grep. No build, test or QEMU run was performed for this dossier.

## Authority

- [C5 proposal](phase-04-c5-desktop-parity-proposal.md) (current SHA-256 `7e5685ebe47ee04f26f386f0b1c69ae0139dd7b46866c19e244956d64c043769`): **CP1 APPROVED** by the user on 2026-10-22 with these decisions:
  - D-C5-1: Terminal + Files + Spotlight power actions + dead launcher entries;
  - D-C5-2: remove the power actions until a provider exists;
  - D-C5-3: the full Files set (create/copy/move/delete).
- Implementation results are in the proposal's "Results (2026-10-22)" section. The closure review §3 records four more click runs and one autoload run. This dossier adds no scope.
- **Builds on S2-T/W:** Files' launch rows already carry W through `LEGACY` (`targets.rs:72`, `profiles.rs:30`, `boot_ceiling.rs:186`). C5 changes no kernel row. See the [S2-T/W CP2 dossier](phase-04-s2tw-cp2-dossier.md).
- **Builds on S3-gui:** the GUI runner, driver and glyph helper are the [S3-gui CP2](phase-04-s3-gui-cp2-dossier.md) files, extended by C5.

## Delta summary

**C5a: headless proof (runner and checks only)**
- The Phase04 terminal runner fails closed if `cargo tree --offline --locked -p app-cellbox -e normal` fails, or if its output lists a crate matching `viui*`, `*compositor*` or `vi-surface*`.
- New driver marker `headless-no-ui-tasks`: the `ps --records` output that `builtin-ps-records` already takes must be non-empty and must contain no task named `compositor`, `desktop` or `fb-console`.

**C5b: Files on the shared VFS service**
- `cells/apps/files` is rewritten onto `ostd::clients::VfsClient`. The new `fs.rs` contains the data path and a `FileSystem` seam whose only production implementation is `Vfs`.
- Operations and requests:
  - browse: `ListDir` plus `Stat`;
  - Quick Look: bounded read, 64 KiB;
  - new folder: `Mkdir`;
  - new file: empty `Write` after `Stat` finds nothing;
  - copy: one bounded read (1 MiB) plus `Write`/`Append` in 400-byte chunks. Folders are copied iteratively; Files refuses to copy a folder into itself and stops at the first error;
  - move: `Rename` only when both paths are under `/srv/`; otherwise, or if that rename is refused, copy then remove;
  - delete: `Unlink` for files, `RmdirRecursive` for folders under `/data/`, `Rmdir` for other folders.
- VFS refusals appear in the status bar. The fabricated sample listing, preview text and in-memory model are gone.
- Files' declared ceiling gains **`VfsMutate` only**: 13→14 syscalls (`main.rs:41-57`, against `HEAD`'s 13).
- `libs/ostd/src/clients/vfs.rs` gains `rmdir`, `rmdir_recursive`, `rename` and a private `call_ok` helper. This file is outside the CP1 closure; see deviation 1.

**C5c: fabricated and dead desktop actions removed**
- The Spotlight `Reboot`/`ShutDown` actions, their two `main.rs` match arms each (which printed "Authorized system … initiated") and the System-section rows are removed. `SYSTEM_ACTIONS` now holds only Appearance Theme.
- The launcher entries `/bin/app-sys-tools`, `/bin/service-httpd` and `/bin/app-c-math-smoke` are removed from `DEFAULT_CATALOG`.

**GUI proof extension:** proposal §3 steps 1-6 run between the stderr and Ctrl-C scenarios. The glyph helper gains an `ops` mode (replay of viui `fill`/`stroke`/`label`/`button`) and a `lineheight` mode.

**Unchanged:** kernel, launch rows, policy/signer, the VFS service, `libs/api`, cellbox source, input service, `Cargo.toml`/`Cargo.lock` (current hashes equal the S2-T/W dossier's), and the Files/Desktop `Cargo.toml` files.

## Evidence

Line numbers refer to `reports/c5-evidence/`. Each run exercised the **combined** tree: C5 plus every earlier delta.

### GUI runner: six runs

All six logs have 738 lines and identical line positions for the cited checks. Each run has its own work dir and its own throw-away key:

| Log | Work dir | Key identity (`:21`) | Launch (`:376`) |
|---|---|---|---|
| `gui.log` | `-lTb7` | `5cc71677…` | click |
| `gui-click2.log` | `-vuvV` | `b2b01921…` | click |
| `gui-click3.log` | `-cytw` | `37cc4f02…` | click |
| `gui-click4.log` | `-0clJ` | `c47fa6bf…` | click |
| `gui-click5.log` | `-M7Ne` | `843e2050…` | click |
| `gui-autoload.log` | `-7dne` | `cd77f03f…` | autoload |

| Check | Saved log lines (same in all six logs) | Result |
|---|---|---|
| Files packaged and signed | `:84` `Compiling files`; `:544` `"files": …` in signed provenance (13+2 cells signed at `:320`/`:375`; the archived S3-gui runs signed 12+2 and have no `files` key) | Per-run hashes differ (`c9207355…`, `b8bdc6b4…`, `cd712776…`, `cc8cc589…`, `48c7ce5d…`, `e13962c2…`) because each run signs with its own key |
| Repository checks: no fabricated Files/Desktop strings, Files uses `VfsClient` and `VfsMutate` | `:4` banner, then no `FAIL` before `:20` | Runner 163-183 runs under `set -euo pipefail`. A hit would `SystemExit` before signing |
| Files ceiling delta | `:15-19`: baseline `HEAD` 13 syscalls, candidate 14, `ADDED vs baseline: VfsMutate` | The `G-S1a FAIL` wording is the informational diff tool's (`|| true`, runner 200-203), not a gate |
| Cellbox ceiling unchanged | `:9` `G-S1a PASS` | — |
| C5-1: Terminal creates `/tmp/c5dir/a.txt` | `:643-652` (`C5-1-mkdir-*`, `C5-1-write-*`) | Exact screen, before-frame and one-char controls |
| C5-2: Files (taskbar click) lists `a.txt` | `:653-655` | Exact Files path label, list pane and status bar |
| C5-3: Files mkdir, copy, open, paste; CLI sees the copy | `:656-667` (Files); `:668-677` (`ls`/`cat` show `a.txt`/`c5`) | — |
| C5-4: Files deletes the file, then the folder; CLI sees both gone | `:678-696` | The delete key used is keypad Del (`kp_decimal`); see limitation 1 |
| C5-5: `/srv/cellos` shows the VFS refusal; listing stays real | `:697-699` | Expected status text is `Cannot open /srv/cellos: permission denied (VFS ERR_DENIED)` (driver 843) |
| C5-6: Spotlight has no power action | `:700-708` (`shutdown`/`reboot` no-match, System section) and `:709` `C5-6-control-removed-power-rows-would-differ` | The control shows that the removed rows would have been detected |
| UART negatives and markers | `:726` `no-command-uart:c5-typed-command`, `:728` `uart-files-ready-once`, `:729` `no-spotlight-power-claim`, `:731` `no-launch-deny`, `:732` `no-panic-or-fault` | — |
| Pre-existing S3-gui checks | `:560-642`, `:710-725`, `:727`, `:730` | Unchanged set. The autoload log differs only by its mode-aware `:561` `autoload-no-click-before-window` and `:564` `GUI-2-autoload-window-without-click` |
| Whole run | `:733` `PASS: 173 GUI checks` in **6/6** logs | 173 = 103 S3-gui + 70 C5 lines (67 `C5-*` + 3 UART checks). One check, `window-close-control` (`:714`), is informational (driver 666) and passed |
| Canonical artifacts unchanged | `:1-3` before = `:736-738` after; `:735` `PASS: canonical …` | cellbox `64eb28f3…`, terminal `636497ac…`, `kernel_fs.img` `447d0348…`, the same values recorded in the S3-gui CP2 |

**Exit status is not in the logs.** The proposal Results and closure review record exit 0. From source, the driver prints `PASS: N GUI checks` only on its `return 0` path (driver 896-901), and the runner ends `exit "$DRIVE_STATUS"` (runner 424) after the canonical check. "Exit 0" is therefore an `[INFERENCE]` from these lines plus the recorded Results.

### Headless and regression runs

| Check | Saved log lines | Result |
|---|---|---|
| C5a: cellbox has no UI crate | `phase04-terminal.log:7` `C5a PASS: app-cellbox dependency tree has no viUI/compositor crate` | Build-time check (runner 163-168) |
| C5a: no UI task at runtime | `phase04-terminal.log:533` `PASS marker:headless-no-ui-tasks` | Image plan: `:333` autoload = driver + `/bin/free`, `:334` services = `e1000`, `input`, `net`. No compositor/desktop/fb-console row |
| Existing data commands on that image | `:530` `builtin-free`, `:532` `builtin-ps-records`, `:536-537` `file-create-ls`/`file-cat-roundtrip`, external `ls`/`cat` markers | — |
| Whole runner | `:873` `no-FAIL-markers`, `:880` `PASS: 354 Phase04 S3-output runtime checks (88 driver markers)` | 355 `PASS`-prefixed lines (354 checks + summary) and 88 `PASS marker:` lines. Previous S4-final run: 353/87. The `C5a PASS` line at `:7` is not counted in either number |
| Phase03 regression | `phase03.log:397-456` (60 `PASS`), `:457` completion; `:395` final probe `halt_proven: true`, samples `[14, 15]` | 60/60 |
| Phase02 regression | `phase02.log:407` `passes: 48`, `:408` orphan ticks 4, `:409` completion | No exit status is printed. "Exit 0" comes from the Results |
| Files host tests 21, Desktop 2 | **No raw log saved** | Results only. The current source has exactly 21 `#[test]` in `cells/apps/files/src` (app 7, fs 7, main 3, model 2, preview 2) and 2 in `cells/apps/desktop/src` (spotlight 1, taskbar 1). That is a count, not a run |

`phase04-terminal.log:11` `G-S1a FAIL` (and `gui.log:13`) is the informational Terminal `SetTimer` delta approved earlier. It is not C5.

**Evidence ↔ current source.**
- Every run's `cargo_lock_sha256` (`phase04-terminal.log:487`, `gui*.log:532`) equals the current `Cargo.lock` (`e58e780a…`).
- The runner writes `product-source-digests.json` (directory digests of `cells/apps/{desktop,files}`, `libs/ostd/src`, …; runner 184-192) to its work-dir evidence. That file was **not archived** (no `sanitized evidence saved` line). No saved per-file or per-directory source digest ties the runs to the bytes below.
- Saying the runs used exactly these bytes is an `[INFERENCE]`. All inventory mtimes (latest `qemu-phase04-gui-test.sh` 20:27, `CHANGELOG.md` 20:28) precede the log files' mtimes (20:48 for `gui.log` and the regression logs, 21:11 for the five reruns), but those are copy times.

## Proposed implementation inventory — current disk SHA-256

**Exclusive to C5** (new files, or every changed hunk against `HEAD` is C5):

| Path | State | Bytes | SHA-256 |
|---|---|---:|---|
| `cells/apps/files/src/fs.rs` | new (untracked): VFS data path, `FileSystem` seam, bounds 29-33, copy/remove/move/create 213-305, in-memory test tree and 7 tests 307-end | 21072 | `7f8b750f3a8fad3cc6e84e63533818a0a4037157327aae810de55b7f1d690ad3` |
| `cells/apps/files/src/app.rs` | new (untracked): `FilesApp` state, prompts, copy/paste/delete 181-233, 7 tests | 17791 | `7cb315238b37a51786017bed934905974d8413656cb33e26049eb36266e98db8` |
| `cells/apps/files/src/model.rs` | modified: `size: Option<u64>` (VFS `Stat` may be refused → `?`), `mtime` removed, test | 4708 | `c140885f0d8825ee7a6b105effc497a1f8430183a706f2180e42af7df45a58c9` |
| `cells/apps/files/src/preview.rs` | modified: Quick Look over bytes read from the VFS (fabricated preview text removed) | 8069 | `2e72cf4b6294c97cbcdf21b05de4f6bed2d62364dbb47e528c0d5d62c9abc8a9` |
| `cells/apps/files/src/draw.rs` | modified: drops the unused `draw_button` re-export | 187 | `b3ace2ae54fb8d2f0a8198df6ded54f42f8679da851a42aa389df3ce82ae2911` |
| `libs/ostd/src/clients/vfs.rs` | modified: one hunk at 143-170 (`rmdir`, `rmdir_recursive`, `rename`, private `call_ok`) | 6287 | `9dd8ca876ac8eb80e7a1c9a7e43c9ed33226e20e782c4f490918f79d8e4cb4a6` |
| `cells/apps/desktop/src/apps.rs` | modified: three `AppInfo` entries removed (`HEAD` 71-84, 87-102) | 3356 | `2c8d4c88172e94e6ec9e1c5ebb30999de2bb55ba3278f101874098a22c84767c` |
| `cells/apps/desktop/src/spotlight.rs` | modified: `Reboot`/`ShutDown` variants removed, "System settings..." label (129), `SYSTEM_ACTIONS` (323-325) used by render (343), key count (435) and click (519); test 564-583 | 20731 | `eb0978321ca1faf0ce512469d7d1cd7b37da867fec7458eb17411fea56eecf7f` |

Unchanged Files crate files compiled into the evidence binary: `Cargo.toml` `dcd8b53f…`, `build.rs` `309c7f94…`, `views/{columns,grid,list,mod,places}.rs` (not dirty against `HEAD`).

**Shared files.** Each is hashed once, as the whole file, and the hash covers every delta's hunks. Approving this dossier covers only the C5 hunks named here.

| Path | Bytes | SHA-256 | C5 hunks (current lines) | Other content in the same file |
|---|---:|---|---|---|
| `cells/apps/files/src/main.rs` | 16409 | `f5120b16dfb634c2ea720d14ab9ca459e6018994938ba3c2eb3851d4bfc386d0` | Rewritten: module doc 1-6, `mod app/fs`, `declare_syscalls` + `VfsMutate` 41-57, render over `FilesApp`, key dispatch 259-301, 3 tests | **Pre-existing user-owned dirt.** At the Phase 01 baseline this file was already modified: 23366 B `fcb5224c…` versus `HEAD` 23011 B. The rewrite subsumes it, so its bytes cannot be separated. The surviving `KeyState::Repeated` branch (259-268) matches the key-repeat change visible in `desktop/main.rs` below; that it is the same behavior is an `[INFERENCE]` |
| `cells/apps/desktop/src/main.rs` | 14080 | `96d1dfba0f1c1fd4a5c4c1763b33c0e313b044f405da8d663e5c30616e3078bd` | Two `SpotlightAction::{Reboot,ShutDown}` arm pairs removed (`HEAD` 172-181 and 280-289; 666 + 626 = 1292 B) | **Pre-existing user-owned key-repeat hunks** at 231, 239-241, 254-259 (+487 B). Byte accounting is exact: `HEAD` 14885 + 487 = baseline 15372 (`de787d62…`), and 15372 − 1292 = current 14080. **Not C5** |
| `scripts/qemu-phase04-terminal-test.sh` | 43558 | `dfaf1baa1ed7e24a1be284df7e31306a96c0bd72c7394ce271df025aa05abeb0` | C5a `cargo tree` gate 163-168 (589 B); expected marker `headless-no-ui-tasks` 541 (28 B) | S3/S2-K/S2-N/S2-T/W/OOM runner content. The previous approved hash `d4cdaada…` (42941 B) + 617 B = current size exactly. That these are the only changes is an `[INFERENCE]` (the file is untracked, so no diff exists) |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 53963 | `00324c7114ad378fcc25a9473e6cea20dd56a9fbaf12c53baeed778dc04682da` | `headless-no-ui-tasks` check 400-407 (382 B) inside `builtins` | **S2-K** `secret_namespace` 446; **S2-N** `net_rights` 798; **S2-T/W** 855-918; **S4a** `sandbox-no-file` status (891-892, 896, 904, 907, as attributed in the S4a CP2 dossier); **OOM** 992-1060; S3 base. The previous hash `0139d5cc…` (53581 B) + 382 B = current size exactly. Same `[INFERENCE]` caveat |
| `scripts/qemu-phase04-gui-test.sh` | 25009 | `286a81be5670a5983db41b76ddb0e49448e22b3f39371591275c6faf2e1cb039` | Header: image list adds `files` (18), C5 scenario/oracle text (36-60); banner 139; C5 repository checks 163-183 and `cells/apps/files` in the digest list 187; Files ceiling report 200-203; `files` build/sign/image lists 240, 243, 250, 256; provenance name 366 | **S3-gui** runner (CP2 approved at `795df7ff…`, 22241 B). The file is untracked and no copy of the old bytes exists, so attribution is by content only |
| `scripts/phase04_gui_drive.py` | 45901 | `ad3bf449d5a53588b863e563f3da119b15f8116b8e1e83fe99ce81d4bdf06222` | C5 geometry/colours/regions/`SYSTEM_ACTIONS`/`REMOVED_POWER_ACTIONS`/`file_icon` 51-90; ops/line-height render helpers ≈210-231; call `self.c5(command)` 594-595; `files_ops`/`spot_ops`/diff/expect/raise/focus helpers and `c5` 670-864; UART `c5-typed-command`, `uart-files-ready-once`, `no-spotlight-power-claim` 875, 880-881 | **S3-gui** driver (CP2 approved at `8cbd955b…`, 29859 B). Attribution by content only, as above |
| `scripts/phase04-gui-glyphs/src/main.rs` | 8898 | `a355dbaf5bbc1609b836d4effbfc9a7d34c9c460849164b8f9134d05a9773ffb` | `ops` mode (doc 13-23, `surface_ops` 189-end, `fill`/`stroke`/`label` 137-186, `colour`/`face` 113-135); `lineheight` mode (doc 25-26) | **S3-gui** terminal-grid renderer `terminal_grid` 76-112 (CP2 approved at `79967ee3…`, 3196 B) |
| `CHANGELOG.md` | 289008 | `ee7bde17ec793535010c1e44974ee14c4ca16b096f74b00e37010d50fa072eac` | `[Unreleased]` entry at line 10 (C5b/C5c) | Every other in-flight entry. **The C5 entry is stale.** It ends "its QEMU run is pending", and it mentions neither C5a nor the Delete-key finding. This dossier does not edit it |

`scripts/phase04-gui-glyphs/Cargo.toml.in` is **unchanged** (`ff464ceb…`, 442 B, equal to its S3-gui CP2 hash), so it is not part of this request.

### Context: not C5 hunks, listed for completeness

- **`cells/services/input/src/layout_us_qwerty.rs`** (`5337392d2bc013dc0488d327d9adb02bceb1b6447d3d93590b91faa039091e29`) is unchanged and clean against `HEAD`. Its line 246 maps `0x6F` (evdev `KEY_DELETE`, 111) to `KeySym::Unknown`. This is the Delete-key defect in limitation 1. C5 does not touch it.
- **Plan/status docs** (`phase-04-multicall-cutover.md`, `phase-04-closure-review.md`, the C5 proposal) are documentation, not source inventory.

### Relationship to earlier approved hashes

These files changed after their earlier CP2 approval. No earlier approval extends to the current bytes.

| File | Approved in | Approved hash | Current hash |
|---|---|---|---|
| Phase04 terminal runner | S2-K / S2-N / S2-T/W / OOM CP2 | `d4cdaada…` | `dfaf1baa…` |
| Phase04 terminal driver | S2-K / S2-N / S2-T/W / OOM CP2 | `0139d5cc…` | `00324c71…` |
| GUI runner | S3-gui CP2 | `795df7ff…` | `286a81be…` |
| GUI driver | S3-gui CP2 | `8cbd955b…` | `ad3bf449…` |
| glyph `src/main.rs` | S3-gui CP2 | `79967ee3…` | `a355dbaf…` |

The Files sources, Desktop sources, `libs/ostd/src/clients/vfs.rs` and `CHANGELOG.md` have **no prior CP2 hash**. The Phase 01 baseline hashes for `files/main.rs` and `desktop/main.rs` record user-owned entry bytes, not an approval.

## Caller census (read-only grep, 2026-10-22; untracked files included)

- **New `VfsClient::{rmdir, rmdir_recursive, rename}`:** the only caller is `cells/apps/files/src/fs.rs` (86, 92, 95; used by `remove_entry` 280/282 and `move_entry` 291).
  - The other `VfsClient` users (hypha `tools/fs`, ocel, lua `bindings_vfs`, ai, kms, net-broker, bench scenarios, cpp-smoke, posix-shim-test, cellbox `cmd_fs`, init `configuration`, net-tools `httpd`, wasm loader, ostd `app.rs`, port-platform) do not call them.
  - `call_ok` is private. The pre-existing `mkdir`/`unlink` keep their own identical bodies.
- **Same request family, different wrapper.**
  - Cellbox sends `api::ipc::VfsRequest::{Rmdir, RmdirRecursive, Rename}` through its own `vfs_req_ok` (`cmd_fs.rs` 334, 359, 582, 601, 700), not through `VfsClient`.
  - The VFS service handles them in `dispatch_paths.rs` 92-139.
  - "Parity" therefore means the same service and the same `VfsRequest` variants, as the CP1 defined it. It does not mean shared client code.
- **`VfsMutate` declaration.** Files now declares allowlist bit 63, so the kernel attests `CALLER_FLAG_VFS_MUTATE` to the VFS for Files. Both conditions hold: the bit is declared, and the task holds `vfs_mutate` from its `LEGACY` row (S2-T/W `vfs_mutate_caller_flag`). The VFS `caller.rs` `may_mutate` consumes the flag and is unchanged.
- **Removed Desktop symbols.**
  - `SpotlightAction::{Reboot, ShutDown}` and the string "Authorized system" have no remaining reference in non-doc source.
  - The paths `/bin/app-sys-tools`, `/bin/service-httpd` and `/bin/app-c-math-smoke` appear nowhere in tracked non-doc source (`git grep` exit 1).
  - The package names `app-sys-tools`, `service-httpd` and `app-c-math-smoke` remain as crate/CI/build-script names. Those are not launch targets.
- **Launch path of Files:** Desktop `DEFAULT_CATALOG` `/bin/files` (`apps.rs:37`) → `sys_spawn_from_path`, authorized by the existing desktop edge (`profiles.rs:30`). No new edge was added. On the image this shows as `gui*.log:731` `no-launch-deny`.
- **Headless check inputs:** `headless-no-ui-tasks` reuses the `ps` result of `builtin-ps-records` (driver 391). The `cargo tree` gate reads only the normal-dependency graph of `app-cellbox`.

## Known limitations / not exercised

1. **Delete-key input defect: found, NOT fixed.**
   - `layout_us_qwerty.rs:246` maps evdev `KEY_DELETE` (111, `0x6F`) to `KeySym::Unknown`, so the dedicated Delete key never reaches any cell.
   - Files deletes only on `KeySym::Delete` (`main.rs:290`), and there is no mouse/menu delete path. On a keyboard without a keypad, Files therefore cannot delete at all.
   - The GUI proof uses keypad Del instead (`kp_decimal`; Num Lock off maps `0x53` → `Delete`; driver 817-821, 831). The fix belongs to the input service, outside C5, and needs its own CP1.
2. **GUI coverage.**
   - Exercised on the image: copy+paste, mkdir, file delete, empty-folder `Rmdir` under `/tmp`, the `/srv/cellos` denial, and Spotlight.
   - **Not exercised on the image** (host tests only, whose raw logs were not saved): cut/move (`Rename` on `/srv`, copy+remove fallback), new empty file (Ctrl+Shift+N), Quick Look read, `RmdirRecursive` under `/data/`, folder copy, the 1 MiB copy limit, Files list/grid/columns view switching.
3. **Delete is immediate and irreversible.** There is no confirmation step. Under `/data/` one keypress removes a whole tree (`RmdirRecursive`). The status text says "(irreversible)", but no undo exists.
4. **Partial-failure states.**
   - Copy stops at the first error and may leave a partial copy; the view refreshes to show what the VFS holds (`app.rs:219-220`).
   - Move outside `/srv/` is copy-then-remove. If the removal fails, both copies remain; this always happens for a non-empty folder outside `/data/`, because `Rmdir` handles empty folders only.
   - `copy_tree` has no entry-count or depth cap of its own. It is iterative, so the cell's stack is not at risk; the tree is bounded only by VFS path limits and the 4 MiB heap. The same holds for cellbox `copy_dir_recursive`, which has no depth cap either.
5. **Authority is coarse.** With W and `VfsMutate`, Files may write and remove under every VFS-open mutable prefix (`/tmp`, `/data`, `/srv`, `/mnt/sd`), the same coarse right as other `LEGACY` Cells. Scoped D03 remains in Phase 08.
6. **Headless scope.**
   - The terminal image is not literally `input`-only: since S2-N its services are `e1000`, `input` and `net` (`phase04-terminal.log:334`).
   - The runtime check is name-based, using the three task names, from one `ps` snapshot.
   - The same runner compiles the GUI `terminal` crate as a compile proof (runner 243-244), which is why `Compiling viui` appears at `phase04-terminal.log:140`. That crate is not packaged on the image. The C5a claim covers `app-cellbox`'s dependency tree and the running task set, not the whole build.
7. **Oracle independence.**
   - Expected Files/Spotlight pixels come from the glyph helper replaying viui primitives, and from geometry/colour constants copied by hand from guest source into the driver (51-90).
   - An error shared by the oracle and the guest would not be caught. Each positive match carries one-char and before-frame controls, and C5-6 adds a removed-rows control, so the checks are sensitive to the guest's actual screen. They do not independently verify layout.
8. **Evidence archive gaps.**
   - Only runner stdout was saved. Screens, serial logs, `verdict.json` and `product-source-digests.json` were not archived, unlike `s3-gui-tty-evidence/`.
   - Exit statuses are not printed (see the GUI evidence section).
   - Host-test runs (Files 21, Desktop 2) and any `ostd` host test after the `vfs.rs` change have no raw log.
9. **Platform coverage.** TCG only. Six runs (5 click + 1 autoload) are repeat evidence, not a failure-rate bound. Not run: KVM, physical Intel/VMware display, resize/multi-window beyond the Terminal/Files raise-and-focus sequence, Ocel parity (not selected).
10. **The proposal Results line "Not exercised: autoload … repeated GUI runs" is superseded** by the five later logs (closure review §3). The proposal text itself was not amended.

## Deviations (proposal and Results, cross-checked against source)

1. **`libs/ostd/src/clients/vfs.rs` was edited outside the CP1 closure.** Proposal §2/§6 named only `cells/apps/{files,desktop}` and the runner checks. Without the three new methods, Files could not send `Rmdir`/`RmdirRecursive`/`Rename` through `VfsClient`. The Results disclose this.
2. **A new test seam.** Proposal §3 allowed a fake `VfsClient` "if the crate already has a test seam", otherwise only a throwaway check. The crate had none. C5 added the `FileSystem` trait, with `Vfs` as the only production implementation and an in-memory tree in `#[cfg(test)]`. The tests check Files' operation logic, not the VFS service.
3. **Delete covers more than the CP1 named.** Proposal §2 said `Unlink`/`Rmdir`. C5 also sends `RmdirRecursive` for `/data/` folders, mirroring cellbox `rm -r` (`cmd_fs.rs:454`).
4. **Move is `Rename` only for `/srv/`→`/srv/`.** The VFS accepts `Rename` only for a regular file on the same `/srv` mount. Elsewhere Files does copy+remove, as cellbox `mv` does.
5. **Copy stops at the first error**, whereas cellbox `copy_dir_recursive` continues (`ok &=`) and reports failure at the end.
6. **The GUI step-4 delete uses keypad Del** because of limitation 1. The proposal assumed a working Delete key.
7. **Files keeps key-repeat handling** (`main.rs:259-268`). This carries forward pre-existing user-owned behavior (see the shared-files table); it is not new C5 scope.

## What CP2 would authorize

Approving CP2 would make the C5 hunks identified above, at the listed whole-file hashes, the C5 baseline:
- Files on `VfsClient`, with its +`VfsMutate` ceiling;
- the three `VfsClient` methods;
- the Desktop removals;
- the GUI-proof extension and the C5a runner/driver checks.

Later work could then rely on Files and cellbox reaching the same VFS service with the same request types, and on the absence of fabricated power actions.

CP2 would **not**:
- approve the pre-existing user-owned key-repeat hunks in `desktop/main.rs` (or the subsumed baseline bytes of `files/main.rs`);
- approve the S3-gui, S2-K, S2-N, S2-T/W, OOM or S4a content of the shared files;
- approve the stale `CHANGELOG.md` text as an accurate record;
- fix or accept the Delete-key defect;
- lift quarantine, or authorize canonical packaging, release signing or rollout;
- close Phase 04 criterion 4.

## Options

A. **Approve this CP2 baseline** at the exact hashes and evidence scope above.

C. **Not yet**: retain the implementation and evidence, withhold CP2.

No approval has been recorded.

## Rollout and rollback

**Rollout stays BLOCKED** under the [containment](reports/rollout-containment.md) release conditions; nothing here changes them.

**Rollback steps:**
1. Delete `cells/apps/files/src/{app,fs}.rs`.
2. Restore `model.rs`, `preview.rs` and `draw.rs` from `HEAD`.
3. Restore `files/src/main.rs`. `HEAD` lacks the user's pre-existing baseline hunk, and no copy of the baseline bytes (`fcb5224c…`) exists on disk, so the user must re-apply that hunk. Files' ceiling returns to 13 syscalls.
4. Remove the `vfs.rs` hunk (143-170). Files is the only caller.
5. Restore `desktop/src/{apps,spotlight}.rs` from `HEAD`. Revert `desktop/src/main.rs` **at hunk level only**: re-insert the two arm pairs and keep the user's key-repeat hunks.
6. Remove the C5a lines from the terminal runner (163-168, 541) and driver (400-407). Remove the C5 parts of the GUI runner, driver and glyph helper; the S3-gui scenario then counts 103 checks again.
7. Amend the `CHANGELOG.md` entry.

**Constraints on rollback:**
- No kernel, policy, signer or launch-row change is involved, so no matched kernel/image rollback is needed.
- The GUI runner's repository checks ban the old fabricated strings (runner 170-179). Reverting only Files or only Desktop, without the runner, makes the GUI runner fail closed.
- No persisted state beyond fixture files under `/tmp` on private images. Deletions a user made through Files on any image cannot be recalled.

## Evidence file hashes

| File | SHA-256 |
|---|---|
| `reports/c5-evidence/gui.log` | `7d5c7e5b380ab8c4d59c47e46a4f06b7c201d2c6ea9f1e4e0316fbe9fd97b977` |
| `reports/c5-evidence/gui-click2.log` | `5cee875457379c3db684c70bd822fc43354c2d1d6d468add94894d03366017a8` |
| `reports/c5-evidence/gui-click3.log` | `49cad4e22f4b8dccb06689a401e5f9a13b3305ebcaeb3f2e64d6f043fa801590` |
| `reports/c5-evidence/gui-click4.log` | `c962f816de718957ce31a3253af3227360146ea64eb7d85629da20b11f6344ae` |
| `reports/c5-evidence/gui-click5.log` | `7762435d7b7662245ce43c675349395d5ac2d473643fa935e70ace0fb05db45e` |
| `reports/c5-evidence/gui-autoload.log` | `b3558b3b0cd5fafcf346d262cf11cfd5fa71e04806d0e4cf8c007b81372015d2` |
| `reports/c5-evidence/phase04-terminal.log` | `381c6cab923531e7c1a180a2f870e7275508d7bf06b4f72bb5505343ccaa8898` |
| `reports/c5-evidence/phase03.log` | `c4b3fc5ac3827c10057b80db40ae73857e4f218a3a5104b11b4bf2d777efd01f` |
| `reports/c5-evidence/phase02.log` | `b989ebfce90d5404efc4dc17e14c9860de3d5afb31992c5f479f4c5822735ad1` |
