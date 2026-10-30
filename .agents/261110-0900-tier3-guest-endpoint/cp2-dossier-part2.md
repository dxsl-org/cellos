# T3 CP2 dossier, part 2: confirmation boot and T3-D implementation (T3Exec, 2026-10-30; no commit, no CHANGELOG, no rows/docs)

Follows `cp2-dossier-part1.md`. Approved by Main (cp1-approval.md amendment): one extra stage-boot confirmation of the fixed guest-init (not evidence, outside the 2-boot budget), then T3-D implemented per CP1 section 3 **without** running the full-stage evidence boot.

## 1. Confirmation boot (stage boot, not evidence): PASS

`evidence/t3-bc-confirm/` (same lane, same judge, run with `TIER3_LINGER=60` so the driver could run on after the gates; `TIER3_EVIDENCE_OUT` pointed at the new dir). Judge: B1, B2, C1, C2 all PASS, `stop_rule: null`, 26/26 checks, protected digests unchanged. Timeline (s): SVM root 1.9, hypervisor starting 3.4, vCPU ready 7.0, `TIER3_SSHD_GUEST_INIT_START` 9.6, sshd `Server listening` 10.8, `TIER3_SSHD_LISTENING` 11.8, `ssh-guest-ready attempts=1` 12.1 (guest init bytes: the fixed `guest-init.sh`, sha256 `6ff396d9…3def0`).

What the 60 s linger showed (informational, **not judged, not claim evidence**): the whole reused scenario set passes against the guest, `P04 PASS` for `shell-prompt`, `ssh-conf`, `ssh-refusals`, `ssh-changed-host-key`, `ssh-first-trust-no`, `ssh-first-trust-paste-refused`, `ssh-first-trust-yes` (the part-1 FAIL is fixed: `chmod 755 /`), `ssh-first-trust-known-after`, `ssh-wrong-password`, `ssh-exec-bytes`, `ssh-exec-stdin-eof`, `ssh-exec-signal`, `ssh-pty-shell`, `ssh-pty-resize`, `ssh-pty-escape-disconnect`, `ssh-shell-alive`, `shell-long-line`, `session-a-close`, then `P04-TERMINAL: PASS checks=18`. Guest sshd log: six `Accepted password for tester`, one `Failed password for tester` (the wrong password), PTY session `shell on pts/0`. The run took 72 s.

## 2. T3-D implementation (all inside the CP1 section 6 closure)

| Path | sha256 | Change |
|---|---|---|
| `cells/tests/phase04-terminal-driver/src/ssh/guest.rs` | `d5e4d95c1547379bf15e1929ca1863f85afaf935141370daaa57c46a4c51fab0` | `guest_checks` with `ssh-guest-cancel-producer`, `ssh-guest-fault-may-have-run`, `ssh-guest-no-replay-ledger`; `follow_up` (fresh login, up to 2 retries when the read did not exit 0); `wait_ready` and `guest_checks` are `pub(super)` |
| `cells/tests/phase04-terminal-driver/src/ssh.rs` | `69207cd52b00fe4e905d223a3bba0d77219e40fecfe9daf994d38bdfc984d937` | `#[cfg(feature = "guest")] guest::guest_checks(a, &f);` after the (guest-excluded) `s96_checks`; guest `SSH_MS` 300 000 ms (was provisional 600 000; T3-B/confirm measured a full exchange at 1–2 s) |
| `scripts/prepare-tier3-sshd-initramfs-x86.sh` | `34ecc945ebbbc55eac9f3567cd17f05a98365828abafa17c5defca6b50930a29` | guest helper `p9` gains `selfkill`, `produce`, `gone` |
| `scripts/qemu-tier3-ssh-guest-x86.sh` | `b34354e06681f10164ca1965f0069dad56f99ae1f0d9d8fc4af2ba4551833eb8` | `--stage full` (ends on `P04-TERMINAL:`; window default 7200 s, max 14400 s; O_EXCL token `RUN_CONSUMED-1/2` under `evidence/t3-d-runs/` claimed just before QEMU, refuses a third run; output default `evidence/t3-d`); `TIER3_LINGER` |
| `scripts/tier3_ssh_guest_judge.py` | `d540e69dc1c35286cfc5c323b9a0b5f77391e9b8ac1f9739bef74d84bddf82e7` | subcommand `full` |
| `scripts/test_tier3_ssh_guest_judge.py` | `3ffde5ba6867b6073a25bcf8828abae611148e4966c54d03c68a12536965dbe1` | 34 tests (12 new for `full`) |

