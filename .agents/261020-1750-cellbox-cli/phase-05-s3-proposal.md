# Phase 05 slice S5-3 — GUI foreground path, proof TUI consumer and selective-TUI negatives (CP1 proposal)

Status: **APPROVED retroactively by the user on 2026-10-23**, recorded with the Phase 05 CP2 ([dossier](phase-05-cp2-dossier.md) §9, §16). It covers the §3 text as implemented, D53-3 as recommended, and the unrequested `/bin/x > FILE` change in Results. D53-1/D53-2 were superseded by option C ([annex](phase-05-s3c-frontend-annex.md)). Before that the status was “PROPOSED; awaiting the user's exact selection”, and implementation followed the annex §7 schedule. Parents: [entry dossier §2](phase-05-entry-dossier.md), [S5-2 results](phase-05-s2-vt-screen-proposal.md) ("S5-3 input (gap)"). Evidence: read-only census 2026-10-23 (citations below). Spec §4 LF amendment applied 2026-10-23 (`spec.md` change log).

## 1. Facts
- **GUI Terminal controller is `TerminalHost`, not cellbox.** The Terminal opens a kind-0 session, then `launch("/bin/cellbox", ["sh"], CELLBOX_STREAMS=merged)` (`cells/apps/terminal/src/session.rs:27-30,82-90`; `libs/terminal/src/host.rs:69-108`). Keys go `TerminalHost::submit_*` → SDK `SubmitEvents` → input service → **current foreground reader** (cellbox, which holds `RawInput`) (`libs/terminal/src/session.rs:117-134`).
- **Context-mode cellbox is not a controller**, so it cannot `spawn_foreground`. Its externals run as `ExtRun`/`PipeChild` stages with EOF stdin and no session binding (`cells/tools/cellbox/src/context_session.rs:1-25,49-71`; `executor.rs:52-163,2820-2875`). On serial, cellbox is the controller and uses `spawn_foreground` (`executor.rs:2875-2925`; `libs/terminal/src/session.rs:185-220`).
- **Controller-only operations:** Create/Attach/PrepareForeground/CommitChild/Restore/Close (`cells/services/input/src/session.rs:349-352,381-409`). The kernel records an immutable bootstrap controller and requires a prepared child to be a **descendant of the controller's bootstrap operation** (`kernel/src/task/cell_session.rs:1055-1110,1337-1349`). There is no controller transfer, and the GUI frontend (kind 1) is rejected (`input/src/session.rs:348-350`).
- **Launch authority:** the Terminal's launch row admits only `/bin/cellbox` (`kernel/src/loader/launch_profile/profiles.rs` terminal profile). Arbitrary programs are launched by cellbox on its own reviewed edges.

## 2. Options for the GUI foreground path
| Option | Idea | Changes | Assessment |
|---|---|---|---|
| **A Commit-on-behalf (recommended, gated by E1)** | Cellbox prepares the child itself, on its own launch edge, as it does today for pipelines. It then asks its controller (`TerminalHost`) to run `PrepareForeground`/`CommitChild`/`Restore` for that exact prepared child. The child is a descendant of the Terminal's bootstrap operation, as the kernel already requires. The Terminal never gains launch authority; it only lends foreground. | A bounded request record (prepared `ChildRef` + epoch) carried on a dedicated control endpoint between cellbox and `TerminalHost`: `libs/terminal/{host,session}.rs`, `cells/tools/cellbox/src/{executor,context_session}.rs`, `cells/apps/terminal/src/session.rs`. Kernel/input change **only if E1 fails**. | Keeps one ownership model (the controller does foreground). No widening of the Terminal's launch edge. |
| B Controller transfer | `TerminalHost` hands the controller role to cellbox and keeps only frontend attachment | New attested transfer protocol in api/SDK/input/kernel; timing, death and lease semantics | High authority cost; changes the core model |
| C GUI frontend (kind 1) + nested session | Cellbox creates its own session; the Terminal attaches as a GUI frontend | Implements the deferred GUI bridge (input routing, identity, focus) across input/SDK/kernel | Largest; it is the deferred Phase 04/09 bridge |
| D Shell as pty proxy | Cellbox keeps the lease and relays raw input/resize to the child's stdin/control endpoints | Userspace line discipline, a stdin-mode TUI reader | Violates the phase rule "no other attachment/ownership model"; rejected |

**E1 (entry check, read-only plus a host or kernel test before any ABI edit):** does input/kernel accept `PrepareForeground`/`CommitChild` from the controller for a child prepared by its foreground descendant (cellbox), with the child's reader nodes installed for that child? If yes, A needs **no** ABI change, only the request record over existing endpoint grants. If no, A needs a narrowly scoped ABI amendment ("controller commits a descendant-prepared child"), which will be drafted as its own annex before any edit. B, C and D are not pursued.

