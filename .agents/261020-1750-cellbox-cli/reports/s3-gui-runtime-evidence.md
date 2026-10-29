# S3 GUI Terminal private runtime evidence

Date: 2026-10-22. **Current proof is the user-approved single-stream tty implementation: 6/6 complete private GUI runs (five click, one autoload), each 103/103 with ten strict stderr iterations, plus archived passing host/signing/S3/P03/P02 logs.** The original run1–4 history and the later failed drain-order reruns remain below and linked; fixed drain order alone is superseded. Real GUI rendering is proven on private QEMU/TCG fixtures only. [GUI CP2 is PROPOSED, not approved](../phase-04-s3-gui-cp2-dossier.md). Full Phase04, other CBX rows, S2b/d/e, S4, canonical qualification and rollout remain **BLOCKED**.

Authority: user approved [GUI-proof CP1](../phase-04-s3-gui-proof-proposal.md) in full, including the guarded private Terminal-signing exception; approved [drain-order option 1](../phase-04-s3-terminal-drain-order-proposal.md), subsequently superseded after failed reruns; then approved [single-stream tty CP1](../phase-04-s3-tty-merge-proposal.md) in full. Earlier [S3-output CP2](../phase-04-s3-output-cp2-dossier.md) remains approved at its recorded hashes; the changed executor and new GUI/signing/tty inventory require the separate proposed GUI CP2, not implicit extension.

## Initial desktop path and pixel oracle (run1–4 history)

New private runner `scripts/qemu-phase04-gui-test.sh` boots a disposable desktop image under q35/TCG, 256 MiB, `-nic none`, virtio keyboard/tablet and an owned QMP channel. The exercised launch mode is **a real taskbar click**, not autoload. QMP `input-send-event` injects virtio keyboard events through the guest input path. The compositor's framebuffer is observed via screendumps; no production Terminal grid-export hook is used.

`scripts/phase04_gui_drive.py` compares the Terminal region with a host renderer from `scripts/phase04-gui-glyphs/`, using the same `ostd::typography::TextFonts`, MonoRegular/14 px, glyph atlas, colors and grid geometry as the Terminal. The cursor cell is masked. Required text-region matches have **0 differing pixels**; negative controls include the pre-launch region, wrong prompt/text, changed glyphs, before/after output and incorrect `cc=0`. This is exact region evidence, not a claim that every pixel of the entire desktop or arbitrary terminal content has been qualified.

Observed commands: shell builtin `echo gui-proof-1`, stock `/bin/echo gui-proof-ext`, `/bin/ls /gui-list`, `/bin/cat /gui-text/note.txt`, and failing `/bin/ls /gui-missing`. The cat pane shows `gui-cat-line-one` and `gui-cat-line-two`; ls shows the fixture entries. Ctrl-C is exercised on `top --records --frames 100000 | to jsonl | /bin/p03-filter cat` with a consumer, not on an unconsumed endless pipeline. Its output visibly changes before interruption, then stops, the prompt returns and exact `cc=130` is rendered. Distinctive command data, typed command, shell prompt and status payloads are absent from UART according to the saved negative checks.

## Final and historical runs

| Run | Saved result | Meaning |
|---|---|---|
| [run1-stderr-after-prompt-defect](phase-04-gui-evidence/run1-stderr-after-prompt-defect/verdict.json) | 43/44 saved checks; `run-aborted` fails | Actual Terminal ordering defect: stderr drawn over the next prompt row; preserved, not accepted |
| [run2-runner-defect-ctrl-c-command-oom](phase-04-gui-evidence/run2-runner-defect-ctrl-c-command-oom/verdict.json) | 53/54 saved checks; `run-aborted` fails | Earlier Ctrl-C command/runner attempt encountered shell OOM on `top | to jsonl` without a consumer; unisolated and separate from the drain-order fix |
| [run3-pass](phase-04-gui-evidence/run3-pass/verdict.json) | **58/58 checks pass** | Initial complete GUI proof plus saved canonical hash comparisons; it did not erase run1's intermittent ordering defect |
| [run4-drain-order](phase-04-gui-evidence/run4-drain-order/verdict.json) | **103/103 checks pass** | Fixed Terminal, including **10/10 consecutive exact stderr-before-prompt screens**, Ctrl-C/status, UART negatives and canonical hash comparisons |

Run1 observed `Cellos > ls: /gui-missing: no such directory`, with prompt-row consumption; the [failed screenshot](phase-04-gui-evidence/run1-stderr-after-prompt-defect/screens/12-timeout-gui-4-stderr-output.png) remains. The defect occurred in one of the first three runs; this is not an estimated population failure rate. Run4 is **one ten-iteration sample**, not proof of a zero race rate or exhaustive concurrency qualification.

