# Phase 04 slice C5 — headless independence and desktop↔CLI backend parity (CP1 proposal)

Status: **CP1 APPROVED by the user on 2026-10-22: D-C5-1 Terminal + Files + power actions + dead launcher entries; D-C5-2 remove power actions; D-C5-3 full Files create/copy/move/delete.** Target: Phase 04 criterion 5. Evidence: read-only census of 2026-10-22; catalog §5 is superseded by §1 below.

## 1. Current state (facts)
| Desktop operation | Today | Cellbox counterpart | Parity |
|---|---|---|---|
| Terminal commands | `cells/apps/terminal/src/session.rs:1-145` runs the real `/bin/cellbox sh` through `TerminalHost`; no matcher | identical process | **Met** (S3 GUI evidence) |
| Files: list/stat/read/create/copy/move/remove | `cells/apps/files/src/main.rs:90-120` fixed entries for `/`, `/bin`, `/data`; `:136-210` mutate only an in-memory list and clipboard; no VFS client imported (`:20-24`) | `ls/stat/cat/mkdir/cp/mv/rm/touch` over typed `VfsRequest` (`cmd_fs.rs:68-83,281-747,896-970`; `VfsClient` stat/read) | **Fabricated** |
| Spotlight Shutdown / Reboot | `cells/apps/desktop/src/main.rs:172-180,289-296` prints "Authorized … initiated" and closes the HUD; no syscall or IPC | builtin `shutdown` refuses (S2c-min); retained `/bin/shutdown` worker; no reboot provider exists anywhere | **Fabricated** |
| Launcher entries `/bin/app-sys-tools`, `/bin/service-httpd`, `/bin/app-c-math-smoke` | `cells/apps/desktop/src/apps.rs:76-103`. `app-sys-tools` is not a sys-tools binary; the httpd target is `/bin/httpd`; math-smoke is a verification-only fixture | none valid | **Dead** |
| Taskbar/Spotlight app launch (real apps) | `desktop/main.rs:153-158,225-229,272-275,334-350` → `sys_spawn_from_path`, authorized by kernel launch rows | not a data operation; independent launcher | Out of parity scope (presentation/launch) |
| Ocel local read/save, network | Ocel's own VFS/NET clients (`ocel/src/resources.rs:263-280`, `main.rs:1525-1771`, `net/http.rs:276-423`) | `cat`/redirect, retained `/bin/curl` | Same services, separate clients. Browser product, **not selected** for Phase 04 |
| Chrome guest launcher | UI only; no backend data path | none | Not applicable |
| `rg` / `edit` / `ssh` / Cell-first `top` | Not implemented | Phases 05–09 / D04 | Not selected for Phase 04 |

Headless: the generated default boot selects compositor/fb-console/desktop rows only with `--config-features ui`. The Phase02/03/04 terminal runners boot with `input` only, and `app-cellbox` has no viUI/compositor dependency. This is a fact from config and Cargo, not yet a recorded runner assertion.

## 2. Proposed slice C5
**C5a — headless proof (runner and checks only):**
- In the existing Phase04 terminal runner (`input`-only image), require that `ps --records` lists no `compositor`, `desktop` or `fb-console` task.
- Keep the existing passing checks for `ls/cat/ps/free/top`.
- Add a build-time check that `cargo tree -p app-cellbox` contains no `viui`, `compositor` or `vi-surface` crates.

**C5b — Files on the shared VFS backend:**
- Replace the fabricated model in `cells/apps/files` with `ostd::clients::VfsClient`, the same typed `VfsRequest` family cellbox uses: `ListDir`/`Stat` for browsing, bounded `read` for preview, `Mkdir`, write for new files, `Rename` for move, and read+write for copy (as `cp`), `Unlink`/`Rmdir` for delete.
- Errors show the VFS refusal (`ERR_DENIED`, IO) and never pretend success.
- No CLI output parsing and no shared mutable state with cellbox. Parity means both call the same service with the same request types.
- Files' syscall declaration gains `VfsMutate` and whatever the client needs (a **ceiling delta for `/bin/files`**). Its launch rows already carry W after S2-T/W (`LEGACY_OBSERVE_MUTATE`); no kernel row change.
- Paths and recursion are bounded the same way as `cp -r`/`rm -r` today (reuse limits, no new limits).

**C5c — remove fabricated and dead desktop actions:**
- Delete the Spotlight Shutdown/Reboot actions and the three dead launcher entries from `cells/apps/desktop`.
- A real desktop power action needs supervisor `ResetSystem` and a platform provider (catalog Z), which do not exist. Reboot has no provider at all. Removing the fake actions is the honest state; their return belongs to a later slice once a provider exists.

**Not in C5:** Ocel, Chrome, `rg/edit/ssh`, Cell-first top, any new reboot/poweroff provider, canonical image/rollout.

