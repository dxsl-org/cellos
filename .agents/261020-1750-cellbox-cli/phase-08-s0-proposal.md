# Phase 08 — compact editor (`cellbox edit`): CP1 proposal

Status: **CP1 approved by the user on 2026-10-24**: D78-1..3, the D03 design in §3.1 (including codec allocation in `libs/api`), and slices S8-1..S8-5 with the file sets in §3.4. Each slice still needs its own CP2. Rollout remains blocked ([rollout containment](reports/rollout-containment.md)). Contract: [command-contracts §3](command-contracts.md); VFS delta: [abi-vfs-telemetry D03](abi-vfs-telemetry.md).

## 1. Facts (2026-10-24)
- **The VFS cannot save safely.**
  - `Stat` returns only size and is-dir. There is no version or generation (`libs/api/src/services/ipc.rs:39-46`; `file_handles/table.rs:20-25` keeps only a sticky `modified`).
  - `Write` creates or truncates (`backend.rs:101-103`).
  - `Rename` is no-replace only and limited to `/srv` (`dispatch_paths.rs:139-178`).
  - FAT32 deletes and then recreates the file (`backend_fat.rs:373-406`).
  - CellosFS `replace_file` is in place and labelled "not crash-safe" (`libs/cellos-fs/src/engine.rs:359-379`). Its `sync` has two barriers but disclaims crash proof (`engine.rs:705-768`).
- **`EditBuffer`** (`libs/text-engine/src/editor.rs`) is byte-indexed and has five tests. Known corruption: Backspace after `é` panics, `"\n"` is lost, mixed CRLF/LF is normalized. `MAX_UNDO_BYTES` is not enforced.
- **No `edit` command exists** in cellbox (`commands.rs:7-22`, `typed.rs:108-145`).
- **Kibi** is a reference only: revision `1c33ca09…`, MIT OR Apache-2.0, with kilo BSD-2-Clause attribution if code is adapted (`dependency-review.md`).
- **TUI pattern:** `top` uses `typed.rs` `CmdClass::Session`, then `executor::with_tui`, then `vitui::TuiSession` (`top/tui.rs:74-180`).

## 2. User decisions (2026-10-24, before CP1)
- **D78-1** D03 covers **CellosFS only** (`/srv`).
  - On FAT32 or bootfs, `edit` may open and edit a file, but save to those mounts is refused with a reason. Save As to `/srv` is still allowed.
  - Deviation from contract §3.1, which said an unsupported backend "refuses without editing". The user chose open-without-save instead.
- **D78-2 Durability** is reported as "saved (synced)" after the engine's two barriers, explicitly with no power-cut proof (contract §3.3 `CommittedDurable` uses that bounded wording). No crash experiments.
- **D78-3 Rewrite** `EditBuffer`. The old module and its tests are deleted.

## 3. Proposed design

### 3.1 D03 — conditional save (VFS + CellosFS)
- **Engine (`libs/cellos-fs`).**
  - Each inode gets a 64-bit `generation`. It increments on every content or metadata mutation and is never reused within the volume (a volume counter is stored in the superblock).
  - Two new operations:
    - `create_exclusive`: fails if the name exists.
    - `rename_replace_if(src, dst, expect)`, where `expect` is `Absent` or `Present{ino, generation}`. Within one metadata commit it checks `dst` against `expect`, swaps the directory entry, frees the old inode, then syncs. A wrong `expect` returns `Conflict` with nothing changed.
- **VFS ABI (`libs/api` `VfsRequest`).**
  - `Observe{path}` returns `Absent`, or `Present{ino, gen, size, regular}`.
  - `SaveBegin{dir}` creates the staging file `.cbx-save-<id>` in the same directory, owned by the caller generation, with a quota and at most 4 objects per caller.
  - `SaveChunk{id, off, bytes≤512}`, then `SaveSeal{id, len, sha256}`. The seal checks length and digest.
  - `SaveCommit{id, dst, expect}` returns `Committed{gen, synced}`, `Conflict`, `NotPublished(reason)` (proven: the old bytes are intact) or `Indeterminate`.
  - `SaveQuery{id}` returns the outcome of the same transaction only and never replays it. `SaveAbort{id}` exists too.
  - Staging is cleaned up on abort, when the caller dies, and at mount (a scan for `.cbx-save-*`).
