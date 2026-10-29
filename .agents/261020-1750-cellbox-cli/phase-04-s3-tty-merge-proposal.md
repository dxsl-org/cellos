# Phase 04 S3 — single-stream tty mode for the GUI Terminal shell (CP1 proposal)

Status: **APPROVED IN FULL by the user; implemented and privately verified on 2026-10-22.** Five consecutive click runs plus one autoload run all exit 0 with **103/103 checks and 10/10 strict stderr iterations each**. Archived host/signing/S3/P03/P02 logs pass as detailed in the [runtime report](reports/s3-gui-runtime-evidence.md). Supersedes the unreliable [drain-order option 1](phase-04-s3-terminal-drain-order-proposal.md); its [failed reruns](reports/s3-gui-rerun-evidence/README.md) remain. [GUI CP2 is PROPOSED, not approved](phase-04-s3-gui-cp2-dossier.md); rollout remains blocked.

## Problem (evidence)
The GUI-4 stderr gate passed 1 of 4 reruns after option 1. Failing screens show the diagnostic on the typed-command row (`... /gui-missingls: /gui-missing: no such directory`): stderr overtook the Enter newline the shell wrote on stdout. Before option 1 the mirror race drew the prompt over the diagnostic. stdout and stderr are separate rings with no ordering, so **any** fixed drain order races; a real terminal avoids this because a PTY carries one stream.

## Proposed design (userspace only, no ABI/kernel/ceiling change)
1. The Terminal launches the shell with the environment entry `CELLBOX_STREAMS=merged` (`TerminalHost::launch` already takes an environment list). The shell already reads its own context in `context_session.rs`, so the value is readable; **argv cannot carry the flag** because it is not readable under the shell ceiling (`cells/tools/cellbox/src/main.rs:125-126`).
2. In context mode with that entry, `executor.rs::terminal_write` (and its two direct stderr write sites at ~190 and ~2525) sends diagnostics to the stdout endpoint, in program order, from the single shell writer. Without the entry (serial, SSH-like and every fixture driver) behavior and the S3 stream separation proofs are unchanged.
3. The Terminal drain order option-1 edit is reverted to stdout-then-stderr (stderr stays empty in merged mode), restoring the pre-fix loop plus its test of the scripted host.

## What this does and does not order
Total order for everything the shell itself writes: typed echo, newline, shell diagnostics, forwarded child stderr relative to the shell's own prompt. **Not ordered:** a child that interleaves its own stdout and stderr inside one command (the shell reads the child's two rings separately); that needs the endpoint ordering token (kernel ABI) and stays a documented limit.

## Closure
`cells/apps/terminal/src/session.rs` (env entry + revert + tests), `cells/tools/cellbox/src/{context_session.rs,executor.rs}` (read entry, route diagnostics, host tests), `scripts/phase04_gui_drive.py` (autoload-mode `no-window-before-launch` check made mode-aware; stderr gate unchanged and strict), docs/evidence. No kernel, ABI, syscall ceiling, launch-profile, signing or compositor change.

## Acceptance and observed evidence
- [x] Host: exact committed `CELLBOX_STREAMS=merged` enables single-writer stdout routing for echo/newline/diagnostic/prompt; absent entry preserves distinct diagnostics stderr. Archived combined host suites pass **459 tests, 0 failed** (Terminal35, app-cellbox86, terminal-sdk49, ostd59+7, api128+2, text-engine74, doctests19).
- [x] GUI: **five consecutive complete click runs plus one autoload run**, all exit0, **103/103 checks each**, all **10/10** strict stderr-before-prompt exact screens. Autoload pre-launch checking is mode-aware; no failing screen was accepted. [Saved exit codes](reports/s3-gui-tty-evidence/exit-codes-gui.txt).
- [x] Archived S3 runner exit0/309 PASS lines including summary (driver52), Phase03 exit0/60 PASS with CPU-halt samples16/17, Phase02 exit0/48 passes; signing49 OK. Saved cellbox ceiling PASS; Terminal delta only preexisting approved SetTimer. Canonical hashes unchanged.

Supplementary parent observations: app-cellbox `--features applets` 88 pass; app-cellbox+Terminal bare-metal check clean (separate raw logs not saved in this archive). No kernel/ABI/ceiling/signing change was made by tty mode.

## Implemented behavior and limits

Terminal supplies the environment entry at shell launch and again drains stdout before stderr; `Streams` and scripted tests remain. `context_session::streams_merged` reads the exact value, executor records it and routes diagnostics through stdout **only in context merged mode**; non-GUI hosts without the entry retain separate streams. Routing stays fixed across partial-write retries. The GUI driver changes only the autoload-mode pre-launch expectation; the strict stderr gate remains unchanged.

This orders the shell writer's own echo/newline/diagnostic/prompt, including forwarded child stderr relative to that prompt, not a child's own interleaved stdout/stderr inside one command. The latter needs an ABI ordering token and remains unordered. TCG only; resize/focus/multi-window/KVM/physical Intel/VMware unexercised. Endless unconsumed-pipeline OOM and the historical completion-queue selftest failure remain disclosed. Full Phase04, canonical qualification and rollout stay blocked; this is not CP2 approval.

## Rollback
Revert the files above; no persisted state.

## Alternatives
A. Endpoint ordering token in the kernel ABI: full ordering, separate CP1, larger blast radius. B. Accept and document the race: leaves GUI-4 flaky. C. Do nothing (CP2 stays on hold).
