# T3 CP1 (rerun, "T3-D3"): does a Ctrl-C cancel of a guest `ssh --command` now end the connection and stop the producer? One diag boot, one full run, at most one confirmation boot (status AWAITING CP1 approval; documents only; nothing edited, built, fetched or run)

Date 2026-10-30. Author RerunCP1. HEAD `d42e70d6a60d7c010f99e2bb8e64f3435a8a0018` (`fix(net,kernel): reclaim sockets of a dead owner with RST (NET-OD)`). `git status --short` at writing: only `?? .agents/261030-1945-ginux-lessons-hardening/` (another plan, not touched by this program).

Authority chain this CP1 sits on, all read:

- [cp1-draft.md](cp1-draft.md) (SHA-256 `2f7ae851…dba8dc`, approved in [cp1-approval.md](cp1-approval.md)): section 5 forbids a rerun after a marker FAIL without a new CP1. This is that CP1.
- [cp1-diag-draft.md](cp1-diag-draft.md) (`cd27817a…e8d35f`, [cp1-diag-approval.md](cp1-diag-approval.md)): an unchanged T3-D rerun is not authorised; the diag stage is spent (`DIAG_CONSUMED-1`, `-2`).
- [cancel-diagnosis.md](cancel-diagnosis.md), [cp2-dossier-part3.md](cp2-dossier-part3.md) (T3-D, `RUN_CONSUMED-1`), [cp2-dossier-part4.md](cp2-dossier-part4.md) (T3-D2, outcome O1: the Net cell never reclaimed the dead owner's socket).
- [net-owner-death-cp1.md](net-owner-death-cp1.md) (`c40fdf98…224965`) section 15 = what this CP1 must contain; [net-owner-death-approval.md](net-owner-death-approval.md) (D-NOD-13 A: new CP1 after landing, 1 diag + 1 full); [net-od-cp2-dossier.md](net-od-cp2-dossier.md) (host proof only; **no QEMU/T3 observation was made**).

The approved ceiling statement is unchanged and is repeated verbatim in section 2.

## 0. Summary

| Item | Value |
|---|---|
| Question | After NET-OD, does the Net service abort the socket of a cancelled (killed) `cellbox-ssh` cell so that the guest sshd session ends and the `yes` producer stops, observed end to end at the SVM/TCG ceiling? |
| (a) Boot 1 | `--stage diag`: positive control + cancel scenario + host `ss` + slirp-netdev pcap + guest snapshots; new outcome label **O7** "reclaimed" is the only passing result; **STOP-7** if `[net] owner death watch unavailable; socket reclaim deferred` appears |
| (b) Boot 2 | `--stage full`: the T3-D run class, the full-stage judge expecting all **21** markers (including `ssh-guest-cancel-producer`) PASS exactly once, `P04-TERMINAL: PASS checks=21`; run only if boot 1 is O7 and its verdict passed |
| (c) Boot 3 (optional) | at most **one** confirmation boot of the stage that failed, only after a recorded offline lane/judge fix or for a host/environment error (section 3.4); never for a product outcome |
| Product edits | **none**. Lane-only edits: `scripts/qemu-tier3-ssh-guest-x86.sh`, `scripts/tier3_ssh_guest_judge.py`, `scripts/test_tier3_ssh_guest_judge.py`. No driver, guest-image or init edit is needed (section 5.3) |
| New tokens | `T3D3_DIAG_CONSUMED-1`, `T3D3_FULL_CONSUMED-1`, `T3D3_CONFIRM_CONSUMED-1` (the old `RUN_CONSUMED-*`/`DIAG_CONSUMED-*` tokens stay as records; the lane stops reading them) |
| Claims | CBX-23 guest half to "met at stated ceiling" **only if** diag = O7 **and** full passes **and** the S9-3 pipeline-exec / exact non-UTF-8 stdin limitation is dispositioned by the user (options in 8.2); CBX-25 already met, unchanged; nothing about promoted, canonical or matched-candidate bytes |
| Network | none (the apk set and QEMU 10.2.0 are cached and pinned; both re-verified read-only today) |
| F1 gate | green today: `python3 scripts/cellos-sign --check` -> `OK: F5`, `OK: F1 - 134 crates and 928 files scanned; unsafe confined to 67 allowlisted files` (section 11) |
| Decisions | D-RR-1..11 in section 15; recommendations in the last column |

## 1. Why a new CP1, and what is and is not known

Known and observed (T3-D2, boot 2, `evidence/t3-d2/`): after Ctrl-C the cell never sent FIN or RST on flow A, kept ACKing slirp's persist probes with window 0, the guest connection stayed `ESTABLISHED` at +3/+25/+106 s, `yes` stayed alive; the cooperative control (`--timeout`) stopped the producer within a millisecond-scale FIN path. Cause by code reading (not observation in the Net cell): the kernel granted owner-death watches only to the VFS cell, and `release_owner` was unreachable (`[net] owner death watch unavailable; socket reclaim deferred`, `serial.log:314`).

NET-OD (commit `d42e70d6a`) changes the kernel grant (registry-bound Active NET provider), `TryRecv` mask 0 delivery of an owner death, and the Net reclaim (RST for TCP sockets not in the graceful ledger, drain ledger with a 1 s deadline, one log line `[net] owner death: cell C gen G: aborted N TCP, released M other` only when something was reclaimed). It is proven **on the host** only: kernel 295 tests, `service-net` 65 tests including a smoltcp-over-Loopback proof that `SocketSet::remove` alone leaves the peer `Established` and the new abort resets it, builds for the three bare-metal targets (net-od-cp2-dossier sections 1, 3, 4, 6).

**Not known** (this CP1 exists to observe it): whether the kernel's grant predicate holds under the real boot-time registry state (it was proven with a host fixture that registers NET and clears it after each call, dossier deviation 8); the wake latency of a queued death on `sys_wait_completion(NET_RX, 10)` (reasoned as at most one 100 ms turn); what the real slirp/hostfwd path and the guest sshd do with an RST on flow A; `VirtioNetDevice`/e1000 TX behaviour for the RST. These are observed, not assumed, by boot 1.

### 1.1 Budget already spent (and why a rerun is not authorised by NET-OD)

| Token under `evidence/t3-d-runs/` | Content |
|---|---|
| `RUN_CONSUMED-1` | `stage=full run=1 head=ac03f419… window=7200` (T3-D, MARKER-FAIL on the cancel marker) |
| `DIAG_CONSUMED-1`, `DIAG_CONSUMED-2` | `stage=diag`, head `066cb356e…` (boot 1 O6 = my lane defect, boot 2 O1) |
| `RUN_CONSUMED-2` | free, and left unused: the old budget rules (host/environment error only) are closed; this CP1 gives the rerun its own tokens (D-RR-3) |

NET-OD's CP1 section 15 and D-NOD-13 A state the same: the rerun needs this separate approval.

## 2. Ceiling statement (verbatim from the approved CP1; to be repeated in every record these runs produce)

> **Ceiling reduction (user disposition 2026-10-30, S10-11; same kind as DS6-1 B and D10-1 C).** The CBX-23 guest half and the CBX-25 VM route/server/credentials proof are made, if at all, against a Linux guest running OpenSSH on the **existing AMD SVM VMM** under **QEMU-TCG 10.2.0** (`qemu64,+svm`), reached from a Cellos cell by **one loopback-only QEMU `hostfwd`**, with a **password** credential, in a **private lane-only image**. This is **not** Intel VMX/EPT Tier-3 and **not** the P09 outcome; the plans `260711-1917-tier3b-x86-vtx`, `260712-0952-tier3b-vm-hardening-compat` and `260722-2330-tier3b-finish-g5-lite` and relations L11/L13/L18 are unchanged. Not claimed: KVM, physical x86, production, remote or C2C use, a guest bridge or guest-lifecycle authority, virtio-rng, publickey to the guest, a matched-candidate image (the hypervisor cell, the fixture kernel and the guest are lane-only bytes), any inbound Cellos sshd, any guarantee about HEAD beyond the one retained run.

Added for this rerun (each record repeats them): the image is a fresh lane-only build at the HEAD that the token records, with the NET-OD kernel and Net bytes; **not** the canonical/promoted bytes and **not** the matched candidate `4fae8b20…` (D-NOD-12 A: re-promotion is separate and not requested here); the TCP close observed is an **RST** (abort), not an orderly FIN (D-NOD-3 A, with its cost: unread or unacknowledged data of a killed owner is discarded).

## 3. What is requested

### 3.1 The runs

| # | Stage | Token (O_EXCL, claimed just before QEMU starts, after a successful build) | Runs only if | Evidence |
|---|---|---|---|---|
| 1 | `--stage diag` | `T3D3_DIAG_CONSUMED-1` | Gate A (section 5.5) passed | `evidence/t3-d3/diag/` |
| 2 | `--stage full` | `T3D3_FULL_CONSUMED-1` | boot 1 verdict exit 0 **and** outcome O7 **and** no tree/pin change since boot 1 | `evidence/t3-d3/full/` |
| 3 | the failed stage, once | `T3D3_CONFIRM_CONSUMED-1`, needs `TIER3_T3D3_CONFIRM=1` | only the conditions of 3.4 | `evidence/t3-d3/confirm-<stage>/` |

A diag outcome other than O7, STOP-7, or a full-run marker FAIL is a **product result**: it is recorded and the program for this row stops; it is not a reason for boot 3 and not a reason for any further run without a new CP1 and a new diagnosis (section 7; same rule as cp1-draft section 5, K8, S10-4.17; a second FAIL ends the program for this row).

Hard maximum: **3 QEMU boots**. Cargo builds: `--locked --offline`, one per boot plus the `--build-only` gates; a build-only run consumes nothing.

### 3.2 Windows

Diag: the lane's own rule (window = 3 x time-to-listening + 20 min, never above 3600 s; observed 165 s). Full: `TIER3_BOOT_WINDOW=3600` set by the operator (lane default 7200, lane maximum 14400 unchanged; observed 127 s). A run that reaches its window ends as a failed gate (STOP-2 style), never stretched (D-RR-6).

### 3.3 What the runs do not change

The reused 16 scenarios, the three guest scenarios, the guest image recipe, the sshd configuration, the guest `p9` helper, the QEMU argv invariant (one user netdev, one loopback `hostfwd`, `-m 1G`, TCG, no KVM, pinned QEMU `169fb4ea…1241fe`; diag adds exactly the one `-object filter-dump`) and the cancel scenario itself (Ctrl-C 500 ms after `p09-started`, then `p9 gone`) are unchanged. The scenario must not be made easier (no longer wait, no lane-side kill, no helper that kills `yes`).

### 3.4 When boot 3 (confirmation) is allowed

Allowed (one time in total, either stage, with `TIER3_T3D3_CONFIRM=1`), and only after a recorded offline fix in `evidence/t3-d3/lane-fix-record.txt` (what the failed boot showed, the raw data, the changed lines, the tests added):

1. a host/environment error before the driver ran: tooling (`ss`, pcap file), QEMU crash, hypervisor failure at start, a pre-scenario B1/B2/C1/C2 failure that the data attributes to the host (not to NET-OD); or
2. a **lane/judge/instrument defect** where the failed boot's raw product data (pcap, guest snapshots, host `ss`, serial) already satisfy the O7 criteria as written, and the judge or lane misread them (the T3-D2 boot-1 pattern: the control criterion and the pcap clock).

Not allowed: any O1..O4/O6 outcome whose raw data really shows a live connection, a surviving producer, a missing RST, a late reclaim, or a missing `[net] owner death:` line; STOP-7; any marker FAIL in the full run; STOP-D2/STOP-5 (a secret); a protected-path or pin change; a fix that **relaxes** an O7/full criterion that the failed boot failed on product-behaviour data. The failed boot stays recorded as failed and is not claimed; only the confirmation boot's own evidence is judged.

## 4. Pass criteria

### 4.1 Diag (outcome O7, "reclaimed") - the only passing outcome

The new classifier branch is checked **before** O1. All of the following hold, from the stage's own evidence:

| # | Criterion | Source (existing key or new key, section 5.2) |
|---|---|---|
| 1 | gates B1/B2/C1/C2 true, argv invariant, no secret (including the raw pcap), protected digests before = after, window within 3600 s | existing |
| 2 | positive control PASS: client `st=124 ... outcome=timeout`, guest logged the close, producer started and gone at snap-a | `control_ok`, `control_closed_logged`, `control_producer_started`, `control_producer_stopped_at_snap_a` (existing) |
| 3 | cancel marker `ssh-guest-cancel-producer` **PASS** (client `st=130 outcome=cancelled`, `p9 gone` = `p09-gone`) | `cancel_marker_pass` (existing) |
| 4 | **RST from the cell** on the cancelled connection's flow A: at least one RST, **no FIN**, from `10.0.2.15` | new `cancel_A_rst_from_cell` true, `cancel_A_fin_from_cell` false |
| 5 | no zero-window ACK from the cell **after its first RST** (a still-alive socket would keep answering probes) | new `cancel_A_zero_window_from_cell_after_first_rst == 0` |
| 6 | the end propagated: FIN or RST from slirp towards the guest on flow B | `slirp_fin_rst_to_guest_after_cancel` true (existing, key `cancel_B_fin_rst_from_slirp`) |
| 7 | guest `/proc/net/tcp`: the cancelled connection is **not `ESTABLISHED` and not `CLOSE_WAIT` at any of snap-a/b/c** and is **absent at snap-c**; guest `yes` (the pid in the pid file) **not alive at snap-a, b, c** | `cancel_conn_state_at_snap_*`, `yes_alive_at_snap_*` (existing) |
| 8 | host `ss`: `ESTAB` count 0 at snap-a, snap-b and snap-c (SA and SB gone) | `host_ss_established_at_*` (existing) |
| 9 | serial (host lines): at least one `[net] owner death: cell C gen G: aborted N TCP, released M other` with **N >= 1** between the cancel scenario's `P04 INFO p09-case ssh-guest-cancel-producer` line and its `P04 PASS/FAIL ssh-guest-cancel-producer` line, and **no** `owner death watch unavailable` anywhere | new `net_owner_death_in_cancel_window`, `net_watch_unavailable` |

No specific sshd wording is required (`Connection closed by` vs `Connection reset by`): the guest connection having ended is what row 7 requires. Row 9 is corroboration, not attribution: the log line carries a cell number and generation but the judge cannot tell from it alone that it is the cancelled client (other exiting cells may also log one); attribution rests on rows 3-8.

`--expect-outcome O7` makes the verdict fail (`MARKER-FAIL`) for any other label; O1..O6 stay as labels for the historical data and for a failed boot.

An outcome **O4** (the cancelled connection alive at snap-a, gone later) under the fix is a *finding*: the reclaim exists but is not prompt (the design predicts at most a 100 ms turn plus RST drain); it is recorded and not a pass of this CP1.

### 4.2 Full (the "full run class")

The existing full-stage judge, plus the two Net checks of 5.2 (J3):

- every one of the **21** expected markers PASS exactly once: `shell-prompt`, the 16 reused (`ssh-conf` .. `shell-long-line`), `ssh-guest-cancel-producer`, `ssh-guest-fault-may-have-run`, `ssh-guest-no-replay-ledger`, `session-a-close` (`EXPECTED_MARKERS`, judge `:54`: 1 + 16 + 3 + 1 = 21; unchanged), zero `P04 FAIL`, no unexpected marker, `P04-TERMINAL: PASS checks=21` exactly once;
- B1/B2/C1/C2 re-proved at the start, argv invariant (no `-object`), no secret anywhere, protected digests before = after, guest sshd accounting as before (6 accepted + 1 failed password before the guest scenarios; accepted logins after in `[4, logins]`; one session per connection; exactly one `p09x-selfkill` ledger line);
- `[net] owner death: ... aborted N TCP` with N >= 1 in the cancel scenario's window and no `owner death watch unavailable` (J3).

"The full run class passes" means exactly: diag O7 with exit 0 **and** this full verdict exit 0, both without STOP.

## 5. Exact edits (lane-only) and gates

The lane files are lane-only: inside the lane's own allowed-dirty list (`qemu-tier3-ssh-guest-x86.sh:108`), no `kernel/`, `hal/`, `libs/`, `cells/` file. Pre-edit digests are in section 6.1 (all re-taken at HEAD `d42e70d6a`).

### 5.1 `scripts/qemu-tier3-ssh-guest-x86.sh` (483 lines, pre-edit `7c0e3d57…e0a74`)

| # | Edit |
|---|---|
| L1 | Header comment (`:14-34`): describe T3-D3 (tokens, evidence dirs, `--claim-only`, `TIER3_T3D3_CONFIRM`). |
| L2 | `OUT_NAME` (`:67`): `full -> t3-d3/full`, `diag -> t3-d3/diag`, `boot -> t3-bc` (unchanged); with `TIER3_T3D3_CONFIRM=1`: `t3-d3/confirm-$STAGE`. New guard after `OUT` is computed: if `$OUT` exists and is non-empty, `pfail` (T3-D `evidence/t3-d/` and T3-D2 `evidence/t3-d2/` must never be overwritten or mixed; `cmd_archive` copies into `$OUT`). |
| L3 | Token logic (`:285-299`) as a function `claim_token`: `TOKEN_DIR` from `TIER3_TOKEN_DIR` (default `$ROOT/.agents/261110-0900-tier3-guest-endpoint/evidence/t3-d-runs`); stage `diag` -> `T3D3_DIAG_CONSUMED-1`, `full` -> `T3D3_FULL_CONSUMED-1`; if that file exists, claim `T3D3_CONFIRM_CONSUMED-1` **only** when `TIER3_T3D3_CONFIRM=1`, else `pfail`; if the confirm token exists too, `pfail`. `noclobber` O_EXCL as now. Token content: `stage= run= head= window= lane_sha256= judge_sha256=`. The old `RUN_CONSUMED`/`DIAG_CONSUMED` names are no longer read. New flag **`--claim-only`**: run the preflight, call `claim_token` and exit; refused unless `TIER3_TOKEN_DIR` is set to a path under `/tmp` (so Gate A can test the token rules against a scratch directory without touching the real one). In the normal flow the claim stays after the build and just before QEMU, so a build failure consumes nothing. |
| L4 | Preflight (after `:128`): `sha256sum -c` of an inline pin list of the product bytes of section 6.2 (any difference is `pfail` "product bytes differ from rerun-cp1.md section 6.2"; the dirty-tree check alone does not catch a different commit). Also `pfail` if `scripts/cellos-sign --check` is not rc 0 (the F1/F5 precondition of the signing step, so a red gate fails before any key is generated). |
| L5 | Supervisor (`:365`): for stages `full` and `diag` only, a new stop pattern for host lines (not `[guest-uart]`), `owner death watch unavailable`, sets `verdict = "stop-7-watch-unavailable"`, breaks, terminates QEMU, and the usual `finally` evidence is still written. (The line is printed early, at the first refused watch; observed at serial line 314 before any guest work.) |
| L6 | `provenance.sha256` list (`:273`): add `"$REL/service-net"` so the Net cell bytes of the image are recorded (today only cellbox, hypervisor, driver, kernel, ISO, `kernel_fs.img`, initramfs). |
| L7 | Judge call (`:475`): for `diag` add `--expect-outcome O7`. |
| L8 | `provenance-build.txt` text (`:276`): add "NET-OD kernel and service-net bytes (HEAD `<rev>`); not the canonical or promoted bytes". |

No change to `DIRTY`/allowed list, QEMU argv, windows (`TIER3_BOOT_WINDOW` is an environment value), key generation, secrets handling or cleanup.

### 5.2 `scripts/tier3_ssh_guest_judge.py` (861 lines, pre-edit `ff723b74…8f6a`)

| # | Edit |
|---|---|
| J1 | `summarize_pcap` (`:370-484`): per connection track `zero_win_after_first_rst[d]` (zero-window pure ACKs in direction `d` after the first RST packet of that connection; RST/FIN/SYN segments never counted, as today's `zero_win`). New `derived` keys (additive): `cancel_A_rst_from_cell` (`rst.c2s >= 1`), `cancel_A_fin_from_cell` (`fin.c2s >= 1`), `cancel_A_zero_window_from_cell_after_first_rst`, and the same three for the control pair (data only). |
| J2 | `OUTCOMES["O7"]` = "reclaimed: the Net service aborted the dead owner's socket (RST), the guest connection ended, the producer stopped" and a branch at the **top** of `classify` (`:526`) requiring rows 2-9 of section 4.1 (using `d.get` so the existing O1..O6 fixtures that lack the new keys can never be O7). |
| J3 | New checks for stages `full` and `diag` (not `boot`): `net:no-owner-death-watch-unavailable` (no host line contains `owner death watch unavailable`) and `net:owner-death-reclaim-logged-in-cancel-window` (a host line matching `\[net\] owner death: cell \d+ gen \d+: aborted (\d+) TCP, released \d+ other` with N >= 1 between the cancel scenario's `p09-case` line and its PASS/FAIL line). The verdict records `net.owner_death_lines` (serial line number, cell, gen, aborted, released) and `net.watch_unavailable`. Stop chain (`:751-770`): `STOP-7` is evaluated **after** the argv check and **before** the gate classification (the line appears before the guest is up, so gates would otherwise mask it) and before `MARKER-FAIL`; for diag the label stays STOP-7 (not a D-number). |
| J4 | `--expect-outcome LABEL` (diag only): adds `diag:outcome-is-<LABEL>`; default none, so the historical diag tests are unaffected. |
| J5 | `diag_checks` (`:548-622`): fill the new `d[...]` keys (`cancel_A_rst_from_cell`, `cancel_A_fin_from_cell`, `cancel_A_zero_window_from_cell_after_first_rst`, `net_owner_death_in_cancel_window`, `net_watch_unavailable`); nothing else in the diag checks changes. |
| J6 | Module docstring (`:1-31`): O7, STOP-7, `--expect-outcome`. |

`EXPECTED_MARKERS` (21), `GUEST_MARKERS`, the full-stage final-verdict rule and `ARCHIVE_FILES` are **not** changed (the full stage already requires `ssh-guest-cancel-producer` as a PASS marker); the tests of 5.4 pin that.

### 5.3 Driver and guest image: no edit

`cells/tests/phase04-terminal-driver/src/ssh/guest.rs`, `src/ssh.rs`, `Cargo.toml`, `scripts/prepare-tier3-sshd-initramfs-x86.sh`, `tests/guests/x86-sshd/guest-init.sh` stay byte-identical (digests in 6.3). Checked: `guest_checks` has no diff between the T3-D commit `3af9a6516` and HEAD except a `cfg_attr(guest-diag, allow(dead_code))` attribute; `SSH_MS` is 300 s in both. `diag_checks` and `snap` (snap-a at +3 s, snap-b at +25 s, `p9 gone`, snap-c at +70 s) were never run against a reclaimed connection [INFERENCE that they work unchanged]; with a reclaimed connection `p9 gone` should return on the first attempt, which puts the guest login count exactly at the minimum the judge accepts (9 for diag, 4 for full; `D:guest-scenario-logins-accounted` bounds, `judge:186`) [INFERENCE].

### 5.4 `scripts/test_tier3_ssh_guest_judge.py` (823 lines, pre-edit `0831939e…dff`): tests (69 pass today; all stay green)

1. `full_serial(...)` and `DiagRun.serial()` gain a default `[net] owner death: cell 7 gen 1: aborted 1 TCP, released 0 other` line inside the cancel window and knobs `reclaim=`, `watch_unavailable=`; `DiagRun` gains `cancel_rst_cell` (RST instead of FIN/none, via the existing `conn_pkts(rst_cell=...)`) and `zero_after_rst`.
2. `Classify`: O7 positive; a table that flips each of rows 2-9 of 4.1 alone and must give a non-O7 label (including the new keys missing, `None` and `False`); O1 and O4 expectations unchanged.
3. Pcap: single RST from the cell sets `cancel_A_rst_from_cell` and not `cancel_A_fin_from_cell`; a zero-window ACK **before** the RST is not counted, one **after** it is counted (value 1); FIN + RST sets both; control-pair keys.
4. `DiagJudge`: end-to-end O7 run passes, `stop_rule` None, `outcome == "O7"`, `--expect-outcome O7` check passes; the same options against an O1 run -> `diag:outcome-is-O7` fails and stop `MARKER-FAIL`; an O4-shaped run -> not O7.
5. STOP-7: a `owner death watch unavailable` host line in a diag run and in a full run -> stop `STOP-7`, even when every other check passes and even when the gates are incomplete; the same text on a `[guest-uart]` line does not trigger it.
6. Reclaim-line rules (both stages): missing line, `aborted 0 TCP`, line **before** the cancel `p09-case` line, line **after** the cancel PASS/FAIL line -> check fails.
7. Full stage: `len(J.EXPECTED_MARKERS) == 21` and the cancel marker in it; a full serial with `P04 FAIL ssh-guest-cancel-producer` -> `MARKER-FAIL` naming the marker; a missing cancel marker -> `MARKER-FAIL`; `PASS checks=21` required; a clean full run with the reclaim line passes.
8. CLI: `--expect-outcome` parsing; `verdict.json` carries `net.owner_death_lines`; the existing end-to-end archive tests unchanged.

### 5.5 Gate A (offline, before boot 1; and again, minus what did not change, before boot 2 and boot 3)

1. `bash -n` on the lane; `python3 -m py_compile` on judge and tests; `python3 -m unittest scripts.test_tier3_ssh_guest_judge` (69 + the new tests, all pass; the count is recorded).
2. Token and guard tests against a scratch directory: `TIER3_TOKEN_DIR=$(mktemp -d /tmp/t3d3-tok.XXXX)` with `--claim-only`: diag claims `T3D3_DIAG_CONSUMED-1`, a second diag refuses, the same with `TIER3_T3D3_CONFIRM=1` claims the confirm token once and a third refuses; full likewise; `--claim-only` without `TIER3_TOKEN_DIR` refuses; an existing non-empty `$OUT` refuses. The real token directory is not touched.
3. `sha256sum -c apk-pins.sha256` in `$W/apk-fetch` (**re-run today read-only: OK**), the product pin list of 6.2, the cached Alpine kernel/initramfs pins and the QEMU binary hash (re-taken today: equal to the pins).
4. `python3 scripts/cellos-sign --check` rc 0 (F1/F5; section 11).
5. `--stage diag --build-only` and `--stage full --build-only` (cargo `--locked --offline`; `Cargo.lock` unchanged; no QEMU, no token): both `BUILD_ONLY_OK`; protected digests before = after; the read-only audit of the new cpio (`initramfs-audit.json`, `problems: []`, only `etc/t3-host_ed25519` and `etc/t3-password` secret, mode 0600); `fat-layout.txt` shows no `sshd`.
6. A recorded read-only check that `provenance.sha256` of each build-only run lists the kernel, ISO, `kernel_fs.img`, the driver, the hypervisor and (after L6) `service-net`, and that the kernel and ISO digests differ from `evidence/t3-d2/provenance.sha256` (the NET-OD bytes are in the image; the earlier provenance has no `service-net` line, so the Net cell cannot be compared directly).

Gate A failure of anything but a lane/judge/test defect is a STOP-8 (section 7); a lane/judge/test defect found by Gate A is fixed before boot 1 and costs no boot.

## 6. Closure (all SHA-256 at HEAD `d42e70d6a`, taken 2026-10-30; anything else under `git status` at execution time is a STOP)

### 6.1 Existing files to edit (lane-only; hash = pre-edit digest)

| Path | SHA-256 |
|---|---|
| `scripts/qemu-tier3-ssh-guest-x86.sh` | `7c0e3d570f80c03276a5a61550e1d39db6983e4e3ced6dd13041e6740e7a0e74` |
| `scripts/tier3_ssh_guest_judge.py` | `ff723b7485737657ee957d87402376c81bd7711226d7053575a54bc032b48f6a` |
| `scripts/test_tier3_ssh_guest_judge.py` | `0831939e219f1172b1b6fd7f200b22564ef38ac51ff13f188aefc1a8c8a99dff` |

These equal the post-edit digests recorded in cp2-dossier-part4 section 2 (no drift since the T3-D2 commit).

### 6.2 Product bytes of the image: pinned read-only (the lane's preflight checks this list, L4)

| Path | SHA-256 |
|---|---|
| `kernel/src/task/scheduler.rs` | `e49d5fea5559275dff32dbdb2469d2d1ff190462cd94aaa30d63923908c711d2` |
| `kernel/src/task/syscall.rs` | `c00a71b14fc774e7d961cc302930812c59449ba70932b7e17c1f935148e13b85` |
| `kernel/src/task/tcb.rs` | `3e8880236286f30b33afa8a72291eb7fefa58fe5b8c7a4f0fd38aba7127cdf61` |
| `cells/services/net/src/socket_table.rs` | `1964bc9d0ee14d9db1fd6d60715c65d4c0cfda1edda01c2999705903ff456ecb` |
| `cells/services/net/src/socket_table/tests.rs` | `28529be349558c9829d0b76dcb459fef5223efdb7b6d25d06741a1a62c20a6ef` |
| `cells/services/net/src/service-runtime.rs` | `89d9334a01d047c93e3ff97352b7a99a460d9093d4a9dc9ac8d97044e7db811e` |
| `cells/services/net/src/service-runtime-owner-death-tests.rs` | `3c12780f690adb5a1629a1ccc0fdcab9515035e66e3a917c6cc369343de807a5` |
| `cells/services/net/src/main.rs` | `76c486d06511d0ab72c567c2318b05326c88a2e795abdcc917a4116967766ced` |
| `libs/ostd/src/clients/net.rs` | `2344128b467f292f42d4dc4be6a2b45f56c62c4b6983dbca7be34bf156de54b4` |
| `cells/tools/cellbox/src/ssh.rs` | `b71ebc459312c34e4282c93bc5140206b43033181b68fe2fb5414929ec170401` |
| `Cargo.lock` | `5181251ccc712f27f71db9861b1b4feab0a8fec910e43716a7b744a4f8c81130` |
| `kernel/src/main.rs` | `a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64` |
| `kernel/Cargo.toml` | `b71f1ba41c72b36b6295da3c10aef73479ccfc5ec9b2a53b40079cfa80669f27` |
| `kernel/build.rs` | `c46adb3053a91e3513f525017f4cb69601a87c882f2e8195951c09a0518cc22b` |
| `kernel/src/loader/launch_profile/targets.rs` | `45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f` |
| `kernel/src/hypervisor/svm_registry.rs` | `e887e6acfa4e50dc049db9124866a9bee7f0938686725d31a44f89ccf323a44c` |
| `hal/arch/x86/src/x86_64/vmx.rs` | `f4a0301c6ba4710c90c752b562c4b8d8878709e031fbd134c51e44fcc7b92b2b` |
| `cells/services/hypervisor/src/boot_x86.rs` | `0c89aae7748fa7fa464b75686258ae9ca3fd3a94ef1debee15e44f73bbe284dd` |

The first seven equal the NET-OD CP2 "after" digests (net-od-cp2-dossier section 2). The protected-path tree digests (`kernel`, `hal`, `libs`, `cells/services`, `cells/tools`, `config`, `scripts/cellos_sign`, `scripts/x86`, `tools`, `Cargo.lock`) differ from the T3-D / T3-D2 baselines **by design**: `git diff --name-only 066cb356e HEAD` outside `.agents/` is exactly `CHANGELOG.md`, `docs/network-api.md`, the 7 NET-OD code files (3 kernel, 4 Net) and the lane/driver/guest-recipe files committed with the T3-D2 work (`guest.rs`, `ssh.rs`, driver `Cargo.toml`, the prepare script, the lane, the judge, its tests). The lane compares before/after **within** a run (`protected-before.txt` of boot 1 is the new recorded baseline).

### 6.3 Reused unchanged (digest-pinned; any change is a STOP)

Guest recipe and driver (no edit, 5.3): `scripts/prepare-tier3-sshd-initramfs-x86.sh` `e95c327ca6e67bd4b3d0dd187bd3e16daaa1f069cb79c82ef4d21e893d2017d4`; `cells/tests/phase04-terminal-driver/src/ssh/guest.rs` `dc4ce10454932f532273599c8b452209e23f6141c0856b935d32ab223388a727`; `…/src/ssh.rs` `756dee495eb760004914a03c821681d51c34224670e1bbcab02c0e6a204afdd4`; `…/Cargo.toml` `b959b11a6c270a59c5f01bcb54487bc5618fd99fdb43a25df5bde41dacc3706d`; `…/src/main.rs` `b661277daadeadd43904302a31bd9546028b6717bb2bbecd03f03574bdf16ca3`; `…/src/ssh/kssh.rs` `7474e8b9761c62b24c0499050b7d2e0bb1d5cc6e0bfc37bd9ad09ef6e3288a50`; `tests/guests/x86-sshd/guest-init.sh` `6ff396d99dbd71d41b7e1e9b67f03f0774f60af6398dc648a390f24e3f53def0`; `tests/guests/x86-sshd/apk-pins.sha256` `4edae50a39fe265492f1c22ee4d312ad83c3faf9e6ad925719573ee408db8870`; `scripts/fetch-tier3-sshd-apks-x86.sh` `80e02e105f228e7b07c84c0ef534ca19159431b9ff97f0f19767f4cdfbdaf841` (never runs).

Templates and tooling: `scripts/qemu-phase09-ssh-test.sh` `620b735d404037b79c883fde5272e0f1c3cd11c4432ac786003f031ccc071dc2`; `scripts/phase09_trust_disk.py` `10901b916c948efd62b632e2144e1a3ab519ce8487d324f87e42c60390e6e5e1`; `scripts/phase09_ssh_fixture.sh` `07c6747139a41edc686c96c1d3f2763b5092dcc5eece8ec724970526e5eb8b50` (reference only); `scripts/kssh_boots.py` `651ebadb019a3dc31f634cbff2c2efa6cf0fb9eea544545a4b71781b2a6b6388`; `scripts/kssh_judge.py` `8f2e06e0eb506040d306cb45c7204178eaeffb287b1994890e8a4d8b2d0dd732`; `tools/repack-initramfs.py` `dd9cd07b51afc93b1891537e04cde10c6fca936676a7ce35e1f03d21a4ff6ff1`; `tools/mkfat32.py` `b82c284a714ed2f546783085da035328d351b37e712624924b27eb5826500565`; `tools/inspect_fat.py` `a41dfad334ac7f459fa71f010aebea9b82e43eb0b5a3e0e8daf533044d745031`; `scripts/prepare-x86-app-initramfs.sh` `bc9b2bdd9859d54ea306abcc070f913d40cfa638eb5fcac1babb20f18684df36`; `scripts/fetch-alpine-x86.sh` `4e2ec9769047f494f13d4c3433de8cc5a12599def5f8ea3180c05d0f9b3726e6`; `scripts/make-hypervisor-fs-x86.sh` `a0c12d03ae3d993c54938e7b3e0c396f5f6e9909ff572cc106a379fb06ea565b`; `scripts/lib-make-hypervisor-fs.sh` `98c28db9cd248c96fe473ab6eb2ad1ad1e5a2334463197e03fc045180bf5f527`.

Guards (G1-G5, unchanged): `scripts/cellos_sign/signing.py` `ba97129308e60c50d59ff669831ab47d344b3ccbf44e8f17fbf052c9666c9511`; `scripts/cellos-sign` `005bb64c653e7328151c2275690f6dd48a89a05eb61876ec3499b4f5bbddd7f0`; `scripts/assemble-x86-image.py` `b74cc99787ab69dde04dac5c517acb6f771016043c23841bc7b40ab0b3bec7ed`; `scripts/package-desktop-x86.py` `d80517a8517f4e7689828ec89c663d88c0c7a51b29711621fd2b7b0350c4f4cd`.

Cached inputs (equal to the judge's `PINS`; re-taken today): `.alpine-cache-x86/vmlinux` `2d62d53b…1ec07`, `vmlinuz-virt` `26bf81ad…24ca`, `initramfs-virt` `e2562e01…cdf`; QEMU 10.2.0 `~/.cache/cellos/qemu-10.2.0/bin/qemu-system-x86_64` `169fb4eaaa79f7ad1f3a2d1a1f3534a0807354eabdedaf6a51b6f4b1412241fe`; the apk set under `/tmp/cellbox-tier3-261030a/apk-fetch` (pins OK today).

### 6.4 New files

`evidence/t3-d3/diag/**`, `evidence/t3-d3/full/**`, `evidence/t3-d3/confirm-<stage>/**` and `evidence/t3-d3/lane-fix-record.txt` (only if boot 3 is used), `evidence/t3-d3/gate-a.txt` and `evidence/t3-d3/baseline.txt` (protected-path digests of the new baseline, pin-list check), `evidence/t3-d-runs/T3D3_*_CONSUMED-1`, and after the runs the CP2 dossier `cp2-dossier-part5.md`. Text only (serial log, argv, verdict, timeline, `host-ss-*`, `pcap-summary.json`, guest snaps, provenance, protected digests, preflight). No other file.

### 6.5 Files the later CP2 may edit (not edited by the runs; digests now, re-taken before any edit; only for outcomes the judges passed)

`.agents/261020-1750-cellbox-cli/acceptance.md` `4122b5591d6634ddbfe5fc04f558623ea97a4549f96636986ed1f39a6b7456b4` (CBX-23, CBX-25 rows; both already carry the 2026-10-30 Tier-3 update); `…/phase-09-ssh-client.md` `fce05ad752ad2903141695331ee80e06c46ca09c755714880401a30bdfa7368e`; `…/phase-10-qualification.md` `33c2d1ee84faa46904026092c7b066f8a19ed1adf39d28168a77d655bdc8a774`; `…/phase-10-s0-proposal.md` `01f2d442eb27d3a208928212cb8c011c7d6d7a5f44f2f32d6cb2d7df8a6e82e3`; `…/plan.md` `d90d4abb61d6098ab1a3ad1b61726bc7e8d8c783ee62ae52ecdb9a7dfb31f8d4`; `…/verification.md` `77147ebd4fb5c9a9b4c18d5c039be68d02aa13ac884a3b686c16d3d158551c98`; `…/reports/rollout-containment.md` `f92f0d96d340551988c7ff011e03a55698838d40fa82fa89f798068f9de17836`; `.agents/plan-portfolio.md` `1300997bad8a18563f6ec9a780b74c277eb6a9158c18c63677ecfd3e83075ff4`; `docs/cellbox-cli.md` `28ac3c3ab9834e04c207a53cdcd2ce9ad94e600da9e741b490616c16a6e3057f`; `CHANGELOG.md` `c5ef0411d34cd77667a91ca71f8d937fe8ff819d3c22228e036a6ede3f6aa5db` (differs from the NET-OD CP1's `8866d0db…`: edited since). Not edited on purpose: `command-catalog.md` `f06f38ce7447758f7b49e58d6e98dc8309b583515bf1ffd3608e665011a5fd0f`, `.agents/cell-contract-0024-locks.md` `1f674e010ab0715bf7b66c554e5d125b2d35b999fb7b1668441abda7227274c0`.

## 7. STOP rules and failure routing

| Rule | Value | Consequence |
|---|---|---|
| STOP-1..6 | as cp1-draft section 5 (no `vCPU ready`/panic/fault/triple-fault; guest never listening; C1 fails; C2 fails; any secret in any log/artifact; any protected-path digest or `Cargo.lock` change). In the diag stage the judge names them STOP-D4 (gates), STOP-D2 (secret), STOP-D1 (digests), STOP-D3 (argv), STOP-D5 (1 h cap) | record, no substitute; boot 3 only under 3.4 item 1 |
| **STOP-7** | `[net] owner death watch unavailable; socket reclaim deferred` in the serial log (the lane aborts the run live, L5; the judge names it after the argv check, J3) | **product finding or a stale image**: the kernel grant did not hold in the real boot or the NET-OD bytes are not what runs. Consumes the boot's token, no boot 2, no boot 3, no further run without a new diagnosis and CP1. The provenance digests of kernel and `service-net` and the preflight pin list are the first things read. |
| STOP-8 | pre-boot: dirty tree outside the allowed list, product pin mismatch, `cellos-sign --check` not rc 0, apk/QEMU/kernel pin mismatch, existing non-empty `$OUT`, a token already spent, a build failure | nothing runs, no token consumed; fix a lane/test defect, or ask. A product pin mismatch means a different product than this CP1 describes: stop and ask |
| STOP-9 | boot 1 is not O7 with a passing verdict | boot 2 is **not** run; the outcome is recorded as found (O1: the fix did not reach the wire; O4: late reclaim; O2/O3: control regressed; O6: surprise) and the program for this row stops pending a new CP1 |
| Marker FAIL in boot 2 | any marker FAIL or `P04-TERMINAL: FAIL` | recorded; no fix-and-rerun; a second FAIL for this row ends the program for it; the diag evidence of boot 1 stands as the observation of the cancel half, but the "full run class" is not met (8.1) |
| No substitution | a lane-side kill of the connection, an RST injected by the lane or a host process, a `p9` that kills `yes`, a longer `gone` wait, a doctored `gone`, a loosened O7 row after seeing the data, a Docker fixture or host server standing in for the guest | never an acceptable way to make the cancel marker pass; the marker is what the observation shows |
| Evidence | every record states: SVM/TCG ceiling statement, HEAD, token, which boot, which pins | |

## 8. What the results may and may not claim, per row

Nothing is edited by this CP1 and nothing by the runs; rows are edited only at CP2, only for outcomes the judges passed, and only with the user's wording decision (D-RR-8).

### 8.1 CBX-23 guest half

| Run result | Allowed claim |
|---|---|
| diag O7 **and** full exit 0 | The cancel half is **met at the stated ceiling**: "At the approved ceiling (SVM VMM, QEMU-TCG 10.2.0, one loopback hostfwd, password, private lane-only image; not Intel VMX, not KVM, not production, not canonical or matched-candidate bytes) a Ctrl-C cancel of a guest `ssh --command` kills the client; the Net service aborts the dead owner's socket (RST), the guest sshd session ends and the `yes` producer stops (`evidence/t3-d3/`)". Together with the cooperative-close evidence of T3-D2 (kept as is) and the 21 full-run markers, the **guest half's own evidence is complete**. The row status word changes **only** per 8.2. |
| diag O7, full fails | the cancel half is observed once (diag, `ssh-guest-cancel-producer` PASS plus the O7 data); the full run class is not met; CBX-23 stays `partial`; the failing marker is recorded |
| diag not O7 (any label) or STOP-7 | not met; the O1 finding is replaced by the new recorded outcome; row unchanged |

### 8.2 The S9-3 pipeline-exec / exact non-UTF-8 stdin limitation (gates the *status word*, not the run)

Facts (read): pipeline exec (`... | ssh --command ...`) is refused by the client because a pipeline stage has no terminal for the password; exact non-UTF-8 **stdin** needs it and moved to S9-5 (publickey/KMS custody, D79-14) which is not authorised and not buildable at this ceiling (KMS has no SSH key class; `phase-09-s0-proposal.md:303`, `kms-prerequisites.md:83`). It is not a guest-specific defect: it applies to the other-host half too, and no guest scenario in the T3 closure exercises it.

| Option | Effect | Cost |
|---|---|---|
| **A** The user dispositions it as a **retained limitation at this ceiling** (same kind as the CBX-27 and D10-1 C dispositions: "met at stated ceiling by user disposition, not feature-equivalent"), recorded at CP2 with explicit wording; CBX-23's guest half then reads `met at stated ceiling` with the limitation stated, not claimed as passed | the row's guest half closes at the ceiling; the row as a whole and Phase 10 criterion 1 still depend on CBX-24 and on the other half's own status, decided at CP2 | the sentence "raw non-PTY stdin ... without JSON conversion" is met for UTF-8 stdin only; this must stay in the row text |
| **B** Keep CBX-23 `partial` and record the guest-half evidence only (status words unchanged); the limitation stays open until S9-5 | nothing hidden, nothing promised | the cancel fix is shown but the row does not move |
| **C** Open S9-5 (publickey, KMS SSH key class, `bcrypt-pbkdf` in the lock) first | the limitation is actually removed | locked, large, unauthorised here; not this CP1 |

Recommendation **A**, decided by the user at CP2 after the evidence exists (D-RR-7): a user disposition is not evidence and must be stated as such. Until the user chooses, the answer is B.

### 8.3 Other rows

| Row | Allowed |
|---|---|
| CBX-25 | **already met at stated ceiling** (acceptance row, commit `296fecebc`); unchanged. The rerun may be cited as an additional observation of the same route, credentials, host key and no-replay results; the row's text "20 of 21 T3-D markers PASS" may be refreshed to the rerun's counts at CP2 as a wording update, not a status change |
| CBX-24, Phase 10 criterion 1 | unchanged (depend on the publickey half and the other-host status) |
| CBX-28 and the other rows, `command-catalog.md`, the S9-6 `ssh-cancel-exec` and L5f records | unchanged. The other-host lanes' cancel evidence stays client-side only; the NET-OD fix reaches them only through a re-promotion (D-NOD-12) or a rebuilt candidate, neither requested here |
| Anything about HEAD beyond these runs | not claimed. In particular not claimed: a canonical or promoted image, the matched candidate, aarch64/riscv64 behaviour (only built, never booted), FIN-flush semantics for a killed owner (RST by design), TLS handshake-in-flight sockets, legacy `TcpClose`, graceful-deadline silent removal, keepalive (NET-OD CP1 5.7 follow-ups), TX behaviour under an RST burst, Intel VMX/KVM |

## 9. Clean-tree requirement

- At writing: HEAD `d42e70d6a`; `git status --short` shows only the untracked `.agents/261030-1945-ginux-lessons-hardening/` (other plan; allowed by the lane because it lies under `.agents/`; not to be touched).
- Before each boot, `git status --porcelain -uall` may list only: (i) the lane/judge/test files of 6.1 (if D-RR-4 B), (ii) paths under `.agents/` (documents, evidence, tokens). Anything under `kernel/`, `hal/`, `libs/`, `cells/`, `config/`, `tools/`, `docs/`, `Cargo.lock`, `scripts/` outside 6.1 is a STOP-8; the lane's preflight (`:107-110`) and the pin list of L4 enforce it. The lane's own allowed-dirty list is wider (it also tolerates `CHANGELOG.md`, `tests/guests/x86-sshd/**`, the prepare/fetch scripts and the driver files of its closure); this CP1 is narrower: those files must be byte-identical to section 6 (`CHANGELOG.md` is only edited at CP2).
- Recommended (D-RR-4 A): commit the three lane-only files after Gate A and before boot 1, as its own commit, and again for any recorded lane fix before boot 3; then the tree is literally clean at every boot and the token's `head=` identifies lane code and product bytes together. The commit and push are **not** authorised by this CP1 text; they are requested by D-RR-4. No other commit is part of the runs; evidence files stay untracked until CP2.
- Between boot 1 and boot 2 nothing changes (no edit, no commit, no pin change); if anything changes, boot 2 is not run until the user says so.

## 10. What runs, is generated, or is written outside the repository

| Item | Where | Notes |
|---|---|---|
| Work directory | `/tmp/cellbox-tier3-261030a` (exists: mode 700, owned by this uid, not a symlink; `apk-fetch`, `cells-target`, `kernel-target`, old logs) | the lane `rm -rf`s `$W/evidence`, `$W/embedded`, `$W/keys`, ISO, trust disk, private config, scratch, `net0.pcap`, serial socket at start; the old logs and `srv-receipt.json` are not touched |
| Network | **none**. No fetch (A-NET is spent and not renewed), no crates, no registry, no Docker, no rustup; cargo `--locked --offline` (a build that needs the network is a STOP) | the only sockets: QEMU's hostfwd listener on `127.0.0.1:<random free port>` while QEMU runs, the loopback `ssh-keyscan` probe, and `ss` on the host |
| Processes | `qemu-system-x86_64` 10.2.0 (TCG, no KVM), `ssh-keyscan`, `ssh-keygen`, `ss` (`/usr/bin/ss`, present), `cargo`, `objcopy`, `xorriso`, `python3` (`cryptography` 41.0.7 present), `sha256sum`, `git` | no Docker, no other QEMU (`pgrep` empty today; the preflight re-checks) |
| Disk | cargo target dirs and ISO under `$W`, several GiB; `/` has about 685 GiB free (preflight needs 8 GiB) | |
| Raw pcap | `$W/net0.pcap` (diag only), carries SSH ciphertext | scanned for every secret by the judge, **deleted** by the lane at the end; only the derived `pcap-summary.json` is archived |
| Home directory | read-only use of `~/.cache/cellos/qemu-10.2.0/`, `~/.cargo` (offline registry cache; cargo may touch its own lock files there) | nothing written intentionally |
| Repository writes | 6.1 (three edits), 6.4 (new evidence files, tokens, dossier) | `.alpine-cache-x86/` read-only; nothing under `build/` or `kernel/src/embedded-*`; Python test runs create gitignored `__pycache__` |
| Not done | any push, any commit other than D-RR-4, any edit outside 6.1/6.4, any change to `/tmp` outside `$W` | |

### 10.1 Secrets handling (unchanged from the approved CP1, restated)

- Per run, generated by the lane into `$W/keys` (mode 700/600): the cell-signing seed (`os.urandom(32)`, `seed.hex`), the guest password (18 random bytes, base64 without `/+=`), the guest host ed25519 key. They are embedded in the private image/initramfs (`etc/t3-host_ed25519`, `etc/t3-password`; audited read-only, mode 0600) and in the trust-fixture disk; the signing key is used only for the private-fixture signing (`CELLBOX_PRIVATE_FIXTURE_SIGNING=1`), never a product key.
- The judge scans the seed, password (plain and hex), the host private key (body, raw, every line, armor) in **every** evidence file, the raw serial log and the raw pcap; the archive step re-scans and refuses to copy on any hit (STOP-5/STOP-D2: scrub, do not archive). Secrets never appear in repo text; verdicts record only fingerprints and hashes.
- After capture the lane deletes keys, scratch, ISO, trust disk, embedded image, initramfs, ISO root, serial socket and pcap. `TIER3_KEEP_SECRETS=1` is **not** set for these runs; if a lane fix needs the raw pcap or keys for inspection it is allowed only for the diag stage, recorded in `lane-fix-record.txt`, and everything is deleted by hand before the next step.
- The guest password is typed by the driver into the cell client only; no secret is printed to the serial log by design (the T3-D and T3-D2 logs scanned clean).

## 11. The F1 gate and other preconditions (observed today, read-only)

| Gate | Observed |
|---|---|
| F1/F5 (`python3 scripts/cellos-sign --check`, pure source parsing, no build, no network) | rc 0: `OK: F5 - rustc f53b654a8882 is the pinned nightly-2026-05-01`; `OK: F1 - 134 crates and 928 files scanned; unsafe confined to 67 allowlisted files`. NET-OD's own dossier read 927 files before the commit (one more file now; the difference is not explained here). The run prints a `NOTE: allowlist entries overdue for re-review` list (dates 2026-07-14 and later, 108 days): a note, not a failure, and not this program's to change. The lane's signing step (`scripts/cellos-sign --sign`) requires this check to pass: a red gate fails before the token, so it costs no boot (STOP-8) |
| Test baselines from NET-OD (not re-run here) | kernel 295, `service-net` 65, `-p api --target x86_64-unknown-linux-gnu` 154 + 2 (4 ignored) |
| Judge tests | `python3 -m unittest scripts.test_tier3_ssh_guest_judge`: 69 tests OK today |
| QEMU 10.2.0 binary | sha256 equals the pin; no QEMU process running |
| apk set / Alpine kernel and initramfs | `sha256sum -c apk-pins.sha256` OK; the three Alpine files equal the judge's `PINS` |
| Host tools | `ss`, `ssh-keygen`, `ssh-keyscan`, `xorriso`, `objcopy`, `cargo` present; `bash -n` of the lane OK |

## 12. Not authorised by this CP1

Any edit, build, QEMU run, key generation or commit before approval; any product or protected-path edit (`kernel/`, `hal/`, `libs/`, `cells/services/**`, `cells/tools/**`, `Cargo.lock`, `config/`, `tools/`, `scripts/cellos_sign`); a change to the driver, guest image, `guest-init.sh`, `p9`, the apk pins or the QEMU argv beyond 5.1; any network fetch; Intel VMX/KVM; re-promotion or canonical import (D-NOD-12); a candidate import; a Docker fixture, host process or lane-side kill standing in for the cancel or the guest; a fix-and-rerun after a product result; more than 3 boots; a row, phase-record, CHANGELOG or portfolio edit before CP2; push.

## 13. Not checked / risks (stated, not hidden)

- Not run today: the lane, any build, any QEMU boot, the NET-OD test suites. The edits of section 5 are specified, not written; the judge's O7 branch, STOP-7, the token rules and the new pcap keys have no code or test yet.
- Whether the kernel's grant predicate (registry-bound Active NET provider, generation, serving context) holds in the real boot order is the main unknown; STOP-7 is the designed detection.
- Wake latency of the death on `sys_wait_completion(NET_RX, 10)`, RST TX under e1000/`VirtioNetDevice`, and what slirp does with an RST on flow A (the data may show an RST on flow B, a FIN, or nothing at all) are unobserved. O7 requires the end to be visible on both flows and in the guest; an unexpected but benign combination would be O6 (surprise) and would stop the program for this row rather than be accepted ad hoc.
- NET-OD changes the Net service for **every** client cell in the image: any client that exits with an unclosed TCP socket now sends an RST (previously silent). The reused scenarios may therefore show guest sshd `Connection reset` lines that the T3-D logs do not have; the judge counts accepted/failed passwords and sessions per connection, not close wording [INFERENCE: no judge check keys on the wording of a close line other than `closed_logged` for the diag control].
- The `[net] owner death:` log line is corroboration, not attribution (4.1).
- Timing of O7: snap-a is 3 s after the cancel; a reclaim slower than that is O4 and not a pass (a stricter reading than the 20 s `gone` window, chosen so the pre-registered criterion does not move after seeing data).
- The hash `4122b559…` of `acceptance.md` and the other CP2 digests are only for the later edits; they are re-taken at CP2.
- Only the x86-64 path is exercised; the aarch64/riscv64 kernels share the changed source but are not run (NET-OD R-11).

## 14. Effort

Lane/judge/test edits plus Gate A: about 1 working day; build and boot: diag about 165 s, full about 127 s of QEMU time each plus builds; dossier and CP2 documents: about half a day [INFERENCE from the earlier runs].

## 15. Approval request and decisions

**Requested:** approval of this CP1 (the approval record binds the SHA-256 of this file at approval time) with decisions D-RR-1..11 as recommended or as amended by the user. On approval the executing agent (1) makes the section 5 lane/judge/test edits, (2) runs Gate A (5.5), (3) after D-RR-4 commits the lane-only files, (4) takes boot 1 (diag), (5) takes boot 2 (full) only if boot 1 is O7 with exit 0, (6) takes boot 3 only under 3.4, (7) writes `cp2-dossier-part5.md` and stops for CP2, where the row wording decisions (D-RR-7, D-RR-8) are the user's. Nothing outside section 6.1/6.4 is written; no network, no product edit, no push.

| ID | Question | Options | Recommended |
|---|---|---|---|
| **D-RR-1** | Run plan | **A** diag, then full only on O7, plus at most one confirmation boot (max 3 boots); **B** full only (no wire observation of the RST, no O7 data); **C** diag only; **D** no run | **A** (the diag boot is what proves RST + guest effect; the full boot is the 21-marker class that CBX-23 needs; boot 3 is bounded) |
| **D-RR-2** | Lane/judge/test scope | **A** the full set of 5.1-5.4 (O7, STOP-7, reclaim-line check, `--expect-outcome`, pin list, `--claim-only`, new tokens, no-overwrite guard); **B** minimal: O7 label, STOP-7, new token prefixes only | **A** (each added item removes a way to burn a boot or to accept a stale or mislabelled run; none touches product or driver) |
| **D-RR-3** | Budget tokens | **A** new per-stage tokens `T3D3_*` as in L3 (old tokens retained as records, `RUN_CONSUMED-2` stays unused); **B** claim `RUN_CONSUMED-2` for the full run and add a new diag prefix | **A** |
| **D-RR-4** | Commit of the lane-only edits | **A** commit the three files after Gate A and before boot 1 (and before boot 3 for a recorded fix), own commit, no push; **B** leave them dirty (the lane allows it; the token records HEAD only) | **A** (literal clean tree; reproducible head) |
| **D-RR-5** | O7 strictness (4.1) | **A** as written, snap-a at +3 s, RST + guest + host + log all required; **B** drop the pcap rows 4-6 (marker, guest, host only); **C** accept O4 as a pass | **A** (a pre-registered, observable criterion; B and C would let a surviving or late connection pass) |
| **D-RR-6** | Windows | **A** diag lane rule (<= 3600 s), full `TIER3_BOOT_WINDOW=3600`; **B** the CP1's original 4 h for full | **A** (observed 127-165 s; the 4 h lane cap is unchanged but not used) |
| **D-RR-7** | CBX-23 status word and the S9-3 pipeline-exec / non-UTF-8 stdin limitation (8.2) | **A** at CP2 the user dispositions it as a retained limitation at the ceiling, and the guest half becomes `met at stated ceiling` if diag O7 + full pass; **B** keep `partial`, evidence only; **C** open S9-5 first | **A**, as a user disposition at CP2 after the evidence exists; **B** until then |
| **D-RR-8** | Wording of the cancel half after a pass (8.1) | **A** the sentence of 8.1 with the ceiling statement and the RST/abort note; **B** the user rewrites at CP2 | **A** (proposed; binding only at CP2) |
| **D-RR-9** | Failure routing | **A** STOP-9 and 7: a product result ends the program for this row; second FAIL ends it; boot 3 only under 3.4; **B** allow one extra boot after any single product FAIL | **A** (same rule as T3-D, T3-D2, K8; no fix-and-rerun by trial) |
| **D-RR-10** | Boot 3 eligibility (3.4) | **A** host/environment errors and recorded lane/judge/instrument defects whose raw data already satisfy the criteria; never a relaxed product criterion; **B** host/environment errors only | **A** (it is the T3-D2 boot-1 situation; the guard against relaxing a failed criterion is explicit) |
| **D-RR-11** | Evidence retention | **A** text only archived under `evidence/t3-d3/`, raw pcap and keys deleted, `TIER3_KEEP_SECRETS` unset; **B** keep the raw pcap until CP2 | **A** (the pcap carries SSH ciphertext; the summary is what the judge uses) |