- **Authorization.** The existing `VfsMutate` and path policy apply to `dst` and its directory. Non-CellosFS mounts get `NotPublished(Unsupported)`.
- **Codec allocation and owner handoff (P01 D03)** are recorded in this proposal as an owner-approved delta. This is a CP1 item.

### 3.2 Buffer (`libs/text-engine::editor`, rewritten)
- Lines hold exact bytes plus their own ending (`LF`, `CRLF` or `None`). A lone CR is content.
- The cursor moves in UTF-8 scalar steps; Up/Down keep the desired display column (SDK width rules).
- Limits: 4 MiB and 262,144 lines; invalid UTF-8 or NUL is refused.
- Undo stores inverse ranges in groups, bounded by 1,024 groups and 1 MiB, evicting whole groups from the oldest. A new edit after undo drops redo.
- Dirty means the buffer differs from the saved bytes (exact comparison through a digest plus length, then a byte comparison only when they match).
- Search, go-to-line, selection, and a private clipboard capped at 256 KiB.
- Lightweight highlighting for `.toml`, `.sh` and `.rs` only (contract §3.3).
- Kibi ideas only, with no copied code. If any code is copied, attribution goes in the header.

### 3.3 `cellbox edit` (cells/tools/cellbox)
- New `edit/` module following the `top` pattern: `CmdClass::Session`, `with_tui`, `vitui`.
- Keys exactly as in contract §3.2: Ctrl-S/O/Q/Z/Y/Space/C/X/V/F/N/G/H, Escape, bracketed paste.
- Save goes through D03:
  - Open binds the observation; Save As observes the destination and asks y/n only when it is Present.
  - The confirmation is bound to that observation, and there is no automatic retry.
  - Status codes per §3.3: 0, 1, 2, 124, 125, 130.

### 3.4 Slices (each with its own CP2)

| Slice | Content | Main files |
|---|---|---|
| S8-1 | Engine generation, `create_exclusive`, `rename_replace_if`; host tests: conflict leaves nothing changed, ABA (same bytes, different generation → Conflict), mount-time staging cleanup | `libs/cellos-fs/src/engine*.rs`, tests |
| S8-2 | D03 VFS ABI + dispatch + staging table + backend trait; FAT/bootfs return Unsupported; P02 kernel cases for Observe/Commit/Conflict | `libs/api/src/services/ipc.rs`, `cells/services/vfs/src/{dispatch_*,manager*,backend*,save*.rs}` |
| S8-3 | `EditBuffer` rewrite + highlighting; host property/boundary tests (scalar boundaries, CRLF/final newline round trip, undo bounds, a rejected edit leaves state unchanged) | `libs/text-engine/src/editor*.rs` |
| S8-4 | `cellbox edit` TUI + D03 client | `cells/tools/cellbox/src/{edit/*,commands.rs,typed.rs}` |
| S8-5 | Target proof: P04 terminal driver `edit-*` (all keys, resize, dirty exit, save/reopen exact bytes on `/srv`, mixed endings); fault injection under test-hooks (concurrent original change, ABA, Save As destination created/changed after confirmation, short stage, failed commit, denied, FAT save refused) → bytes on disk unchanged + buffer still dirty; GUI scenario; P03/P02 regression | driver, runners, `scripts/phase07_rg_fixture.py` (reuses seeded `/srv`) |

## 4. Risks
- **The ABI grows by 7 requests.** The cellbox text size and VFS footprint will be measured in S8-2 and S8-4.
- **Fault injection needs a test-hooks image.** It is not acceptance and does not use the canonical image.
- **Single-threaded VFS.** A 4 MiB save is 8,192 chunks of 512 B, which is slow on TCG. The chunk limit stays at 512 B per contract; a different size would be a deviation proposal.

## 5. CP1 asks
Approve D78-1..3, the D03 design in §3.1 (including codec allocation in `libs/api`), and slices S8-1..S8-5 with the file sets above.

