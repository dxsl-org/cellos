# GP CP1: pipeline exec and exact non-UTF-8 stdin for `cellbox ssh` against the Tier-3 guest sshd (the S9-3 limitation item) (status AWAITING CP1; documents only; nothing edited, built, fetched or run)

Base: HEAD `952d84c8aa6b6ede349c0447e965d1448d231463`, `git status --porcelain` empty when the hashes below were taken (2026-10-30). Every SHA-256 is from that tree; anything else under `git status` at execution time is a STOP. Evidence this text builds on: [phase-09-s0-proposal.md](../261020-1750-cellbox-cli/phase-09-s0-proposal.md) §9 deviation 3 (`:303`), [acceptance.md](../261020-1750-cellbox-cli/acceptance.md) CBX-23, [rerun-cp1.md](../261110-0900-tier3-guest-endpoint/rerun-cp1.md) §8.2, [k6-cp1-draft.md](../261030-1000-kms-ssh-key-custody/k6-cp1-draft.md) §3.4, [k6-cp2-dossier.md](../261030-1000-kms-ssh-key-custody/reports/k6-cp2-dossier.md), [k8-cp1.md](../261030-1000-kms-ssh-key-custody/k8-cp1.md) S15, [net-owner-death-cp1.md](../261110-0900-tier3-guest-endpoint/net-owner-death-cp1.md) §11 (form of the lock handoff), [cp2-dossier-part5.md](../261110-0900-tier3-guest-endpoint/cp2-dossier-part5.md).

Ceiling (verbatim from the approved T3 CP1, repeated in every record this program produces): **the claim is made against a Linux guest running OpenSSH on the existing AMD SVM VMM under QEMU-TCG 10.2.0 (`qemu64,+svm`), reached from a Cellos cell by one loopback-only QEMU `hostfwd`, in a private lane-only image. Not Intel VMX/EPT, not KVM, not production, not matched-candidate or promoted bytes, no remote or C2C use.** Added here: the credential is a **plain (unwrapped) KMS-held ed25519 key at `DEV_POSTURE`** (volatile `ssh-dev-custody`), because that is the only credential a pipeline stage can use.

## 0. Summary

