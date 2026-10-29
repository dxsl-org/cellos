# S3-gui archived reruns (2026-10-22) — requested by the user before CP2

Raw runner logs, serial logs and sanitized evidence are in this directory (no keys/sockets/images; work dirs deleted). Exit codes: `exit-codes-gui.txt`, `exit-codes-reg.txt`.

## Regression lanes (all PASS, raw logs archived)
Host suites (terminal, terminal-sdk, ostd, api, text-engine, app-cellbox): **450 passed, 0 failed** (`host-tests.log`). `test_cellos_sign.py`: OK (49). S3 private runner exit 0, 309 PASS lines, `P04-TERMINAL: PASS checks=52` (`s3/`). Phase03 exit 0, 60 PASS, CPU-halt proof `halt_proven=true`, samples [14,15] (`p03/`). Phase02 exit 0, 48 passes (`p02/`).

## GUI runner: 1 pass of 4 — the drain-order fix is NOT reliable
| Run | Launch | Result |
|---|---|---|
| gui-run1-click | click | **FAIL** at stderr iteration 04: unclassified screen |
| gui-run2-click | click | PASS 103 checks |
| gui-run3-click | click | **FAIL** at stderr iteration 02: unclassified screen |
| gui-run4-autoload | autoload | **FAIL** `no-window-before-launch` (runner expectation, see below) |

**Product defect, new form.** In both click failures the diagnostic is drawn on the *same row as the typed command*: `Cellos > /bin/ls /gui-missingls: /gui-missing: no such directory`, then a blank row, then the prompt (`screens/*FAILED-final.png`). Cause (PROBABLE, screens + source): the shell echoes Enter as `\n` on **stdout**; the child's diagnostic is on **stderr**; with the option-1 order (stderr first) the diagnostic overtakes that newline. This is the mirror of the original defect (prompt drawn over stderr when stdout was drained first), not a fix: the two endpoints carry no ordering, so either drain order races. Before the fix 1 of 2 evaluable runs showed the defect; after it 2 of 4 do. The earlier "10/10" was one lucky sample, as the CP2 dossier warned.

**Runner defect (autoload):** the autoload mode's `no-window-before-launch` check fails because in that mode the Terminal window legitimately exists from boot. Not a product failure; the check must be mode-aware.

## Consequence
The S3-gui CP2 is **not** ready: the GUI-4 stderr criterion is flaky and the claimed fix does not hold. Real fixes need ordering: either an ordering token in the endpoint ABI (kernel change) or a userspace single-stream tty mode in the shell for the GUI Terminal. CP2 stays pending.