## 6. S8-1 results (2026-10-24) — CP2 approved by the user on 2026-10-24, including deviations 1–4
- **Format.** `InodeRecord.generation: u64` occupies the previously unused bytes 492..500, inside the CRC-covered range. Existing volumes read as 0, so there is no format-version bump.
- **Generation bump.** `CellosFs::put_inode` advances the generation on every staged mutation (saturating). All mutations go through it.
- **New primitive.** `CellosFs::rename_replace_if(src, dst, Expect)` with `Expect::{Absent, Present{inode_num, generation}}`, exported from `cellos_fs` along with the new `FsError::Conflict`.
  - When `dst` does not match `expect`, it returns `Conflict` and stages nothing.
  - On a match it swaps the directory entries, retires the old inode and frees its blocks. The caller's `sync` publishes this in one superblock flip.
- **Host tests** (`libs/cellos-fs/tests/conditional_replace.rs`, 5 tests):
  - The generation advances even on a byte-identical rewrite and survives reopen.
  - A matching replace publishes, frees exactly the old 4 data blocks and survives reopen.
  - ABA conflicts with no change: same bytes, free count unchanged, only the bare sync flip.
  - Absent/present races are refused both ways.
  - A directory destination is refused; cross-directory publication works.
- **Runs.**
  - `cargo test -p cellos-fs` (host target): all pass.
  - With `g2-cow-experiment`: pass.
  - `service-vfs` check and host tests 94/94.
  - `cellos-fs-provision` 5/5.
- **Deviations (for CP2).**
  1. The generation is a per-inode counter, not a superblock volume counter. `inode_num` is never reused (`next_inode_num` is monotonic), so the pair (inode_num, generation) is already ABA-free without touching the superblock.
  2. No new `create_exclusive`: the existing `create_file` already refuses an existing name with `AlreadyExists`, and S8-2 uses it for staging.
  3. Staging cleanup at mount moves to S8-2, because the `.cbx-save-*` naming belongs to the VFS rather than the engine.
  4. The host-only `g2-cow-experiment` build lacks `rename_replace_if`, and the new test file is gated off under that feature.

## 7. S8-2 results (2026-10-24) — CP2 approved by the user on 2026-10-24, including deviations 1–6
- **ABI (`libs/api` `ipc.rs`).** New variants are appended after `FileHandleState`, so no existing index shifts:
  - Requests: `Observe{path}`=33, `SaveBegin{dst}`=34, `SaveChunk{id,offset,bytes}`=35, `SaveSeal{id,len,sha256}`=36, `SaveCommit{id,expect}`=37, `SaveQuery{id}`=38, `SaveAbort{id}`=39.
  - Responses: `Observed`=13, `SaveId`=14, `SaveOutcome`=15.
  - New types: `SaveExpect{Absent | Present{inode,generation}}`, `Observation` (with `expect()`, which returns `None` for a non-regular object), `SaveOutcome{Committed{inode,generation} | Conflict | NotPublished(reason) | Indeterminate}`, and the `save_refusal` codes.
  - Limits: `VFS_SAVE_CHUNK`=512 and `VFS_SAVE_MAX`=4 MiB.
  - Classification: every step that changes storage needs mutation authority; only `Observe` and `SaveBegin` are path-addressed (a sealed cell cannot begin a save).
  - A wire test pins the indices and the classification.
- **VFS.**
  - `save.rs` holds the transaction table: at most 4 per caller generation and 8 in total.
    - `SaveBegin` checks write access, `may_own_state` and that the parent directory exists.
    - `SaveChunk` requires sequential offsets and checks quota on the bytes staged so far.
    - `SaveSeal` digests what storage actually holds, not the request bytes, then records the staged object's (inode, generation).
    - `SaveCommit` re-checks write access, takes the `/srv` namespace lease and applies the same net-delta quota as `write_file`. It records the first outcome; a repeat or `SaveQuery` returns that outcome and never runs the publication again.
    - `SaveAbort` removes an unpublished stage.
  - Backend trait: `stage_path`, `observe`, `stage_create`, `stage_write`, `publish_if`. Defaults mean unsupported, so FAT32, bootfs and RamFS return `Err(6)` (`ViError::NotSupported`) before anything is staged.
  - CellosFS backend:
    - Stages live in `<mount>/.cbx-save/<id>`. Writes to them are not synced individually.
    - `publish_if` checks the stage identity, runs `rename_replace_if` and syncs, all under one lock. A storage error after the engine accepted the publication poisons the backend and returns `Indeterminate`.
    - At mount it removes any leftover `.cbx-save/*`.
  - Change marks: the destination and the stage are marked on Committed and on Indeterminate.
