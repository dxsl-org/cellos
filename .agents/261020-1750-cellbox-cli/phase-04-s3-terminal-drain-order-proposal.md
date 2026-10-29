# Phase 04 S3 — GUI Terminal stderr/prompt ordering defect (CP1 proposal)

Status: **SUPERSEDED by user-approved [single-stream tty mode](phase-04-s3-tty-merge-proposal.md).** Option 1 was approved/implemented and initially passed run4 103/103, but [archived reruns](reports/s3-gui-rerun-evidence/README.md) passed only 1/4 and exposed the mirror race: stderr overtook the typed Enter newline. Fixed drain order is not a reliable ordering contract. Terminal now drains stdout then stderr again and requests merged tty routing in its shell. The original observations/results below are preserved as history, not current acceptance.

## Original failure evidence (preserved)
GUI run1 ([screenshot](reports/phase-04-gui-evidence/run1-stderr-after-prompt-defect/screens/12-timeout-gui-4-stderr-output.png)): `Cellos > ls: /gui-missing: no such directory` — the stderr line was drawn **over the next prompt row**, prompt consumed, cursor at column 0 below. Runs 2 and 3 drew the correct order (1 occurrence in 3 runs; true rate unknown).

Original causal finding: `cells/apps/terminal/src/session.rs::drain()` read stdout first, then stderr per pump. The shell writes the child's diagnostic to stderr before the next stdout prompt; with both rings pending, that drain order draws the prompt first. The fixed implementation now reads stderr before stdout; the original failing screen remains evidence. Separate endpoints still carry no total-order metadata, and no ABI sequence number was added.

## Options
1. **Terminal drains stderr before stdout each pump (recommended).** One-line order flip in `drain()` plus host tests with a scripted host. The shell always writes the prompt *after* the diagnostics of the command it just finished, so the prompt can no longer overtake them. Limit: a command that writes stdout then stderr within one pump window can have that pair swapped; this is the same unordered-pipes limitation in the other direction and is documented, not fixed.
2. **Shell forwards external stderr into its stdout stream in context mode.** Gives total order but destroys the stream separation that S3-output AC-2/AC-6 prove; rejected unless you want that trade.
3. **Add an ordering token to the endpoint ABI.** Correct but a kernel/ABI change needing its own CP1; out of scope here.
4. **Leave it and document.** The GUI-4 gate stays red 1-in-N runs.

## Closure for option 1
`cells/apps/terminal/src/session.rs` (the loop order and its comment + tests) and `scripts/phase04_gui_drive.py` only to add a deterministic repeat (N=10 stderr commands, all required to pass) so the fix is proven statistically, not by one lucky run. No kernel, ABI, ceiling, shell, compositor or signing change.

## Historical acceptance observations (superseded; not current proof of reliability)
- [x] Host test: scripted pending stderr is drawn before the following stdout prompt. Two added host tests also cover empty/error behavior; parent observed Terminal **33 pass**, terminal-sdk **49 pass** (raw host logs not archived).
- [x] GUI: run4 requires and passes **10/10 consecutive** missing-path commands with exact pixels showing stderr fully before the next prompt, without prompt-row consumption.
- [x] Parent observed the Terminal bare-metal check clean and only the preexisting approved SetTimer ceiling delta; no new drain-order syscall/capability/ABI change.

One ten-iteration sample is not a measured zero failure rate or exhaustive race proof. Stdout-then-stderr emitted within one pump window may render stderr-first: **total cross-endpoint ordering remains unsupported**. The fix preserves separate endpoints; it does not merge diagnostics into stdout. No claim is made to fix the separately observed unconsumed endless-pipeline OOM.

## Rollback
Revert the order flip and test; no persisted state.

## Subsequent resolution

The [tty-mode evidence](reports/s3-gui-runtime-evidence.md) records five click runs and one autoload run, all 103/103 with ten strict stderr iterations each. The shell now writes its echo/newline/diagnostic/prompt through one stdout writer when launched by the GUI Terminal with `CELLBOX_STREAMS=merged`; non-GUI hosts retain separate streams. A child's internal cross-stream interleaving is still unordered. The tty fix restores stdout-first draining while retaining the scripted `Streams` test boundary. [Current GUI CP2](phase-04-s3-gui-cp2-dossier.md) is proposed, not approved; rollout remains blocked.