1. **What the limitation is today (read from code, not from the retained text).** "Pipeline exec and exact non-UTF-8 stdin" is two different situations:
   - **Password mode: refused by design, and still refused.** `ssh --auth password …` is a `Session` command (`typed.rs:141-151`), which must be the only stage with no redirects (`typed.rs:335-337, 422-424, 440, 462`): `cat F | ssh …`, `ssh … < F`, `ssh … | cat` are rejected before any effect with status 2 and `` `ssh` is an interactive session; it must be the only stage with no redirects `` (`executor.rs:1351-1356`). If admission were bypassed, the child refuses a stage that is not publickey+`--command` with `no_terminal` (`ssh.rs:1117-1122`). A foreground password exec reads **canonical typed text** (`EventKind::Text(&str)`, Enter→LF, Ctrl-D→EOF, `ssh.rs:797-820`), which cannot carry `0xFF`. The reason is structural: the password is captured only by the foreground client under its own no-echo lease, and the contract forbids "batch password via stdin/environment/argv" (`command-contracts.md:136`).
   - **Publickey mode: implemented (K6b) and proven only on the other-host fixture.** `--auth publickey --identity K … --command C` is an external byte stage (`typed.rs:148`, `PipeTerm` `ssh.rs:937-1028`, stage launch `executor.rs`, completion slot `libs/terminal/src/pipe_child.rs:64-69`): bytes of the upstream stage unchanged, one EOF, stdout/stderr as byte streams, a `completion` record; a wrapped key, an unknown host and a PTY are refused in a stage. Evidence: host tests (`typed.rs:2157-2218`), and **K8 RT0 S15(iv) PASS and RT1 boot 3 S15 PASS on the Docker OpenSSH fixture** (4,096-byte `raw.bin` with `0x00`/`0xFF`, remote SHA-256 equal to the host's, no prompt, one `Accepted publickey`; `k8-evidence/rt0/serial.log:703`, `rt1/boot3/serial.log:413`). **Nothing has ever run a pipeline stage against the Tier-3 guest.** The retained wording in CBX-23 ("S9-3 pipeline-exec/non-UTF-8 stdin limitation retained … not proven for the guest") is therefore **stale for publickey** (K6/K8 re-opened it for the other host) and **accurate only for password mode and for the guest**.
2. **Does the product need a code change? No, for the proof asked.** The publickey stage path is transport-agnostic (the same vissh session, the same `NetClient`); the guest differs only in the server. What is missing is a lane that has (a) KMS and the K7 admin cell in the guest-lane image, (b) a guest sshd that accepts the key, (c) a driver scenario and judge checks. All of that is lane-only (scripts, guest init, private driver). **The closure of §6 edits no file under `cells/tools/cellbox`, `libs/{terminal,vissh,ostd,api,types}`, `cells/services/**`, `kernel/**`, `hal/**`.** Therefore no lock handoff is needed for the proof (§10 gives the acknowledgement text), and no product-pin of the existing lane breaks.
3. **What would need a product change (not recommended, not in this CP1).** Making *password* mode carry exact stdin (§8, option C): shell-fed stdin plus a terminal-holding child; touches `cells/tools/cellbox/{typed,executor,ssh}.rs`, `libs/terminal/src/{session,pipe_child}.rs`, changes every cellbox ELF, re-opens the lane pins and the K8/L5 matched evidence, and weakens the "secret never crosses a pipeline" argument. §10.3 gives the contingent lock/ownership packet so the user can see the cost; it is not requested.
4. **The program.** New lane stage `--stage pipe` of `scripts/qemu-tier3-ssh-guest-x86.sh` (additive; `boot`/`full`/`diag` bytes and tokens untouched): the existing guest image plus `/bin/kms` (`ssh-dev-custody`), `/bin/kms-ssh-admin`, a throw-away ed25519 key authorized in the guest sshd, a 512 KiB blob with NUL, `0xFF`, invalid UTF-8, CR/ESC/Ctrl-C/Ctrl-D bytes. Eight driver markers, the main one `cat /t3p/blob.bin | ssh … --auth publickey … --command 'cat > f; sha256sum f; wc -c < f'` whose remote digest and length equal the host's, plus a stdout round trip and the refusals that are the retained password limitation. Budget: offline gates, **≤ 2 rehearsal boots, 1 evidence boot, ≤ 1 confirmation boot** (each boot observed at 45-165 s in this lane), tokens `T3P_*`.
5. **Row wording** (§9): if all markers PASS the CBX-23 tail becomes "exact non-UTF-8 stdin and pipeline exec: **met for publickey exec on the other host (K8) and on the guest (this program)**; password mode refused by design"; the sentence "not proven for the guest" is replaced; the row status word is not decided here.

## 1. Facts read (with locations)

| Fact | Where |
|---|---|
| `ssh` classification: bad argv → Builtin; publickey **and** `--command` → `External`; every other parsable ssh (password either mode, publickey PTY) → `Session` | `cells/tools/cellbox/src/typed.rs:141-151`; `is_ssh_stage` `:163-167` |
| A `Session` stage must be alone with no redirects; message text | `typed.rs:335-337` (`session_message`), `:422-424` (sole + redirects), `:440`, `:462` (in pipelines); status 2 via `reject_line` `executor.rs:1351-1356` |
| Background ssh is refused (also a publickey stage) | `typed.rs:765-767` |
| Lone publickey exec with no redirect stays legacy (foreground, typed stdin); `cat F | ssh` and `ssh … > F` become byte stages | `typed.rs:2194` test, `admit_bytes` |
| Child: a stage = no control grant; only publickey+`--command` may be one, else `no_terminal` (exit 255) | `ssh.rs:1117-1122` |
| `PipeTerm`: stdin read non-blocking from the stage's own stdin stream, `Err(Eof)` → one `Input::Eof`; stdout/stderr blocking writes; `confirm_unknown` → `NoConfirmation`; `password` unreachable; wrapped identity → `unlock_needs_terminal` | `ssh.rs:937-1028`, `:963-970`, `:976-979` |
| Foreground exec stdin is canonical typed text | `ssh.rs:797-820` (`Text(&str)`, Enter=LF, Ctrl-D key=EOF) |
| `cat FILE` head of a byte pipeline: `read_file_bytes_exact` into a `Vec`, bounds **1 MiB** (`CAT_READ_MAX`) and 4 MiB (`MAX_SINK_BYTES`); heap arena 6 MiB | `executor.rs:1684-1688`, `cmd_fs.rs:828,834-845`, `typed.rs:45`, `main.rs:73` |
| Redirected stage output is collected (≤ 4 MiB) and written to the VFS in ≤ 400-byte chunks | `executor.rs:4519-4549`, `cmd_fs.rs:719-735` |
| Contract: non-PTY stdin bytes unchanged, no UTF-8 decoding, EOF exactly once; receive window 64 KiB, channel data chunk ≤ 16 KiB, local pending ≤ 64 KiB; batch password via stdin/env/argv rejected | `command-contracts.md:150`, `:156`, `:136` |
| K6b limits stated and accepted: wrapped key refused, unknown host refused, plain identity only, no PTY completion | `k6-cp1-draft.md` §3.4 (approved, `k6-cp1-approval.md`) |
| K8 S15: (i) wrapped key in a stage `unlock_needs_terminal`, no new connection; (iii) unknown host in a stage refused; (iv) plain key, `cat raw.bin \| ssh … 'cat > /tmp/kssh-in; sha256sum /tmp/kssh-in'` digest equals, `completion` present | `k8-cp1.md` row S15; driver `kssh.rs:1224-1281` |
| Guest lane today: password only; sshd_config has `PubkeyAuthentication no`; user `tester`, password per run; `p9` helper; no KMS, no `/POLICY.BIN` | `scripts/prepare-tier3-sshd-initramfs-x86.sh` (sshd_config block), `tests/guests/x86-sshd/guest-init.sh`, lane `qemu-tier3-ssh-guest-x86.sh:257-298,350-363` |
| Guest lane already PASSes (21/21, full run) `ssh-exec-bytes` (exact stdout incl. NUL/`0xFF`), `ssh-exec-stdin-eof` (UTF-8 echo, Ctrl-D), `ssh-refusals` (includes `ssh … --command 'true' \| cat`, a pre-connection refusal) | `cp2-dossier-part5.md:64`, driver `ssh.rs:195-214,322` |
| KMS rights come from launch edges: `/bin/cellbox-ssh` → `SSH_CLIENT_EDGE` (sign), `/bin/kms-ssh-admin` → `KMS_SSH_ADMIN_EDGE` (admin), both in `cellbox_profile`; an **absent** `/POLICY.BIN` "tightens nothing" so request ∩ edge stands; a **loaded** policy strips both rights from unlisted paths (H-1) | `launch_profile/profiles.rs:60-100,186-196`, `cap.rs:573-590`, `policy.rs:1065-1125`. [INFERENCE] The absent-policy + KMS combination has never booted (K8 always loaded the v8 policy); read from code only |
| Lane feature set is identical to K8's: kernel `phase02-terminal-profile,phase04-driver-profile` | lane `:91`, K8 `qemu-phase09-ssh-test.sh:118` |
| Observed lane timings: time-to-listening 11.7-12.9 s, driver verdict 44.8 s (T3-D3 full), 135.6 s (diag), elapsed 127-165 s; image 71.3 MB FAT | `cp2-dossier-part5.md:43`, `evidence/t3-d3/full/{timeline,fat-layout}` |
| Lock inventory names none of the closure paths (grep of `cell-contract-0024-locks.md` for `tests/guests`, `qemu-tier3`, `tier3_ssh`, `phase04-terminal-driver`, `libs/terminal`, `libs/vissh`, `libs/ostd`, `cells/tools/cellbox`, `prepare-tier3`: 0 hits; `kms-ssh-admin` and `cells/tools`: only the K7 adopted-slice paragraph, `:121`) | this session |

## 2. The question, answered per form

| Form | Today (HEAD) | After this CP1 if all markers PASS |
|---|---|---|
| `cat F \| ssh --auth password … --command C` | **refused**, status 2, pre-effect (host test only; the guest lane tests the `ssh … \| cat` form) | unchanged; **new guest marker** exercises this exact form |
| `ssh --auth password … < F` / `> F` | refused (same rule) | unchanged (not re-tested; same code path `typed.rs:422`) |
| password foreground exec, typed stdin | UTF-8 text only; Ctrl-D = EOF; guest PASS | unchanged |
| password exec stdout | exact bytes (guest `ssh-exec-bytes` PASS) | unchanged |
| `cat F \| ssh --auth publickey --identity K … --command C` | works, plain KMS identity, known host; other host PASS (K8); **guest: never run** | **guest PASS** at the ceiling |
| `ssh --auth publickey … --command C > F` | admitted (host test); never run in a lane | guest round trip (marker 7) |
| wrapped key / unknown host in a stage | refused, `unlock_needs_terminal` / `host_key_no_frontend` (K8 other host) | unknown host and unknown identity re-shown on the guest; wrapped key not re-tested (needs a passphrase-wrapped import; K8 S15(i) owns it) |

## 3. Design

### 3.1 Stage `pipe` (lane only)

`scripts/qemu-tier3-ssh-guest-x86.sh --stage pipe [--build-only|--claim-only]` — same preflight, same QEMU argv invariant (one user netdev, one loopback hostfwd, no `-object`), same B1/B2/C1/C2 gates, different payload:

| Piece | Change relative to `full` |
|---|---|
| Cells built | adds `-p service-kms --features ssh-dev-custody` and `-p app-kms-ssh-admin`, each in its own cargo invocation (so the feature is not unified into other packages; K8 precedent, `qemu-phase09-ssh-test.sh:242-248`); `CELLOS_PRODUCTION` must be unset (service-kms `build.rs`) |
| Image | `/bin/kms`, `/bin/kms-ssh-admin`, `/t3p/{blob.bin,pipe.key,pipe.conf}`; `services.toml` gains the KMS row (`name = "kms"`, `path = "/bin/kms"`, `service_id = 13`, `restart = "always"`, as `qemu-phase09-ssh-test.sh:386-389`), and the driver cell's `after` list gains `kms`. **No `/POLICY.BIN`** (recommended shape, D-GP-4) |
| Driver | `app-phase04-terminal-driver --features guest-pipe` (new feature `= ["guest"]`, no dependency, `Cargo.lock` unchanged) |
| Guest | `prepare-tier3-sshd-initramfs-x86.sh` with `T3_PUBKEY=…pub`: stages `/etc/t3-authorized_keys` and a sshd_config with `PubkeyAuthentication yes`, `PasswordAuthentication no`; the default (no variable) output stays byte-identical in content (gate G6) |
| Secrets | one more throw-away key `$KEYS/pipe_ed25519` (`ssh-keygen -t ed25519 -N '' -C t3-pipe`, mode 600); the private key is staged in the FAT image (as K8 stages `kp0.key`) and is deleted with `$KEYS` afterwards; the judge scans every file and the raw serial for its body lines, raw bytes and armor |
| Tokens/windows | §7 |

`full` and `diag` keep their images (`PubkeyAuthentication no`, password); their evidence and tokens (`T3D3_*`) are records and are not re-run.

### 3.2 The blob (host-generated per run, in the lane's python block)

512 KiB = 524,288 bytes, below the 1 MiB `CAT_READ_MAX` head bound, 8 × the advertised 64 KiB window and 32 × the 16 KiB chunk, so both directions cross window credits and the local pending bound many times (backpressure is exercised, not assumed). Content: `os.urandom` filler, overwritten at fixed offsets with: `00 00 ff ff` at 0; `C0 AF`, `ED A0 80`, `F8 88 80 80 80`, `FE FF`, `80 BF` (all invalid UTF-8); `03`, `04`, `0D 0A 0D`, `1B 5B 41`, `7F` (bytes a terminal path would act on or translate); a 4,096-byte NUL run and a 4,096-byte `FF` run straddling the 64 KiB boundary; last four bytes `00 FF 0A 00` (no trailing-newline normalisation). The lane asserts before the run that the blob is not valid UTF-8, contains NUL and `0xFF`, and records `sha256` and length in `pipe.conf` (`t3pipe-conf 1`, `blob sha256=<hex> len=524288`, `keyfp SHA256:<fp>`, `name t3k`, `nonce <16 hex>`). The judge **recomputes** the digest from `$W/t3p/blob.bin` itself (not from the driver's output).

### 3.3 Driver (`ssh/guest.rs`, `pipe_checks`; called after `wait_ready`, replaces the S9-3 group; `ssh_checks` returns after it so `session-a-close` and `P04-TERMINAL:` come from `main.rs` unchanged)

Secrecy discipline copied from `kssh.rs` (read key text with `send`+`wait_prompt`, never `Term::run`, clear transcripts; `admin_import` pastes at the echo-off prompt after a 500 ms settle; `kssh.rs:355-389,585-616`), re-implemented in `guest.rs` (about 80 lines) so `kssh.rs` stays frozen.

| # | Marker | Steps and pass conditions |
|---|---|---|
| 1 | `shell-prompt` (`main.rs`) | as today |
| 2 | `ssh-conf` (`ssh.rs`) | as today (`/p09/ssh.conf`, readiness probe `ssh-guest-ready`) |
| 3 | `ssh-guest-pk-import` | read `/t3p/pipe.conf`; `kms-ssh-admin status` until the service answers (bounded, ≤ 300 s); `kms-ssh-admin import t3k` with the key pasted; `st=0`, `custody: plain`, the `WARNING: unencrypted key held by the key service (development posture)` line, the printed fingerprint equals `keyfp` |
| 4 | `ssh-guest-pk-exec` | foreground `ssh --user tester --auth publickey --identity t3k -p PORT 10.0.2.2 --command 'echo NONCE'`: stdout exactly `NONCE`, `st=0`, no prompt of any kind, `completion` record `mode=exec outcome=remote_exit remote_status=0 error_code=null`. Isolates publickey-to-guest failures from pipeline failures |
| 5 | `ssh-guest-pipe-refusals` | (a) `cat /t3p/blob.bin \| ssh --user tester --auth password -p PORT 10.0.2.2 --command cat`: `st=2`, stderr contains `interactive session`, no `password: `, no `SHA256:` (no connection). (b) `cat … \| ssh … --auth publickey --identity nope …`: `st=255`, `no such identity`, record code `identity_unknown`, no connection. (c) `cat … \| ssh … -p PORT gateway --command true`: `st=255`, `host key not trusted; confirmation needs an interactive frontend`, no prompt (this one connects and ends after key exchange, as K8 S15(iii)). The driver prints `P04 INFO ssh-guest-pipe-mark neg-ab-begin\|neg-ab-end\|neg-c-begin\|neg-c-end` around them. **This marker is the retained password limitation shown on the guest.** |
| 6 | `ssh-guest-pipe-stdin-exact` | `cat /t3p/blob.bin \| ssh … --auth publickey --identity t3k … --command 'cat > /tmp/p09-pipe-in; sha256sum /tmp/p09-pipe-in; wc -c < /tmp/p09-pipe-in'`: `st=0`, no prompt, stderr empty; first stdout line `<sha>  /tmp/p09-pipe-in` with `<sha>` = conf digest, second line `524288`; stage `completion` record as marker 4. Prints `P04 INFO ssh-guest-pipe-digest which=stdin sha=… len=…` |
| 7 | `ssh-guest-pipe-roundtrip` | `cat /t3p/blob.bin \| ssh … --command cat > /tmp/p09-rt.bin` (`st=0`), then `cat /tmp/p09-rt.bin \| ssh … --command sha256sum` prints `<sha>  -` equal to the conf digest: stdout of a stage is byte-exact too, full duplex across the window. Prints `P04 INFO ssh-guest-pipe-digest which=roundtrip …` |
| 8 | `session-a-close` (`main.rs`) | as today |

Also printed: `P04 INFO ssh-guest-pipe-logins 4` (the client logins that reached authentication success: markers 4, 6, 7a, 7b). Final line `P04-TERMINAL: PASS checks=8`.

### 3.4 Judge (`scripts/tier3_ssh_guest_judge.py`, stage `pipe`; tests in `scripts/test_tier3_ssh_guest_judge.py`)

- `main`: subcommands become `boot full diag pipe`; `cmd_boot` maps `pipe` to its own stage; `--expect-outcome` and `--pcap` stay diag-only.
- `judge_boot(stage="pipe")`: reuses B1, B2, C1, C2, the argv invariant (no `-object`), hash pins, no-sshd-in-outer-image, secret scans, protected paths, `run-ended-by-driver-verdict`, `net:no-owner-death-watch-unavailable` (STOP-7 applies). **Not** reused: `D:` S93 accounting, the cancel-window check `net:owner-death-reclaim-logged-in-cancel-window`, the ledger rule.
- `secrets_from`: generalise the host-key block to also cover `pipe_ed25519` (body, base64-decoded raw, per-line, armor). The blob is not a secret.
- New `pipe_checks(...)` (prefix `P:`; any `P:` failure is `MARKER-FAIL`):
  1. `P:markers`: `PIPE_MARKERS = [shell-prompt, ssh-conf, ssh-guest-pk-import, ssh-guest-pk-exec, ssh-guest-pipe-refusals, ssh-guest-pipe-stdin-exact, ssh-guest-pipe-roundtrip, session-a-close]`, each PASS exactly once, no FAIL, no extra marker; `P04-TERMINAL: PASS checks=8`.
  2. `P:digest-stdin-equals-host`, `P:digest-roundtrip-equals-host`: the two `ssh-guest-pipe-digest` lines equal the judge's own `sha256`/length of `$W/t3p/blob.bin` (recomputed; the file is read before cleanup and its digest/length written to `pipe-blob.json`, archived without content).
  3. `P:blob-is-non-utf8-with-nul-and-ff` (host-side property check of the same file).
  4. `P:kms-banner-once`: exactly one host line `[kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf` (the control that the feature build ran), and (policy-less shape) no `[policy] loaded` line; if D-GP-4 = B the line `[policy] loaded + verified (59 entries` is required instead.
  5. `P:sshd-accepted-publickey-equals-logins`: guest log has exactly `ssh-guest-pipe-logins` (= 4) `Accepted publickey for tester from 10.0.2.2 port N ssh2: ED25519 SHA256:<fp>` lines and every `<fp>` equals `pipe-key.fingerprint` (host `ssh-keygen -l`).
  6. `P:no-password-auth`: zero `Accepted password`, `Failed password`, `Invalid user` lines.
  7. `P:neg-ab-no-connection`: zero `Connection from` guest lines between `neg-ab-begin` and `neg-ab-end`; `P:neg-c-no-accept`: no `Accepted` and no `Starting session` between `neg-c-begin` and `neg-c-end`. [INFERENCE] guest-uart lines are time-ordered with host lines (the existing `D:` splits rely on it).
  8. `P:one-session-per-connection` (existing rule), `P:completion-records`: exactly the expected records for markers 4, 6, 7a, 7b (`mode=exec`, `outcome=remote_exit`, `remote_status=0`, `error_code=null`).
  9. `P:no-secret-in-any-evidence-file` / raw serial: extended secret list; `P:no-blob-or-key-text-in-driver-output` is covered by (2) and the scan.
- `ARCHIVE_FILES` gains `pipe-blob.json`, `pipe-key.fingerprint`.
- Tests (host, no QEMU): serial fixtures for pipe PASS and for every flip (missing/duplicate marker, digest differs by one nibble, length differs, wrong fingerprint in sshd log, an `Accepted password`, a connection inside `neg-ab`, an `Accepted` inside `neg-c`, KMS banner absent, `[policy] loaded` present/absent per D-GP-4, secret in a file, stage `full` unchanged by the edit, stage `diag` unchanged). The existing 98 stay green.

### 3.5 Guest edits

`prepare-tier3-sshd-initramfs-x86.sh`: when `T3_PUBKEY` (path of the `.pub`) is set, write a second sshd_config (`PubkeyAuthentication yes`, `PasswordAuthentication no`, everything else as the default block), stage `--add /etc/t3-authorized_keys <pub> 100644`, and make the audit tolerate exactly that extra non-secret entry; unset: the script's staged set and texts are unchanged.
`tests/guests/x86-sshd/guest-init.sh`, after `adduser tester`: if `/etc/t3-authorized_keys` exists: `mkdir -p /home/tester/.ssh; cat … > …/authorized_keys; chown -R tester:tester /home/tester/.ssh; chmod 700 …/.ssh; chmod 600 …/authorized_keys; rm -f /etc/t3-authorized_keys; say PUBKEY_AUTHORIZED` (`mkdir/cat/chown/chmod` rather than `install -o/-g`, whose busybox support is not verified here). `sshd -t` already guards the config. `StrictModes` is satisfied by `adduser -D` (home 755).

### 3.6 Why no product change is needed (and what could still go wrong)

The K6b path is exercised unchanged. The first-observation risks are all *environment*, listed in §11; two are *product-relevant* observations the lane can surface and must be recorded rather than hidden: full-duplex pumping of 512 KiB through `drive_external_sink` with `cat` echoing (marker 7; deadlock would be a product finding), and the KMS sign bound (`SSH_SIGN_TICKS` 6,000, a placeholder bound, `k6-cp2-dossier.md` §6) under TCG with a nested guest competing for CPU.

## 4. Claim map (pre-registered; a marker FAIL is evidence, not an invitation to adjust)

| Outcome | Claim allowed |
|---|---|
| 8/8 PASS, judge exit 0, `stop_rule` null | exact non-UTF-8 stdin and pipeline exec for publickey exec to the guest at the ceiling (marker 6), byte-exact stage stdout through a redirect (marker 7), the password limitation shown as refusals (marker 5) |
| marker 7 FAIL only | stdin claim stands (marker 6); no stdout-redirect claim; product finding recorded; no rerun |
| marker 6 FAIL | no claim; CBX-23 wording keeps "not proven for the guest"; finding recorded |
| marker 3/4 FAIL with `kms_denied`/`kms_unavailable` and the `[kms]` banner present, before any network effect | lane-shape defect (policy-less), eligible for the one confirmation boot with the loaded-policy shape (§7, D-GP-4) |
| anything else | `MARKER-FAIL`: no fix-and-rerun without a new CP1 |

Nothing here is claimed for password mode, wrapped keys, first-trust confirmation in a stage, PTY, `--timeout`/cancel of a stage, rekey during a stage, larger-than-1 MiB heads, production custody, canonical/promoted/candidate bytes, Intel VMX.

## 5. Judge/driver/lane edit summary

See §3.1, 3.3-3.5 and the closure in §6. Lane script edits in detail: (L1) usage/header text; (L2) `STAGE` accepts `pipe`; `OUT_NAME` `pipe -> t3-pipe/run`, `b0-N` with `TIER3_PIPE_B0=1`, `confirm` with `TIER3_T3P_CONFIRM=1`; (L3) the preflight `BAD` regex gains the new closure paths; the 18 pins stay and a second pin block checks the six product tree digests of §6.3 (stage `pipe` only); (L4) `claim_token` gains `T3P_*` names with the same O_EXCL rules and the existing "confirm only if the stage token exists" rule; `--claim-only` guard unchanged; (L5) key/blob/conf generation block (stage `pipe` only); (L6) build block adds the KMS and admin invocations and `DRIVER_FEATURES=guest-pipe`; (L7) `CELL_BINARIES`/`CELL_IMAGE_PATHS` and `FAT_ARGS` gain the KMS, admin and `/t3p/*` entries; the services.toml patch adds the KMS row and `after`; (L8) `grep -Eiq "sshd"` outer-image guard unchanged (`kms-ssh-admin` contains no `sshd`); (L9) supervisor python: stage `pipe` ends on `driver-terminal` like `full`, STOP-7 applies, `window` default 1800 (cap 3600); (L10) judge call `pipe`; archive gains the two files; (L11) cleanup removes `$W/t3p`.

## 6. Exact closure (SHA-256 at HEAD `952d84c8a`)

### 6.1 Existing files edited (lane/test/guest only)

| Path | SHA-256 now | Edit |
|---|---|---|
| `scripts/qemu-tier3-ssh-guest-x86.sh` | `8b3a8b0cb440e8fd92cfcfe19a5bc58d8121ed302564fb6bab2bcf2cea1fcb45` | §5 L1-L11 |
| `scripts/tier3_ssh_guest_judge.py` | `93bbcb2d44f5d09050e00fec759c0f245a2f908555bd058598ddfcd64ef376b4` | §3.4 |
| `scripts/test_tier3_ssh_guest_judge.py` | `994db398f075616b366db398236c7dd99d8fdd2aae44196f63ccbef26eb55249` | §3.4 tests |
| `scripts/prepare-tier3-sshd-initramfs-x86.sh` | `e95c327ca6e67bd4b3d0dd187bd3e16daaa1f069cb79c82ef4d21e893d2017d4` | §3.5 |
| `tests/guests/x86-sshd/guest-init.sh` | `6ff396d99dbd71d41b7e1e9b67f03f0774f60af6398dc648a390f24e3f53def0` | §3.5 |
| `cells/tests/phase04-terminal-driver/Cargo.toml` | `b959b11a6c270a59c5f01bcb54487bc5618fd99fdb43a25df5bde41dacc3706d` | feature `guest-pipe = ["guest"]` (no dependency) |
| `cells/tests/phase04-terminal-driver/src/ssh.rs` | `756dee495eb760004914a03c821681d51c34224670e1bbcab02c0e6a204afdd4` | after the `guest::wait_ready` block (`:195`): `#[cfg(feature = "guest-pipe")] { guest::pipe_checks(a, &f); return; }`; the S9-3 group and `guest_checks` are guarded `not(guest-pipe)` |
| `cells/tests/phase04-terminal-driver/src/ssh/guest.rs` | `dc4ce10454932f532273599c8b452209e23f6141c0856b935d32ab223388a727` | `pipe_checks` (§3.3) under `#[cfg(feature = "guest-pipe")]`; `guest_checks`/`diag_checks` get `cfg(not(guest-pipe))` so default `guest`/`guest-diag` builds are the same code |
| `CHANGELOG.md` | `c5ef0411d34cd77667a91ca71f8d937fe8ff819d3c22228e036a6ede3f6aa5db` | one entry, at CP2 |

Where another landing edits the same file first, the second landing re-hashes (checked with peers FuPhase02, FuSvc, the owner-audit agents and FuReap: none edits these paths). **One coupling (FuReap, reap-and-rst-cp1.md):** the lane's 18-line pin block (`qemu-tier3-ssh-guest-x86.sh:135-154`) hashes four Net files (`cells/services/net/src/{service-runtime.rs 89d9334a…, socket_table.rs 1964bc9d…, socket_table/tests.rs 28529be3…, service-runtime-owner-death-tests.rs 3c12780f…}`). If the reap slice lands first, the lane refuses until those four pins are refreshed deliberately (a stage-`pipe` lane edit would then re-pin them, and the CP2 must state that the evidence carries the new Net bytes); if this program lands first, nothing changes for it. Recommended order: GP first (it needs no Net change), or re-pin in the same lane edit with the new hashes recorded.

### 6.2 New files

None required (the key/blob/conf generation is inline in the lane python block, as the existing keys are). Evidence goes under `.agents/261110-0900-tier3-guest-endpoint/evidence/t3-pipe/{b0-1,b0-2,run,confirm}/` (small text files only: serial log, argv, verdict, `pipe-blob.json`, fingerprints, fat layout, provenance; no key, no blob content). Tokens under `evidence/t3-d-runs/` next to the existing ones. Records: `.agents/261110-1200-cellbox-followups/guest-pipeline-cp2-dossier.md` at CP2.

### 6.3 Read-only inputs pinned (any drift is a STOP)

Existing 18 lane pins (all re-verified OK today with the lane's own `sha256sum -c` block): unchanged. Tree digests (formula of the lane's `protected_digest`: `git ls-files --cached --others --exclude-standard DIR | LC_ALL=C sort | xargs sha256sum | sha256sum`), stage `pipe` preflight:

| Tree | Digest |
|---|---|
| `cells/tools/cellbox` | `94779b69aefdd5aa56bf583dd46f3657edcd5ffa8da1653115d4d7ddd695f756` |
| `cells/tools/kms-ssh-admin` | `f718d358e85b7a2c459c4cf8eef6b6e30a0f9c58cabca9b7323f57402b5c52a8` |
| `cells/services/kms` | `460bdf66cbf9d73e2be76f11d48d15cbd7761abefa5c2b67c7c09215a19c3769` |
| `libs/terminal` | `de3317176e14caa5f1dc3a02f41d44acd92af85e7ea718bb70c950e1317ec109` |
| `libs/vissh` | `f032373672fd16c4142f06e754353ae38df0ce512cf5c5b1f3888e896029dc5d` |
| `libs/ostd` | `7b6b98d06c4a9fffbe677a8bc8c96e6a793065b576778d927db37323d50761bf` |

Key single files behind them: `cells/tools/cellbox/src/ssh.rs` `b71ebc459312c34e4282c93bc5140206b43033181b68fe2fb5414929ec170401`, `typed.rs` `526b367cac5fa09ed22487de36c3259efac87dfd633f877bb1de8b64c9acf49a`, `executor.rs` `083f1a6c0107418d6013cbb52234f5527feb4debb5c89a80b827c49b2c4dd2a5`, `cmd_fs.rs` `dabfb5279842f183c7acda8a8ef700f4da90e30e4832915cc71dea35a97e398a`; `libs/terminal/src/pipe_child.rs` `50e7478dc4cc84f5f2747afc716ccbb61f108290bae3abb01462ccdf5515f485`; `libs/vissh/src/session.rs` `a3d51d33b057c2cfb9f61b02394391693722a85934b479bad475dedd292368b1`, `libs/vissh/src/userauth.rs` `78c5bbb87a567d7be778754f08816bbac6a6d5164f4338e1afb2257441ce1afb`; `libs/ostd/src/clients/kms.rs` `129e18dde03366610fef64fd8f0d18b9f81c55b3a0dcf6f9461179b7b717ed76`, `…/kms/ssh.rs` `68433b8e6cf4eea011c245bd801755cf47b497d173889017a54f5bf4db286587`; `kernel/src/policy.rs` `c16896d908e426faf0fcc73febed18dde81c097b6cd07a2eb4fb1a17b3ae558e`, `kernel/src/task/cap.rs` `7d7bae8cc71ae176f046a7560602256cbd00a22585d16b2fbad255cc26a5a651`, `launch_profile/profiles.rs` `78058780bad2549c67ab7487b2a1e741d52580cbbdd27d1fccef132a69aa2930`. Reused unchanged: `cells/tests/phase04-terminal-driver/src/main.rs` `b661277daadeadd43904302a31bd9546028b6717bb2bbecd03f03574bdf16ca3`, `…/ssh/kssh.rs` `7474e8b9761c62b24c0499050b7d2e0bb1d5cc6e0bfc37bd9ad09ef6e3288a50`, `scripts/phase09_trust_disk.py` `10901b916c948efd62b632e2144e1a3ab519ce8487d324f87e42c60390e6e5e1`, `scripts/qemu-phase09-ssh-test.sh` `620b735d404037b79c883fde5272e0f1c3cd11c4432ac786003f031ccc071dc2` (template only), `scripts/sign-policy.py` `7e2c2cc7cb3fbd30d36d40525128fdc7f917dadf65daf7df6106ddee24b10203`, `tools/mkfat32.py` `b82c284a714ed2f546783085da035328d351b37e712624924b27eb5826500565`, `tools/inspect_fat.py` `a41dfad334ac7f459fa71f010aebea9b82e43eb0b5a3e0e8daf533044d745031`, `tools/repack-initramfs.py` `dd9cd07b51afc93b1891537e04cde10c6fca936676a7ce35e1f03d21a4ff6ff1`, `tests/guests/x86-sshd/apk-pins.sha256` `4edae50a39fe265492f1c22ee4d312ad83c3faf9e6ad925719573ee408db8870`, `scripts/fetch-tier3-sshd-apks-x86.sh` `80e02e105f228e7b07c84c0ef534ca19159431b9ff97f0f19767f4cdfbdaf841`, `Cargo.lock` `5181251ccc712f27f71db9861b1b4feab0a8fec910e43716a7b744a4f8c81130`. Also unchanged and protected: the rest of `kernel`, `hal`, `libs`, `cells/services`, `cells/tools`, `config`, `scripts/cellos_sign`, `scripts/x86`, `tools` (before/after tree digests recorded by the lane's existing `protected_digest`; at HEAD: `kernel` `ecef424d3e79e604cb67af622625a570675090e0133d630653744abd917b60fd`, `hal` `8c01b510ab7fa30d7d7f2abe550093406143463fde692650c0a72000403ebab6`, `libs/api` `5417fe1feda2c15c20269c6815ba54b3822ea118adf3a6e62a73bd1c7fb3a9ba`, `libs/types` `ef871bea1313be755ee840f66974c84144dd41900df19f9859048d8ad3070e81`, `cells/services/net` `6832054b013130953af4ae56f86b3cb3103c8190e787c0264751e384995583c6`, `cells/services/vfs` `96a14c9c4c3ad9d2d121f5648b7da3b2768b3c7916c889a7cbe88945b41256c9`, `cells/services/hypervisor` `ed50798b94a154cecc85d80065173db7a484b311d72ce5d3e6e6404500a9cb4e`).

### 6.4 Documents edited at CP2 only (after evidence)

| Path | SHA-256 now |
|---|---|
| `.agents/261020-1750-cellbox-cli/acceptance.md` (CBX-23 row) | `43f03a4f38c737a7b282808574e999494434c547fcc8a5aede9a6edb7082e38b` |
| `.agents/261020-1750-cellbox-cli/phase-09-ssh-client.md` (`:101` item) | `fce05ad752ad2903141695331ee80e06c46ca09c755714880401a30bdfa7368e` |
| `.agents/261020-1750-cellbox-cli/phase-10-qualification.md` (criterion 1, `:31`; closure text `:388`) | `a016a1d5745924692edf139420cab957f2a71ee1d9081b960c2272bf88215068` |
| `.agents/261020-1750-cellbox-cli/phase-10-s0-proposal.md` (`:3043` bullet) | `01f2d442eb27d3a208928212cb8c011c7d6d7a5f44f2f32d6cb2d7df8a6e82e3` |
| `.agents/261020-1750-cellbox-cli/plan.md`, `verification.md`, `reports/rollout-containment.md` | `ff465746a621146fafef5173b8792202ae78aa7670a0614cfea9eb4d25bdbef1`, `77147ebd4fb5c9a9b4c18d5c039be68d02aa13ac884a3b686c16d3d158551c98`, `f92f0d96d340551988c7ff011e03a55698838d40fa82fa89f798068f9de17836` |
| `.agents/plan-portfolio.md` (`:129`, `:472-473`) | `d1e89ceb1c0433627fa68b4122e2127ff6fee6b9e6a734e669503c347a42ddd8` |
| `docs/cellbox-cli.md` (`:10` SSH bullet; it still says publickey is refused and the guest is blocked, stale since K6/K8 and T3: a records fix independent of this program's outcome) | `28ac3c3ab9834e04c207a53cdcd2ce9ad94e600da9e741b490616c16a6e3057f` |
| `.agents/cell-contract-0024-locks.md`, `command-catalog.md` (`f06f38ce…a5fd0f`), `command-contracts.md` (`4ff1c3be…2090d`) | not edited |

## 7. Verification, run budget and tokens

| Gate | What | Budget |
|---|---|---|
| G1 | `bash -n` on the three shell files; `python3 -m py_compile` on judge and tests; `python3 -m unittest scripts.test_tier3_ssh_guest_judge` (98 today + new, all green) | seconds |
| G2 | `--claim-only` token tests against a scratch `TIER3_TOKEN_DIR` under `/tmp` (the existing mechanism): `T3P_PIPE_CONSUMED-1` claims once and a second claim refuses; confirm without the stage token refuses; `T3P_B0_CONSUMED-1/2` claim in order and a third refuses; the real token directory is untouched | seconds |
| G3 | `python3 scripts/cellos-sign --check` rc 0 (F5/F1; no new unsafe: nothing is edited in Rust under `cells/`/`libs/`) | seconds |
| G4 | `--stage pipe --build-only` (no QEMU, no token): builds the KMS and admin cells, the driver `guest-pipe`, the image; asserts the FAT layout has `/bin/kms`, `/bin/kms-ssh-admin`, `/t3p/*`, the KMS row in `services.toml`, no `sshd` in the outer image/config | cargo build minutes (cold) |
| G5 | offline blob/property tests and the judge fixtures of §3.4 | seconds |
| G6 | prepare-script regression: with a fixed scratch `$KEYS`, run the default mode before and after the edit; `output_sha256` and every staged file hash in `audit.json` equal [INFERENCE: the repack is deterministic for fixed inputs; if it is not, compare the staged non-secret entries only] | minutes |
| R | **Boots (QEMU 10.2.0 TCG, one loopback hostfwd)** | see below |

Boots: **≤ 2 rehearsal boots** (`TIER3_PIPE_B0=1`, tokens `T3P_B0_CONSUMED-1`, `-2`, evidence `t3-pipe/b0-N`, **no evidence value**, only lane/judge/driver fixes between them), then **1 evidence boot** (`--stage pipe`, token `T3P_PIPE_CONSUMED-1`, evidence `t3-pipe/run`), then **at most 1 confirmation boot** (`TIER3_T3P_CONFIRM=1`, token `T3P_CONFIRM_CONSUMED-1`, only for a host/environment error or a recorded lane/judge/driver instrument defect whose raw data already satisfy the criteria, or the loaded-policy switch of §4 row 4; never a relaxed product criterion; evidence `t3-pipe/confirm`). Total ≤ 4 boots, expected evidence boot 1-4 min [INFERENCE from the 45-165 s full/diag runs plus KMS and about 1.2 MiB through nested TCG]; window default 1800 s, hard cap 3600 s (no 4 h window is needed or requested). A product-result FAIL on the evidence boot ends the program for that marker (no fix-and-rerun). Resource tokens in the CP2 sense: no Docker, no network (the apk set, QEMU 10.2.0 and Alpine kernel files are cached and pinned; `fetch-tier3-sshd-apks-x86.sh` is not run), no host listener beyond QEMU's loopback hostfwd port, no key outside `$W`.

STOP rules (additions to the lane's STOP-1..7 of the T3 CP1/rerun): STOP-P1 KMS/admin cell did not start or the `[kms]` banner is absent (image/lane defect → counts as a rehearsal result only); STOP-P2 any pipe key, seed, password or blob-private datum in any artifact or raw serial → stop, scrub, archive nothing (STOP-5); STOP-P3 any protected digest or `Cargo.lock` change; STOP-P4 `[net] owner death watch unavailable` (STOP-7); STOP-P5 marker FAIL on the evidence boot (`MARKER-FAIL`, §4).

## 8. Alternatives considered

| Option | What | Product change | Verdict |
|---|---|---|---|
| **A (recommended)** | lane-only guest proof of the **publickey** stage; password mode stays refused by design | none | proves exactly what the product supports; honest residual |
| B | the same, but run inside `full` after `guest_checks` | none | rejected: `T3D3_FULL_CONSUMED-1` is spent; changes the sshd config of the record stage; mixes password and publickey accounting |
| C | make password mode carry exact stdin: a foreground child that keeps the terminal for the password and takes a file-fed stdin (`ssh --auth password … --command C < FILE`), or a shell-prompted password handed to a stage | cellbox `typed.rs`, `executor.rs`, `ssh.rs`; `libs/terminal` `session.rs`/`pipe_child.rs`; every cellbox ELF changes; `GrantSlot` lives in `libs/ostd` (`child_context.rs:11`), not Law-1, but the foreground context layout (`libs/terminal/src/session.rs:344` `foreground_context_len`) must carry Stdin next to Control [INFERENCE: not read in depth]; a shell-held password crossing into a stage contradicts `command-contracts.md:136` and the no-secret-in-pipe design of K6 | **not recommended**; contingent packet §10.3 only |
| D | do nothing; keep "retained" wording | none | the wording is stale for publickey even now (K8), so at least the records fix of §9 is due |
| E | prove the guest with a wrapped key | needs a passphrase prompt, impossible in a stage | not possible by design (K6b §3.4) |

## 9. Row wording that would follow (applied at CP2 only for outcomes the judge passed)

**CBX-23, replace the sentence "S9-3 pipeline-exec and exact non-UTF-8 stdin limitation is retained at this ceiling by the same disposition (not proven for the guest)" with (all 8 PASS):** "**Exact non-UTF-8 stdin and pipeline exec (S9-3 deviation 3), re-opened for publickey:** `cat FILE | cellbox ssh --auth publickey --identity KEY_ID … --command C` passes the upstream bytes unchanged with one EOF and byte-exact stdout/stderr (K6b; other host: K8 S15 RT0/RT1 PASS, `DEV_POSTURE` plain KMS identity, 4,096 B; Tier-3 guest: GP program, `evidence/t3-pipe/run`, 512 KiB blob with NUL/`0xFF`/invalid UTF-8, remote SHA-256 and length equal to the host's, stdout round trip equal, four `Accepted publickey` with the key's fingerprint, no password login) at the SVM/TCG ceiling (OpenSSH in the nested guest, one loopback hostfwd, private lane-only image, QEMU-TCG 10.2.0), plain KMS identity at `DEV_POSTURE`. **Remains refused by design (not a defect):** password mode as a pipeline stage or with a redirect (`` `ssh` is an interactive session ``), exact non-UTF-8 stdin in password mode (typed text only), wrapped keys and unknown hosts in a stage; not claimed: Intel VMX, KVM, production custody, cancel/timeout/rekey of a stage, heads above 1 MiB." The status word of the row is not decided by this wording (it follows the user's disposition of the row, currently set by "Tier-3 update 2").

If marker 7 FAILs only: same text without "stdout round trip equal" and with "stdout of a redirected stage on the guest: not proven (finding …)". If marker 6 FAILs: keep "not proven for the guest" and add the finding. If the program is not run: still correct the other-host half (K8) in the same sentence (records fix D).

**Phase 10 criterion 1 / closure text (`phase-10-qualification.md:31`, `:388`):** replace "S9-3 pipeline-exec/non-UTF-8 stdin limitation retained" by "S9-3 pipeline-exec/non-UTF-8 stdin: met for publickey (other host K8, guest GP at the SVM/TCG ceiling); password mode refused by design". `phase-09-ssh-client.md:101` and `phase-10-s0-proposal.md:3043`: strike "pipeline exec and exact non-UTF-8 stdin … stay open", keep "publickey half of CBX-24 …" as already dispositioned by the KSSH updates. `docs/cellbox-cli.md:10`: replace the stale clause "`--auth publickey` is refused; the Tier-3 guest endpoint and publickey/KMS custody stay blocked" by the dispositioned state (publickey at `DEV_POSTURE`, guest at the SVM/TCG ceiling, password pipelines refused by design). `command-catalog.md` is digest-bound and not edited (D-T3-10 precedent).

## 10. Locks, Law-1, protected paths

### 10.1 Law-1

No byte of `libs/api` or `libs/types` is edited; no syscall, capability, launch edge, policy row or ABI constant is added or changed (the edges for `/bin/cellbox-ssh` and `/bin/kms-ssh-admin` and the policy rows exist, K3/K7). No Law-1 confirmation is requested. The `kms` service row and `service_id = 13` are the K8 lane's existing private services.toml patch, not a repository config change.

### 10.2 Locks for the recommended option A (no handoff packet)

Locked paths touched: **none**. The closure edits files under `scripts/`, `tests/guests/x86-sshd/`, `cells/tests/phase04-terminal-driver/` (the private test cell) and records. Built read-only (no edit): `cells/services/kms`, `cells/tools/kms-ssh-admin` (K5/K7 adopted slices) and the product trees pinned in §6.3. **Acknowledgement text requested for `guest-pipeline-approval.md`:** "The accountable owner (the user) acknowledges that the GP program builds and boots, in a private lane-only image under QEMU-TCG, the already-implemented KSSH KMS (`ssh-dev-custody`), the K7 admin cell and the unchanged `cellbox ssh` against OpenSSH in the SVM guest; edits no file under the scope of relations L11, L13, L18, L25, L32, L36, L37 or any KMS plan, consumes completed outputs as evidence only, adopts, narrows or executes no locked plan, and changes neither Intel VMX/EPT P09 status nor the KSSH tiers. The reading that a private lane consuming already-landed product bytes needs no six-step unfreeze (`cell-contract-0024-locks.md` procedure, steps 1-6) is accepted for this slice only." Overlap notice: the KMS paths are named by `260825-1726-kms-silo-production-root` and `260826-1605-phase4-dev-reference-authority` (K5/K4b paragraphs, `locks.md:119,123`); nothing in them is edited.

### 10.3 Contingent lock handoff for option C (DRAFT for the accountable owner; **not requested, not recommended**; form of `net-owner-death-cp1.md` §11)

**C.1 Slice identity.** "GP-C: password-mode exact stdin" — a Cellbox bounded slice (single integrating owner of `.agents/261020-1750-cellbox-cli/plan.md`, status `closed-partial-private-evidence`); serves CBX-23; not an Intel C2C outcome; `DEV_POSTURE`. Re-opens no closed phase.
**C.2 ADR-0024 delta.** A foreground child would hold a Stdin grant next to its Control grant; no new principal, capability or handle crossing; the password stays in the child's no-echo lease; no secret enters the stream. [INFERENCE] Whether the kernel's child-context validation accepts Stdin+Control together was not read.
**C.3 Locked rows.** None of the closure paths is named by `cell-contract-0024-locks.md` (0 hits above); the Cellbox plan row 14 of `lock-adoption-draft.md` is the owner basis ("Cellbox plan (single integrating owner); libs/ostd is not Law 1", as used by K6). Overlap notices: `libs/terminal` is also used by the GUI Terminal (P04 evidence), and `libs/ostd/src/child_context.rs` by every launch path: start-of-slice file-claim re-check.
**C.4 Closure (pre-edit SHA-256):** `cells/tools/cellbox/src/typed.rs` `526b367c…acf49a`, `executor.rs` `083f1a6c…2dd2a5`, `ssh.rs` `b71ebc45…170401` (and its `ssh/tests.rs` `fa1e03c9…02a9c`), `libs/terminal/src/session.rs` `e82eb9f3532ae116fe135c3a216eea215e05bbade2767ea6140a05fcd4f00dea`, `libs/terminal/src/pipe_child.rs` `50e7478d…485`, `libs/terminal/src/tests.rs` `4c723bf2…2d52`, possibly `libs/ostd/src/child_context.rs` `3e6847006d2d02cc615d3155087142f340b46041e95842d6ac9802eb878d5433`, plus `CHANGELOG.md`. Consequences: every cellbox ELF changes, so the lane pin `ssh.rs b71ebc45…` and the six tree pins break, the matched candidate/promoted bytes `4fae8b20…`/canonical stay without it (re-promotion is a separate decision), K8/L5/L5f and the P04 terminal lane (494/494 baseline) must be re-run, and the guest lane re-pinned.
**C.5 Retained gates.** Law-1 not triggered; contract text `command-contracts.md:136` and the K6 §3.5 refusals ("password mode as a pipeline stage") would need an owner-approved amendment (the catalog digest `f06f38ce…` is user-acknowledged, S10-5 CP2).
**C.6 Decisions requested from the owner (if ever).** Adopt GP-C; amend the contract wording; accept the re-qualification cost. Nothing in this packet authorises a source edit.

## 11. Risks and unknowns (stated, not hidden)

| # | Risk | Mitigation / handling |
|---|---|---|
| R1 | Absent policy + KMS rights never observed together [INFERENCE: read from `policy.rs:1065-1125`] | §4 row 4: one confirmation boot with the K8-proven loaded v8 blob (`scripts/sign-policy.py --out`, `[policy] loaded + verified (59 entries`); `/bin/hypervisor` has a DEV_POLICY row (`sign-policy.py:139`), the shape is otherwise untested with this hypervisor |
| R2 | CPU contention: nested guest sshd + client + KMS sign under one TCG; `SSH_SIGN_TICKS` 6,000 is a placeholder bound (K6 O-K6-3) | rehearsal boots measure it; a `kms_timeout` is a recorded product observation, not a retune |
| R3 | Local redirect of 512 KiB (`> /tmp/p09-rt.bin`, 400-byte VFS writes, ≈1.3 k IPC) and `/tmp` RamFS capacity never exercised at this size in a lane [INFERENCE: quota default 32 MiB, `quota.rs:27`] | marker 7 is separate from marker 6 (§4) |
| R4 | Full-duplex pumping (`cat` echoing while the client still sends) never exercised (K8 sent to `cat > file`) | observation, §3.6; a deadlock is a product finding and ends marker 7 |
| R5 | guest-uart vs host serial interleaving for the connection-window checks of §3.4 (7) | the existing `D:` accounting relies on the same ordering; judge defect → confirmation boot rule |
| R6 | `sshd` StrictModes / locked-account behaviour with `PasswordAuthentication no` and a chpasswd'ed account [INFERENCE] | rehearsal; `sshd -t` and the `Accepted publickey` line are the oracles |
| R7 | busybox `chown tester:tester`, `cat >` availability in the pinned initramfs [INFERENCE: both are standard applets, `guest-init.sh` already uses `adduser`, `chpasswd`, `install`] | rehearsal |
| R8 | Image size/arena: `/bin/kms` bss ≈ 1 MiB (`k8-cp1.md` §5.2), FAT 71 MB today, cellbox heap 6 MiB holds the 512 KiB `Vec` plus feed | rehearsal; `--build-only` prints sizes |
| R9 | The lane preflight refuses a dirty tree outside its closure and pins product bytes: any concurrent product edit (other programs) stops the run | land the closure alone; run from a clean tree; re-pin deliberately |
| R10 | The blob is fed as one `Vec` (≤ 1 MiB head): this program says nothing about streaming heads above that | stated in §4 |
| R11 | A plain unwrapped key in volatile KMS is a `DEV_POSTURE` credential; the claim must carry that label | §9 wording |

Not checked in this text: that the guest-lane build with `-p service-kms --features ssh-dev-custody` links under the lane's other feature unification (K8 built the same way); `ssh-keygen` accepting the throw-away key for KMS import on the host version in use (K8 used `ssh-keygen -N ''` plain `kp0`); busybox `sha256sum`/`wc` output formats in the pinned initramfs (`p9 bulk` already uses `sha256sum`); that the 512 KiB redirect and `cat` head stay within the shell's per-command time bound.

## 12. Effort

Lane/judge/driver/guest edits and tests about one working day; G4 cold build several minutes; rehearsal and evidence boots minutes each; records at CP2 half a day. Entry gates: this CP1 approved; clean tree; `scripts/cellos-sign --check` rc 0; QEMU 10.2.0 pinned binary and the apk set present (the lane preflight checks both).

## 13. Not authorised by this document

Any source edit under `cells/`, `libs/`, `kernel/`, `hal/`; option C; a re-run of `full` or `diag`; Docker; any network fetch; KVM; a canonical, promoted or candidate image; production custody; edits to acceptance rows or phase records before CP2; commit, push or tag by agents.

## 14. Decisions (OPEN until the user answers; recommendation in the last column)

| ID | Question | Options | Recommended |
|---|---|---|---|
| **D-GP-1** | How to treat the limitation | **A** prove the publickey stage on the guest (this CP1), keep password refused by design; **B** only correct the wording (other host K8 evidence), no run; **C** open the product slice of §10.3 | **A** (B alone leaves "not proven for the guest"; C is cost without a user need) |
| **D-GP-2** | New stage vs extending `full` | **A** new `--stage pipe`, tokens `T3P_*`; **B** extend `full` (needs new `T3D3_*` tokens and changes the record image) | **A** |
| **D-GP-3** | Run budget | **A** ≤ 2 rehearsals + 1 evidence + ≤ 1 confirmation (≤ 4 boots, cap 3600 s); **B** 1 evidence + ≤ 1 confirmation; **C** more | **A** (the KMS-in-guest-lane combination is new; boots are minutes) |
| **D-GP-4** | Policy shape of the pipe image | **A** no `/POLICY.BIN` (the existing guest-lane shape; rights from edges), switch to **B** only through the confirmation rule; **B** loaded v8 blob from the start (K8's shape) | **A** |
| **D-GP-5** | Guest sshd password login in the pipe image | **A** `PasswordAuthentication no` (no password anywhere in the stage); **B** keep `yes` | **A** |
| **D-GP-6** | Blob size | **A** 512 KiB; **B** 128 KiB; **C** 1 MiB minus a margin (the head bound) | **A** |
| **D-GP-7** | Marker 7 (stdout round trip) required for the full class | **A** required, its failure only voids the stdout claim (§4); **B** drop it | **A** |
| **D-GP-8** | Records fix of the stale CBX-23/phase-10/`docs/cellbox-cli.md` wording for the other-host publickey half | **A** apply at CP2 regardless of the run outcome; **B** only if the run passes | **A** |
| **D-GP-9** | Lock reading and acknowledgement text of §10.2 | **A** accept; **B** require a formal slice adoption first | **A** |
| **D-GP-10** | On a marker FAIL on the evidence boot | **A** record, no fix-and-rerun without a new CP1; **B** allow one extra boot | **A** (same rule as K8, T3-D, T3-D3) |