## Historical drain-order fix and remaining ordering limit (superseded)

Before the fix, `cells/apps/terminal/src/session.rs::drain()` read stdout then stderr. The shell emits the command diagnostic to stderr before its next stdout prompt; when both are pending, drawing stdout first can render the prompt ahead of the diagnostic. The separately approved option 1 now drains **stderr before stdout** via the private `Streams` test boundary. Two adjacent host tests cover diagnostic-before-prompt and empty/error propagation. The GUI driver strictly requires every one of ten missing-path commands to render the diagnostic fully before its next prompt; it does not accept a prompt-then-stderr alternative.

This preserves stdout/stderr endpoint separation; it **does not establish total ordering across endpoints**. A command's stdout-then-stderr pair, if both are pending in the same pump, can instead be rendered stderr-first. There is no new sequence token, merged stream, ABI change, shell change or compositor change in this fix. Parent-observed host results are Terminal **33 passed**, terminal-sdk **49 passed**; the bare-metal Terminal check was clean, and the ceiling comparison shows only the preexisting approved **SetTimer** addition, no new drain-order ceiling change. Raw host output was not archived; these counts are parent-observed, not independently replayable from this archive.

## Signing exception and immutable canonical artifacts

`scripts/cellos_sign/signing.py` defines `PRIVATE_FIXTURE_NAMES = {cellbox, app-cellbox, terminal}`. These names are admitted only with **`CELLBOX_PRIVATE_FIXTURE_SIGNING=1` and an explicit non-dev seed**. Default/no-seed and dev-seed signing remain refused. The exact quarantined Cellbox and Terminal hashes are refused **unconditionally**, including renamed copies, regardless of environment/seed. Whole-batch preflight remains before signer loading/mutation. `scripts/package-desktop-x86.py` and `scripts/x86/make-iso-ci.sh` canonical guards were not reopened.

Parent observed `scripts/test_cellos_sign.py`: **49 tests OK**, versus the earlier 41, including the Terminal private-signing cases and default/refusal behavior. Raw signing-test output was not archived; this is a parent-observed count only. Saved [before](phase-04-gui-evidence/run4-drain-order/canonical-before.txt)/[after](phase-04-gui-evidence/run4-drain-order/canonical-after.txt) hashes remain equal for the canonical/quarantined artifacts; no canonical packaging or replacement is claimed.

## Initial GUI-1..GUI-6 ledger (run4 snapshot; superseded by tty evidence below)

| Criterion | Evidence and exact scope |
|---|---|
| GUI-1 | Private desktop/compositor boot; [run4 provenance](phase-04-gui-evidence/run4-drain-order/provenance.json), [sanitized QEMU argv](phase-04-gui-evidence/run4-drain-order/qemu-argv.txt), unchanged saved canonical hashes |
| GUI-2 | Actual Terminal window contains exact `Cellos >` shell prompt; pre-launch/no-window and wrong-prompt controls differ; [prompt screenshot](phase-04-gui-evidence/run4-drain-order/screens/02-prompt.png) |
| GUI-3 | Injected typed lines and actual stock echo/ls/cat stdout render with exact matching regions; UART payload negatives pass; [stock cat screenshot](phase-04-gui-evidence/run4-drain-order/screens/10-GUI-3-stock-cat-after.png) |
| GUI-4 | Failing stock ls diagnostic renders before prompt in all ten iterations; `st=1` renders; flowing external pipeline interrupted, stopped and exact `cc=130` rendered; [tenth stderr screen](phase-04-gui-evidence/run4-drain-order/screens/30-GUI-4-stderr-10-after.png), [Ctrl-C status](phase-04-gui-evidence/run4-drain-order/screens/36-ctrl-c-status.png) |
| GUI-5 | Saved canonical hash comparisons unchanged; guarded exception implemented; **49 signing tests OK, parent-observed count only** |
| GUI-6 | Only the listed post-signing private runners/host suites below; parent observed passes, **no archived raw regression/host logs**; not broader system acceptance |

### GUI-6 post-signing regression observations

These are **parent-observed command results/counts only**. The owned `/tmp` regression directories and raw host-test output were not archived and have been deleted; do not substitute the older S3-output logs for these newer runs or invent their hashes/paths.