## 3. Verification
- **Host:** Files model tests over a fake `VfsClient` boundary, if the crate already has a test seam. Otherwise a throwaway check only; no tests that echo mocks.
- **Private desktop image** (extend `scripts/qemu-phase04-gui-test.sh`, `input,ui`):
  1. In the GUI Terminal, `mkdir /tmp/c5dir; echo c5 > /tmp/c5dir/a.txt`.
  2. Open Files at `/tmp/c5dir` (real taskbar click) and screen-check that `a.txt` is listed.
  3. In Files, create folder `c5new` and copy `a.txt` into it; in Terminal, `ls /tmp/c5dir/c5new` and `cat /tmp/c5dir/c5new/a.txt` show `c5`.
  4. Delete it in Files; Terminal `ls` no longer lists it.
  5. Files on `/srv/cellos` shows a denial, not an empty fake listing.
  6. Spotlight search for "shutdown"/"reboot" returns no action.

  All of these are exact-screen or exact-stream checks, as in the S3 GUI runner.
- **Headless:** the C5a checks in the Phase04 terminal runner.
- **Regression:** Phase04 terminal (353), GUI runner baseline (103 checks), Phase03 (60), Phase02.
- **Not claimed:** KVM, physical display, Ocel parity.

## 4. Risks
- The Files UI rewrite is the largest piece, and GUI screen checks depend on glyph fixtures (`scripts/phase04-gui-glyphs`). New strings need new glyph references.
- Files gaining `VfsMutate` is a real authority increase for that app, bounded by VFS path policy and its launch row. It is intentional, since a file manager writes files.
- Removing the Shutdown/Reboot entries is user-visible.

## 5. Decisions requested (recommendations first)
- **D-C5-1 Selected operations:** *Terminal + Files + Spotlight power + dead launcher entries* (recommended), or also include Ocel local read/save (shared client refactor; larger).
- **D-C5-2 Desktop power actions:** *remove until a provider exists* (recommended), or route Shutdown to the retained `/bin/shutdown` worker through a new reviewed desktop→`/bin/shutdown` edge (a new Z-bearing launch edge from the GUI, i.e. an authority delta).
- **D-C5-3 Files mutation scope:** *full set: create/copy/move/delete* (recommended; it matches the census), or read-only Files (list/stat/preview) in Phase 04 with mutations later.

## 6. Rollback
Revert `cells/apps/{files,desktop}` and the runner checks. Files' ceiling returns to its old value. No persisted state beyond fixture files under `/tmp` on private images.

## Results (2026-10-22)
- **C5a:**
  - The terminal runner asserts `cargo tree -p app-cellbox` has no viUI/compositor crate (fail-closed if `cargo tree` itself fails).
  - Driver marker `headless-no-ui-tasks`: `ps --records` on the `input`-only image lists no compositor/desktop/fb-console task.
- **C5b:**
  - `cells/apps/files` rewritten onto `ostd::clients::VfsClient`: ListDir/Stat, 64 KiB Quick Look read, Mkdir, empty-Write new file, copy as read+Write/Append (400 B chunks, 1 MiB limit), move via Rename (only `/srv`→`/srv`) else copy+remove, Unlink/RmdirRecursive (`/data`)/Rmdir.
  - VFS refusals are shown in the status bar.
  - Files ceiling: +`VfsMutate` only (13→14 syscalls).
  - `libs/ostd/src/clients/vfs.rs` gained `rmdir`, `rmdir_recursive`, `rename` (outside the written closure; same client pattern).
  - Files copy has no BootFS fallback, refuses copying a folder into itself, and stops at the first error.
- **C5c:** Spotlight Reboot/Power Off removed; dead `/bin/app-sys-tools`, `/bin/service-httpd`, `/bin/app-c-math-smoke` entries removed.
- **Verified (logs in `reports/c5-evidence/`):**
  - Files host tests 21, Desktop 2.
  - **GUI runner exit 0, 173/173 checks** (TCG, click launch), including C5-1..C5-6 exact-screen checks with one-char and before-frame controls: Terminal creates the file → Files lists it → Files mkdir/copy/paste → Terminal `ls`/`cat` see the copy → Files delete → Terminal no longer lists it → Files `/srv/cellos` shows the denial → Spotlight has no power actions. Canonical image hashes unchanged.
  - Headless terminal runner exit 0, **354/354** (88 markers).
  - Phase03 60/60, Phase02 exit 0.
- **Found, not fixed:** the input layout maps the real Delete key (evdev 111) to Unknown, so no app receives it; the GUI runner uses keypad Del. This needs an input-service fix.
- **Not exercised:** autoload GUI launch for the new scenario, repeated GUI runs (earlier S3 practice was 6x), KVM, physical display, Ocel parity (not selected).