## 3. Rest of S5-3
1. **Proof consumer** (`cells/tests/tui-proof`, removed after evidence; plan step 6): linked only to `terminal-sdk` + `tui-cellos` (no cellbox, no viUI).
   - Draws a Ratatui list/table of real VFS entries (`/tmp`) and uptime.
   - Keys move the selection, `r` re-reads, and resize redraws.
   - `q` exits cleanly; it also gets killed in a separate scenario.
2. **Serial (desktop off, Phase02 serial runner)** — cellbox `spawn_foreground`, which exists today:
   - frames, arrow keys and resize via the session;
   - alternate screen enter/exit; prompt and echo recovery after `q`, Ctrl-C and kill (S5-2 soft reset).
3. **GUI (after option A):** the same scenarios through the real GUI Terminal, with exact-screen checks and 6 runs. Two independent Terminal windows run the consumer concurrently (plan step 7).
4. **Selective-TUI negatives (plan step 8):**
   - one-shot commands (`ls`, `ps`, `cat`) emit no alternate-screen or cursor-hide bytes and take no raw lease;
   - `tui-proof | cat` and `tui-proof > /tmp/x` detect a non-terminal stdout and exit nonzero with a message, never drawing or hanging;
   - `top` stays batch when piped. These are byte-exact stream checks.
5. **Bounds:** no new limits. The consumer uses SDK/emulator bounds.

## 4. Verification summary
Host tests for the request record (validation, stale epoch, wrong child, duplicate). Serial runner and GUI runner scenarios as above. Regression: terminal runner 368, GUI 197×6, Phase03 60, Phase02 48.

## 5. Decisions requested (recommendations first)
- **D53-1 GUI foreground design:** *A commit-on-behalf, gated by E1* (recommended), or B/C.
- **D53-2 If E1 fails:** *draft a narrow ABI annex for A* (recommended), or stop GUI TUI at serial-only for Phase 05.
- **D53-3 Consumer:** *throwaway `tui-proof` removed after evidence* (recommended; plan step 6), or keep as a maintained example.

## Results (2026-10-23, after option C)
- **Implemented:**
  - Throwaway `cells/tests/tui-proof` (terminal-sdk + tui-cellos + pinned Ratatui only): a `/tmp` list, uptime header, Up/Down, `r` re-read, resize, `q`/Ctrl-C restore, `--abandon`. It refuses non-terminal stdout with exit 2 and no escape bytes.
  - Private `cellbox → /bin/tui-proof` row under `phase04-driver-profile`; launch-profile digests re-pinned (`mod.rs` c932f3e0…, `profiles.rs` 6f01056d…, `tests.rs` e18483dd…).
  - Driver `tui.rs` (screen reconstructed from exact bytes) and a GUI `tui_proof()` scenario.
- **Unrequested cellbox change (fixes a real defect):** a lone `/bin/x > FILE` previously ran as a terminal foreground child, writing to the screen and leaving the file empty. It now runs as a one-stage byte pipeline into the file. `/bin/x > o 2> e` and `/bin/x < i > o` are refused before effects; `/bin/x 2> e` is unchanged. Covered by `external_byte_pipelines`.
- **Deviations:**
  - The pipe negative uses `| /bin/p03-filter cat`, because stock `/bin/cat` does not read stdin.
  - The "no lease" part of the one-shot negative is not observable by a frontend; only "no TUI bytes and no soft reset" is checked.
  - tui-proof lists `/tmp` through its cwd handle (the driver does `cd /tmp`).
  - tui-proof logs one UART line per frame, so the GUI driver can build the exact expected screen.
  - A `tui-reread` marker was added.
  - **No second GUI Terminal window.** Two concurrent sessions are proven only on the terminal runner (`tui-two-sessions`).
- **Verified (logs `reports/s5-3-evidence/`):**
  - Host: tui-proof 9/9, app-cellbox 94/94, launch_profile 29/29 (with and without the feature).
  - **Phase04 terminal runner 393/393 (113 markers)**: `tui-frame/keys/resize/reread/exit-restores/ctrl-c-130/abandon-recovered/two-sessions`, `oneshot-no-tui-bytes`, `tui-pipe-refused`, `top-pipe-batch`.
  - **GUI 6/6 at 252/252.**
  - Phase03 60/60; Phase02 48.
- **D53-3 (remove consumer after evidence):** evidence is archived. Removal is deferred until S5-4 so the TUI regression checks stay runnable while the emulator hardening lands; it is then removed together with its row, runner checks and digest pins.