| Lane | Parent-observed outcome |
|---|---|
| S3 headless runner | exit 0; **309 PASS lines including the summary** (not 309 independent checks) |
| Phase03 runner | exit 0; **60 PASS**; CPU-halt probe `halt_proven=true`, matching samples **[15,16]**, RIP `0xffffffff80039571`, HLT=1, IF=0, CPL=0 |
| Phase02 runner | exit 0; **48 passes**, orphan ticks 4 |
| Terminal / terminal-sdk host suites | **33 / 49 passed** |
| Signing guard suite | **49 tests OK** |
| Terminal bare-metal / ceiling | check clean; only preexisting approved SetTimer delta |

The historical `COMPLETION-QUEUE` boot selftest failure remains disclosed; passing assigned gates is not a globally clean boot. The separate unconsumed endless-pipeline OOM remains unisolated/unfixed; no remediation is claimed by the GUI/drain-order work.

## Run4 provenance and archive limits

Run4: source HEAD `3471028bc8b4db14275553b4740c17e24fa4f32b` (worktree reference, not a clean-commit claim); kernel features `default+phase02-terminal-profile,phase03-filter-profile`; launch mode `click`, accelerator `tcg`. Signed Terminal SHA-256 `163fc929ef1bbbee18b5c81085d8b3ea7fa4483802cb79a19fc16610421ca669`; cellbox `08a67672f965aff6b213d2fbffbdce8a92f9494f636957a3e6b0b2534bee89bc`; kernel `1f6565d17999d97e0a3269e7a85a7cfef5ac07e115a2c1cb954cb44e4cfabe9b`; ISO `b9a98aacdfc69e6bcfff6cab718134dbf53ad91e3a386073287985476737211a`. Full cell/tool/trust-identity hashes are in provenance; no key bytes are included.

The private image has no `/POLICY.BIN` and uses the kernel's development-permissive posture, with signature verification at signing/spawn. It is not a production policy qualification. Original GUI run directories were deleted by their owner. The sanitized files already under `phase-04-gui-evidence/` are now the only saved source; **no original copy manifests were supplied**. [Archive hashes](phase-04-gui-evidence/archive-hashes.json) inventory the current saved bytes for integrity checking; they do **not** establish equivalence with unavailable originals. Existing PNG framebuffer screenshots are retained; no private keys/seeds, sockets, VM disk/ISO images, signed executables or build trees were copied by this documentation update.

## Initial remaining limits and rollout decision (historical snapshot)

Not exercised: GUI runner autoload launch mode, resize, focus changes, multi-window/tab behavior, KVM, physical Intel, VMware, arbitrary ANSI/VT/TUI behavior or exhaustive scheduling races. The taskbar-click launch, compositor rendering, injected keyboard, listed command bytes, stderr order and Ctrl-C cases are the demonstrated boundary.

The user's “duyệt rollout” is recorded, but the [containment release conditions](rollout-containment.md#release-conditions) are **not met**: other CBX/authority/implementation/dependency conditions, S2b/d/e, S4 and canonical packaging qualification remain open. No source guard was reopened for release, nothing was unblocked, and **rollout remains BLOCKED**. The signing exception is private-fixture-only, not release signing or quarantine removal. New GUI/signing/Terminal source files are listed in the CP2 dossier addendum, outside the previously approved S3-output CP2 baseline until explicitly accepted.

## Current tty-merge implementation and archived rerun evidence

The [failed option-1 reruns](s3-gui-rerun-evidence/README.md) passed only **1/4**: two click runs showed stderr on the typed-command row before the Enter newline; autoload failed the runner's incorrect no-window-before-launch expectation. Screens/raw logs remain. Either fixed drain order can race across unrelated endpoint rings; the initial clean run4 was not a reliable fix.

The user approved [single-stream tty mode](../phase-04-s3-tty-merge-proposal.md) in full. Terminal launches its shell with committed environment entry **`CELLBOX_STREAMS=merged`**. `context_session::streams_merged` reads the exact value; executor records it and `sink_for`/`route_terminal_write` send diagnostics to stdout only when **context mode and merged** are both true. Shell echo, Enter newline, diagnostics, forwarded child stderr and the following prompt are emitted by the shell's single writer in program order. The Terminal pump returns to **stdout-then-stderr**; the private `Streams` boundary and scripted tests remain. Without the entry, non-GUI/fixture hosts retain distinct stdout/stderr, preserving the S3 separation contract.

The GUI driver's autoload no-window check is now mode-aware; the strict ten-iteration stderr oracle is not weakened. No kernel, endpoint ABI, syscall ceiling, launch-profile, signing or compositor change belongs to this tty delta. A child's own interleaved stdout/stderr still traverses two child rings: **the tty fix does not establish that internal order**; an ABI ordering token would be required.

### Six complete GUI runs