- **Tests.**
  - Host `save::tests`, 6 tests through `dispatch::handle_request`:
    - create, then replace, with exact bytes (CRLF) and no replay;
    - ABA and Save As races give Conflict with no byte changed;
    - unsealed, wrong-digest and tampered-after-seal stages never publish;
    - out-of-order chunks and another caller's id are refused;
    - backends without support refuse;
    - caps hold and a successor generation inherits nothing;
    - mount cleanup works.
  - Plus a host run of the boot self-test under `test-hooks`.
  - `service-vfs`: 100/100 normally, 101/101 with `--features test-hooks`.
- **Target (private TCG).**
  - P02 kernel lane: 39/39 cases and 7 of 7 VFS self-test markers. The four new `[vfs-save]` markers run over a private in-memory CellosFS mounted at `/srv/s8test`, which shadows rather than touches the user volume.
  - P02 session lane 48.
  - P03 67.
  - Logs are in `reports/s8-2-evidence/`.
  - `service-vfs` text is 473,667 B, +15,264 B over the S7-4 build (458,403 B).
- **Deviations (for CP2).**
  1. Stages live in one `.cbx-save` directory per CellosFS mount, not next to the destination. This makes mount cleanup a single directory scan, and the engine publishes across directories.
  2. A "dead caller" is detected only when the same cell returns with a new generation (the VFS receives no death notice). The stages of other dead callers stay until the per-caller and global caps or the next mount clean them up.
  3. Staged bytes are not charged to quota until commit. `SaveChunk` only checks that the caller's quota could cover the bytes staged so far.
  4. Any `/srv` writer can write into `.cbx-save`. This is not a hole, because the seal-time identity check refuses to publish a changed stage (`STAGE_CHANGED`, which is tested), but staged content is not confidential from other `/srv` writers.
  5. `VfsResponse::Err` code 6 (`ERR_UNSUPPORTED`) is new for the VFS. `ERR_HANDLE` (4) still overlaps `ViError::AlreadyExists`, as it did before this slice.
  6. A `SaveCommit` refused by access policy or the namespace lease returns `Err(3)` and records nothing, so the client may commit again. These refusals happen before any publication is attempted.