Scenario design (the unmodified product `cellbox` is the client; the guest is the only fault injector):
1. `ssh-guest-cancel-producer`: `echo p09-started; p9 produce` (pid to `/tmp/p09-prod.pid`, then `exec yes p09y`); the driver waits for `p09-started`, sleeps 500 ms, sends Ctrl-C. Required: `st=130`, `completion` mode exec, phase running, dispatch accepted, outcome cancelled; then a **second login** runs `p9 gone` (waits up to 20 s for the pid to be gone or a zombie) and must exit 0 with `p09-gone`.
2. `ssh-guest-fault-may-have-run`: `p9 selfkill` appends `p09x-selfkill` to `/tmp/p09-ledger`, SIGKILLs the nearest ancestor whose cmdline starts `sshd-session` / `sshd: `, then `sleep 20`. Required: `st=255`, mode exec, outcome `unknown`, remote status/signal null, dispatch `accepted` or `may_have_run` (the observed value is printed as `ssh-guest-selfkill dispatch=…`), finished in under 18 s (so the command's own sleep did not end it). No retry by the client is visible as a second session or a second ledger line.
3. `ssh-guest-no-replay-ledger`: a later login `cat /tmp/p09-ledger`; exactly one `p09x-selfkill` line (printed as `ssh-guest-ledger selfkill=N lines=M`).
The driver also prints `P04 INFO ssh-guest-logins N` (every password login it made in these three scenarios). Not run (need Docker hooks/sinks, per CP1): the S9-6 group.

Full-stage judge (`full`, everything `boot` judges is re-judged from the same log, so B1/B2/C1/C2 are re-proved at the start of every full run): each of the 21 markers (`shell-prompt`, 16 reused, 3 guest, `session-a-close`) PASS exactly once, no FAIL, no unexpected marker; `P04-TERMINAL: PASS checks=21` exactly; `[selftest] boot-ceiling: PASS`; trust store mounted; no weak-entropy line; fixture kernel features recorded; guest sshd log: before the first guest scenario exactly 6 accepted and 1 failed password, after it ≥4 accepted (≤ the driver's reported logins) and 0 failed; at least 10 sessions and never two sessions on one connection; ledger rule; the driver-verdict ends the run; window ≤ 4 h. A failing marker or log rule is reported as stop rule `MARKER-FAIL` (no fix-and-rerun without a new CP1); STOP-1..6 as before.

## 3. Offline gate (`evidence/gate-d-offline.txt`)

`bash -n` on the four scripts; py_compile; judge host tests 34/34; driver `cargo build --features guest` plus `cargo check` of the default, `phase09-ssh` and `kssh` builds all compile without warnings (`Cargo.lock` untouched); apk pins 9 OK; `p9` verbs exercised on the host (selfkill killed the renamed ancestor, wrote exactly one ledger line, the command after it did not run; `produce`/`gone` recorded and detected the pid); `--stage full --build-only` assembled the ISO (no QEMU, no token); protected digests and `Cargo.lock` equal `protected-before.txt`. Work dir cleaned of keys, images and ISO.

## 4. Command for the main agent (run once)

```
cd /home/dmin/cellos && CELLBOX_TIER3_WORK=/tmp/cellbox-tier3-261030a bash scripts/qemu-tier3-ssh-guest-x86.sh --stage full
```
Requires a clean tree except the closure and `.agents/**` (the lane checks), no running QEMU, pinned QEMU 10.2.0, `/tmp/cellbox-tier3-261030a/apk-fetch` (kept). Duration expected a few minutes (confirmation run: 72 s for the reused set); it self-limits to 7200 s. Evidence lands in `evidence/t3-d/` (verdict.json, serial.log, qemu-argv.txt, timeline.json, c1-keyscan.json, boot-config, provenance, preflight, protected digests); a token `evidence/t3-d-runs/RUN_CONSUMED-1` is created before QEMU starts; the second token is for a host/environment error only.

## 5. Known risks for that run (unobserved in a guest, so a MARKER-FAIL would be a first observation, not a regression)

- The three guest scenarios have never run against the guest: the `selfkill` ancestor walk (verified on the host with a renamed process, not against real `sshd-session` process titles), the Ctrl-C cancellation of an unbounded `yes` flood through nested TCG (output volume until the cancel is bounded by the 500 ms wait), the dispatch value after the kill (`accepted` expected; `may_have_run` also accepted), and `p9 gone` after a cancelled session (zombie handled).
- The sshd-log accounting uses serial order; a retried follow-up login is accounted via the driver's login count.
- No third boot or second full run beyond the budget exists without a new CP1.