All saved [exit codes](s3-gui-tty-evidence/exit-codes-gui.txt) are zero. Parent verified the archived files; each saved verdict has **103/103 checks**, including **10/10** strict exact stderr-before-prompt screens, stock output, Ctrl-C `cc=130`, UART negatives and unchanged saved canonical hashes.

| Consecutive run | Launch | Saved verdict |
|---|---|---|
| 1 | taskbar click | [103/103](s3-gui-tty-evidence/gui-run1-click/verdict.json) |
| 2 | taskbar click | [103/103](s3-gui-tty-evidence/gui-run2-click/verdict.json) |
| 3 | taskbar click | [103/103](s3-gui-tty-evidence/gui-run3-click/verdict.json) |
| 4 | taskbar click | [103/103](s3-gui-tty-evidence/gui-run4-click/verdict.json) |
| 5 | taskbar click | [103/103](s3-gui-tty-evidence/gui-run5-click/verdict.json) |
| 6 | autoload | [103/103](s3-gui-tty-evidence/gui-run6-autoload/verdict.json) |

This is **60 strict stderr iterations across six full runs**, not a zero-failure-rate or exhaustive concurrency guarantee. Autoload is now exercised once, superseding the former launch-mode gap. Resize/focus/multi-window/KVM/physical Intel/VMware remain unexercised.

### Archived host, signing and private regressions

The current [tty archive](s3-gui-tty-evidence/exit-codes-reg.txt) **does contain raw logs**, closing the earlier count-only gap for these newer runs without fabricating logs for the deleted initial runs.

| Lane | Saved result |
|---|---|
| Host | **459 passed, 0 failed**, 6 ignored: Terminal35, app-cellbox86, terminal-sdk49, ostd59+7, api128+2, text-engine74, doctests19; [raw output](s3-gui-tty-evidence/host-tests.log) |
| Signing | **49 tests OK**; [raw output](s3-gui-tty-evidence/sign-tests.log) |
| S3 | exit0; **309 PASS lines including summary** (driver52, not 309 independent checks); [runner](s3-gui-tty-evidence/s3.runner.log), [serial](s3-gui-tty-evidence/s3/serial.log) |
| Phase03 | exit0; **60 PASS**, child count4→4; [probe](s3-gui-tty-evidence/p03/final-probe.json) halt_proven=true, samples **[16,17]**, RIP `0xffffffff80039571`, HLT1/IF0/CPL0; [raw runner](s3-gui-tty-evidence/p03.runner.log) |
| Phase02 | exit0; **48 passes**, orphan ticks4; [raw runner](s3-gui-tty-evidence/p02.runner.log), [serial](s3-gui-tty-evidence/p02/serial.log) |
| Ceilings | Saved per-GUI-run cellbox ceiling PASS; Terminal only the previously approved SetTimer delta, no tty-mode ceiling change |
| Supplementary parent observations | app-cellbox `--features applets` **88 pass**, app-cellbox+Terminal bare-metal check clean; no separate raw logs for these supplementary checks in this archive |

The GUI-1..GUI-6 claim now relies on these repeat GUI and archived regression/host/signing results, within the same private-fixture boundary. The unchanged canonical hashes remain cellbox `64eb28f3be3d08bf2560d6eb0b58bee4a33ff8ea3439bcd5ed6e1ccc42f84a39`, Terminal `636497acc2bdf735471173c5c428e61d16b1d8c870935cfe868e6d0120e52fa2`, embedded image `447d0348735a25e8b396c11c0091ff3d214b0c0b392043bb0515d4104a298e2c`. Signed private provenance and before/after files accompany every GUI run.

[Saved-evidence hashes](s3-gui-tty-evidence/archive-hashes.json) inventory the existing sanitized tty archive. They establish integrity of the saved bytes, not original-to-copy equivalence with unavailable private workdirs. No key/seed file, socket, boot/disk/ISO image, signed executable or build tree is copied by this documentation update; existing PNG framebuffer evidence remains. The separate unconsumed endless-pipeline OOM and historical `COMPLETION-QUEUE` selftest failure remain unisolated/unfixed.

### Current approval boundary

The [fresh GUI CP2 dossier](../phase-04-s3-gui-cp2-dossier.md) is **PROPOSED**, no longer held for missing tty evidence but not approved. It inventories the nine current files, including context_session.rs and the executor changed since S3-output CP2. Shell_state.rs is unchanged at the earlier approved hash and is not a new scope item. No earlier CP2 is silently broadened. User rollout consent still does not satisfy containment release conditions: no release/canonical guard reopened, nothing unblocked, **rollout remains BLOCKED**.