## 8. S8-3 results (2026-10-24) — CP2 approved by the user on 2026-10-24, including deviations 1–3
- **The old `EditBuffer` is deleted**, along with its five tests. It had no consumers anywhere in the workspace.
- **New `libs/text-engine/src/editor.rs`.**
  - The document is its bytes. Lines are `{text, ending: None|Lf|CrLf}`, and only the last line has `None`. A lone CR is content.
  - Every mutation is one splice over the serialized bytes, recorded as its exact inverse with the cursor and anchor before and the cursor after. Undo and redo therefore restore bytes, not an approximation.
  - Positions are always on a scalar boundary and never inside a terminator. If bytes merge into a CRLF (inserting `\r` before `\n`), the cursor moves past it.
  - API:
    - `from_bytes` refuses invalid UTF-8, NUL, more than 4 MiB or more than 262,144 lines; `to_bytes` returns the exact bytes.
    - Editing: `insert` (replaces the selection), `newline` (the cursor line's own terminator, else the nearest preceding one, else the first in the file, else LF), `backspace`/`delete` (one scalar, or a CRLF as one unit), `copy`/`cut`/`paste` with a private clipboard of at most 256 KiB, and `paste_text` (one group).
    - Undo and redo: `undo`/`redo`, bounded by 1,024 groups and 1 MiB across undo and redo, evicting whole groups from the oldest. A group larger than 1 MiB is refused before any mutation, and a new edit drops the redo branch.
    - Motion: `toggle_anchor`, `left`/`right`/`home`/`end`, `vertical`/`page` (keep the desired display column through a caller-supplied `width` function, with tabs on 8-column stops, landing on the closest boundary and never inside a wide cell), `go_to_line` (1..=n), and `find`/`find_next` (literal, case-sensitive, wrapping once, query at most 1,024 B).
  - Each keypress costs O(line count) offset sums with no copy of the document; only copy, cut and search copy it.
- **`editor/highlight.rs`.**
  - Suffixes `.toml`, `.sh` and `.rs` only, case-sensitive.
  - Kinds: comment, string, number, keyword (the 19 Rust keywords from the contract).
  - The only state carried between lines is a Rust `/* */` block comment or a TOML `"""`/`'''` string.
  - In shell, `#` starts a comment only at the start of a word.
  - Highlighting produces spans only; it never changes text.
- **Tests** (`editor/tests.rs`, 12):
  - exact byte round trip (empty, `\n`, CRLF, mixed, lone CR, no final newline, tabs, multibyte and combining);
  - refusals;
  - Backspace after `é`, and CRLF deleted as one unit (the containment defects);
  - Enter's terminator choice;
  - merging into CRLF, with undo;
  - undo/redo and dirty-after-undo;
  - eviction at both limits, and an oversized delete refused with nothing changed;
  - a rejected edit changes nothing;
  - selection across a CRLF;
  - vertical motion with tabs and wide cells;
  - search wrap and go-to-line bounds;
  - highlighting carry.
- **Runs:** `text-engine` 122/122 (host), the `no_std` check passes, and cellbox builds.
- **Deviations (for CP2).**
  1. Dirty is tracked by state id, not by comparing saved bytes. Undo or redo back to the saved state is clean, but typing a character and then deleting it stays dirty. This is conservative: the buffer never claims to be clean when the bytes might differ, and it avoids keeping a second 4 MiB copy.
  2. Display width comes from a function the caller supplies (in S8-4 that is the terminal SDK's width rules), so `text-engine` does not depend on `terminal-sdk`.
  3. Search starts one scalar after the cursor, so `Ctrl-N` moves on from the current match. A match is a cursor jump, not a selection.

## 9. S8-4 results (2026-10-24) — CP2 approved by the user on 2026-10-24, including deviations 1–7
- **The `cellbox edit [PATH]` command.** Code is in `cells/tools/cellbox/src/edit.rs` and `edit/{editor,render,store,tests}.rs`.
  - **Argument checks.**
    - At most one operand and no options. `-h`/`--help` prints usage and exits 0; anything else invalid exits 2.
    - A valid argv is classified as `CmdClass::Session`, so the existing typed-pipeline rules refuse it inside pipes, redirects, the background and substitutions.
    - Without an interactive terminal it exits 1 with a diagnostic.
  - **Opening a file.** The path is resolved against the cwd, then `Observe` runs before and after the read and the two must match, so the buffer is exactly the observed state.
    - Absent: a new buffer bound to `Absent`.
    - Non-regular, larger than 4 MiB, not UTF-8 or containing NUL: exit 1 with nothing changed.
    - `Err(NotSupported)` (FAT32/bootfs, per D78-1): the file opens with a note and save is refused, but Save As to `/srv` works.
  - **Editor state machine** (`editor.rs`, pure, behind the `Store` trait). Keys follow contract §3.2:
    - Ctrl-S/O/Q/Z/Y/Space/C/X/V/F/N/G/H, Escape, Enter, Tab, Backspace, Delete, arrows, Home/End, PageUp/PageDown.
    - Ctrl-C arrives as the SDK `Interrupt` event and means Copy. A bracketed paste is one group.
    - Prompts: Save As (at most 4,096 B), Find (at most 1,024 B), Go to line.
    - Dirty exit offers s/d/c (Escape = c) with no default. An existing Save As destination asks y/n (Escape or Enter = n), and that answer is bound to what was observed before the question.
    - A commit marks the buffer clean and switches the path and binding. Conflict, NotPublished, Indeterminate and a failure before staging all keep the bytes, the history and the dirty state, and keep the old association. Nothing is retried.
    - Dirty exit with Save exits only after a commit. EOF or a lost frontend exits 125.
  - **Renderer** (`render.rs`):
    - Text rows, a status line (`name *` and `Ln/Col`), and a bottom line showing the prompt, a message or key hints. Ctrl-H shows a help overlay.
    - Tabs are drawn on 8-column stops. C0 and DEL are drawn as `^X`, C1 as `<XX>`, so they are never sent as terminal data.
    - Syntax highlighting comes from S8-3; the selection is drawn reversed.
    - Long lines scroll horizontally, and a wide cell cut by the edge is drawn as blanks.
  - **Store** (`store.rs`):
    - It calls `save_stage` (chunks of 512 B, then seal with SHA-256 via `ostd::session::sha256`), then `save_commit`.
    - If the commit reply is lost it calls `save_query` on that same transaction, and if that fails too the result is Indeterminate. It then calls `save_abort` to free the slot.
  - **Terminal loop** (`interact`/`run_loop`): it follows the `top` pattern of `with_tui`, `enter_bounded` and `next_event`, and `tui.exit()` runs on every path.
- **Wiring:**
  - `typed.rs` classifies the command; `executor.rs` adds it to `BUILTINS` and dispatches it; `commands.rs` adds it to help.
  - `ledger.rs` gets an `edit` row (InSuite), the count gate goes from 57 to 58, and `command-catalog.md` gets the generated row.
  - `top::tui` becomes `pub(crate)` so `color_mode` can be reused.
  - `ostd::clients::VfsClient` gains `observe`, `save_stage`, `save_commit`, `save_query` and `save_abort`; the client maps code 6 to `ViError::NotSupported`.
- **Tests** (`edit/tests.rs`, 8, using a store that records every call):
  - a save is bound to the opening observation, a conflict keeps everything and nothing is re-observed or retried;
  - Indeterminate, NotPublished and Failed never mark the buffer saved;
  - Save As onto an existing file needs y and commits against the observed identity, Enter means no, and a failed Save As keeps the association;
  - dirty exit has no default and exits only after a commit;
  - a mount without D03 is refused before any store call;
  - Ctrl-C copies and EOF exits 125;
  - an unnamed buffer saves through Save As, and Escape cancels a prompt;
  - control characters are drawn escaped and tabs expanded, with the bytes unchanged.
- **Runs:**
  - cellbox host tests 178/178; the workspace check is clean.
  - P03 lane 67, so the shell does not regress (`reports/s8-4-evidence/`).
  - cellbox text 2,354,972 B, +68,744 B over the S7-3 build.
  - Interactive use on the target (serial/GUI, saving, conflicts) is S8-5 by plan and has **not been run in this slice**.
- **Deviations (for CP2).**
  1. `libs/ostd` (the D03 client methods and the NotSupported mapping) is outside the S8-4 file set in §3.4.
  2. Long lines scroll horizontally instead of soft-wrapping; no bytes are inserted either way.
  3. The view is capped at 120×40, the same bound as `top`, keeping two Ratatui buffers of 4,800 cells.
  4. Type-ahead before the screen is entered is discarded, as `top` does; EOF at that point exits 125.
  5. `edit` never returns 130: Ctrl-C is Copy, and there is no other cancel key. Losing the session returns 125.
  6. Display width comes from Ratatui (`Span::width`, i.e. unicode-width), which is what the buffer actually draws with. C0 controls count as 2 columns and C1 as 4.
  7. A dirty session that dies from a terminal failure prints `unsaved changes were not written` after the screen is restored.

## 10. S8-5 results (2026-10-24) — CP2 approved by the user on 2026-10-25, including deviations 1–2; the user asked for more on-target checks (find, go-to, selection/copy/cut, redo) before closing the phase
- **Terminal runner.** `cells/tests/phase04-terminal-driver/src/edit.rs` adds nine markers.
  - Host `t` runs `edit`. Session `a` writes the concurrent changes and reads the stored bytes back with `cat`, cut to the `ls --records` size.
  - Files live on the fixture's CellosFS `/srv` (`/srv/ed`) and FAT32 `/mnt/sd`.
  - The markers:
    - `edit-refusals`: `edit one two` gives 2; a directory gives 1; `edit … | cat` is refused before any screen.
    - `edit-save-reopen-exact`: End, `1`, Down, `é`, Backspace, `2`, Enter, Tab, `new`, then Ctrl-S. `one\r\ntwo\nlast\n` becomes exactly `one1\r\ntwo2\n\tnew\nlast\n` (CRLF kept, LF from the line's own ending, final newline kept). Exit is 0 and a reopen is clean.
    - `edit-conflict-keeps-buffer`: `a` rewrites the file after `t` opened it. Ctrl-S says it changed, the stored bytes are `a`'s, `t` stays dirty and discards.
    - `edit-saveas-overwrite-race`: Save As onto an existing file asks y/n; `a` then rewrites the file; `y` gives a conflict and `a`'s bytes stay. Save As to an absent name creates it, and the buffer takes that name.
    - `edit-fat-save-refused`: a file on `/mnt/sd` opens, Ctrl-S is refused, and the stored `fat` is unchanged.
    - `edit-paste-one-group`: a bracketed paste of `a\nb\r\nc`, then one Ctrl-Z, makes the buffer clean. Ctrl-Q exits at once and the file is never created.
    - `edit-dirty-exit-no-default`: Enter is not an answer, `c` returns to the buffer, `d` exits 0.
    - `edit-quit-restores`: `?1049l`, `?25h` and `?2004l` arrive in order and the screen model is back on the primary screen.
    - `edit-resize`: a resize to 100×30 and back moves the status row and keeps `Ln 1, Col 5`.
- **GUI scenario** (`scripts/phase04_gui_drive.py` `edit`, EDIT-0..5, real QMP keys in the GUI Terminal):
  - the typed line is exact;
  - the editor's status row carries the surface colour on all 79 cells, and the first row is empty;
  - `hello` is drawn in cells 0..4 with nothing after it;
  - Ctrl-Q on the dirty buffer asks;
  - `d` restores the prompt screen exactly, and `echo st=$?` shows `st=0`.
  - The GUI lane has no CellosFS volume, so saving is judged by the terminal runner only.
- **Runs (private TCG):**
  - terminal **458/458** (173 markers, all 9 `edit-*`);
  - GUI **290** (all 11 EDIT checks);
  - P02 48;
  - P02 kernel lane 39/39 with 7/7 VFS self-test markers;
  - P03 67 (S8-4).
  - Logs and EDIT screenshots are in `reports/s8-5-evidence/`. Canonical hashes are unchanged.
- **Failed attempts on the way:**
  1. `wc -c` is not a cellbox option. Cellbox `wc` also reads only RamFS paths, so the read-back switched to `ls --records` for the size plus `cat`.
  2. One terminal run did not boot: an NVMe flush timed out (`io timeout opc=0`) with the kernel completion-queue self-test FAIL in the same boot, before any `edit` code ran. The identical rerun passed. Recorded as a TCG flake; its log is not kept as evidence.
  3. GUI EDIT-1 first judged ink against the window background, but the editor paints its own palette background. Glyph ink is now any pixel that is neither background.
- **Not shown on the target (host or VFS self-test evidence only):**
  - the destination created between Save As observation and commit (VFS `stale-observation`/host `save::tests`);
  - short or tampered staging and failed replacement (`[vfs-save] tampered-stage-not-published`, host `unsealed_tampered…`);
  - Indeterminate (host `edit::tests` only; it cannot be injected without a fault hook);
  - Ctrl-F/N/G, selection/copy/cut and redo through the terminal (host `edit::tests` and `text-engine` tests);
  - frontend loss under the editor (exit path 125 is host-tested only).
- **Deviations (for CP2).**
  1. Fault injection uses the existing evidence layers (VFS boot self-test on the test-hooks image, host tests) instead of a new target fault hook for short staging, failed replacement and indeterminate commits.
  2. The GUI scenario judges structure, glyph ink and exact restored screens, not a pixel-exact editor screen.
- **Added after CP2 (2026-10-25, user request).** `edit-find-select-redo` runs on `/srv/ed/f.txt` (`alpha\nbeta\nalpha\n`).
  - Ctrl-F `alpha` lands on Ln 3. Ctrl-N wraps to Ln 1 with a "wrapped" note. Ctrl-G 2 goes to line 2.
  - Ctrl-Space then End selects `beta`; Ctrl-C copies it and Escape drops the selection. End of line 3 then Ctrl-V gives Col 10.
  - Ctrl-G 1, Ctrl-Space, End, Ctrl-X cuts `alpha`. Ctrl-Z goes back to Col 6; Ctrl-Y redoes the cut.
  - Ctrl-S stores exactly `\nbeta\nalphabeta\n`.
  - Terminal 459/459 (174 markers), PASS on the first run. Logs are in `reports/s8-5-evidence/find-select-redo/`. Phase 08 is closed as completed-private-evidence.
