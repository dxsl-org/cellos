# KSSH K8 CP2 dossier, part 1: development, H0 and B0 (T0 lane)

Implemented by `K8Impl` from `k8-cp1.md` (SHA-256 `5366add4...17e0`, approval `k8-cp1-approval.md` with its Docker and F1 amendments; D8-1..7 as recommended). Written 2026-10-30. HEAD while running: `b66c5211e`, then `c513e60a8` (Main's "F1 gate decision recorded" commit, which also swept earlier work-in-progress versions of my files into history; nothing was committed or pushed by me). **Scope of this part: development + H0 + B0 only. No RT0 / RT0f evidence boot was run and no `RUN_CONSUMED-k8-*` token exists.** Text evidence: `reports/k8-evidence/` (listing in section 9).

## 1. What was built (SHA-256 at the freeze, all listed in `k8-evidence/frozen-sources.sha256`)

| Path | State | Lines | SHA-256 |
|---|---|---|---|
| `scripts/qemu-phase09-ssh-test.sh` | edited: opt-in `--kssh [--faults] [--set b0\|rt0\|rt0f] [--calibrate]`; G-S1a baseline pin for all modes | 777 | `1c573d9160e816751f1bc5da35032fd701cb80939f6eb081c572ab833837f7fb` |
| `scripts/phase09_ssh_fixture.sh` | edited: `start DIR --kssh AUTHDIR` (3 servers, per-server `AuthorizedKeysFile`, `StrictModes no`, `LoginGraceTime 900`, never builds, absent image exits 3) | 191 | `07c6747139a41edc686c96c1d3f2763b5092dcc5eece8ec724970526e5eb8b50` |
| `cells/tests/phase04-terminal-driver/{Cargo.toml,src/main.rs,src/ssh.rs}` | edited: feature `kssh`, dispatch, `pub(crate) mod kssh;` (one deviation: `pub(crate)` so `main.rs` can name it) | +3 / +6-3 / +1 | `8040b18e...`, `fc24450c...`, `efca2f77...` |
| `cells/tests/phase04-terminal-driver/src/ssh/kssh.rs` | new, all T0 + F scenarios, calibration | 1,408 | `156297d238c5d723528b696c5a2674e573de231ccd1dfccd4970eec318a3684a` |
| `cells/tools/cellbox/{Cargo.toml,src/ssh.rs}` | edited: `kssh-faults = []`, cfg-gated seam | +7 / +31 | `9a130f44...`, `b71ebc45...` |
| `cells/tools/cellbox/src/ssh/faults.rs` | new, seam + 13 host tests | 381 | `1313a4ffba6d34a0a7e9caa83d99f43708c05a76ffa5b84dc355cebb68b66709` |
| `scripts/kssh_keys.py` | new | 552 | `c306a319d96f2bb904ff2870806f6dddbb54a655628830a28650287540be2c2c` |
| `scripts/kssh_judge.py` | new | 968 | `b08bad10c284772aac095dc2fa9710f672d79dd91b894b542eb614586ec7c605` |
| `scripts/test_kssh_judge.py` | new, 100 tests | 1,331 | `75f6148138ef28652c6ace657cc2c929190b4da9c931185894a9d09f72fc0b24` |
| `CHANGELOG.md` | one entry | +2 | `a2429a8e2e16b1b3782ab97776ccd0cb84d4f8341823733ae67dfd330ca67678` |

Outside the closure but needed by the run: `cells/tools/kms-ssh-admin/src/{main.rs,tests/armor.rs}` were fixed by K7Impl on Main's decision (crate root `forbid(unsafe_code)`, test token); hashes `a43854af...` / `da1ac944...`, tree digest `f718d358...a52a8` (was `cf48e7f3...`). Every other frozen tree digest equals `k8-cp1.md` section 4.3 (`frozen-trees.txt`). `Cargo.lock`, kernel, KMS, `libs/**`, assembler, signing, canonical bytes: byte-identical (`frozen-sources.sha256` verifies; `git diff --stat` of those paths empty). S10 census 470/470 equal before and after (`census-before/after(-compare).json`).

Division of labour (for the record): driver, judge/keys and faults seam were written by three sub-agents against the interface file `local://k8-contract.md`; runner, fixture, integration, all runs by me.

### 1.1 Runner shape (kssh mode)
Preflight (`preflight.txt`: HEAD, K3-K7 ancestors, dirty files restricted to the K8 closure + the two K7 files, dirty digest, `CELLOS_PRODUCTION` unset, Docker + pinned image present else exit 3, no stray container/QEMU, 8 GiB free, B0 budget `B0_BOOT-<variant>-<n>` O_EXCL files (max 3 per variant), `frozen-sources.sha256` verified for evidence sets) -> ceiling gate with `--baseline-ref 2f0bc06599808be3ab0e4b3df0d79ea08c5317aa` -> throw-away cell key -> builds in `$W` (service-kms `--features ssh-dev-custody` and app-kms-ssh-admin each in their own cargo invocation, cellbox `""` or `p09-faults,kssh-faults`, driver `--features kssh`) -> TEST keys (`kssh_keys.py gen`, fresh per run) -> fixture `--kssh` -> `kssh.conf` -> private F1 allowlist (section 5) -> sign -> unchanged `sign-policy.py` blob as `/POLICY.BIN` + `/kssh/*` + `kms` services row -> FAT/trust disk/kernel (L5 features, no `policy-required`)/ISO -> QEMU version/hash check, census before, token claim -> boot (L5 serial protocol, window 7,200 s, TCG only) -> fixture stop -> census after -> `kssh_judge.py judge` -> `archive` (allowlist copy + canary scan with controls) -> deletion of keys, stage, ISO, trust disk, kernel_fs.img. Exit status = judge (0 only for class A); exit 3 = not a run.

## 2. H0 results (`h0-gate.txt`, run 2026-10-30)

| Item | Result |
|---|---|
| `test_kssh_judge.py` | **100 OK** (24 s); `kssh_judge.py selftest` 6/6 |
| `test_cellos_sign.py` / `test_assemble_x86_image.py` | 71 OK / 40 OK (frozen baselines unchanged) |
| `test_phase10_canonical_drive.py` / `test_phase10_candidate_import.py` | 27 OK / 18 OK (the guards of the two edited scripts) |
| `bash -n` runner + fixture | OK |
| `sign-policy.py --out` | 2,169 B, `4ee6f63ea8211b6c05c70d2fe8d18114e556692412e81ca596a836ebcb56c579` (= K7 dossier), round-trip gates pass |
| cargo host tests | `app-cellbox` **226** (default; equals baseline), **239** with `kssh-faults` and with `p09-faults,kssh-faults` (+13 seam tests); `app-kms-ssh-admin` 71; `service-kms --features ssh-dev-custody` 122 + 1 ignored; `service-kms` default 65 |
| `cellbox-ceiling-diff.py --baseline-ref 2f0bc0659...` | PASS (`G-S1a`) |
| Bare-metal builds | driver `kssh` / `phase09-ssh` / none, cellbox x4 feature sets: OK (cellbox 3 pre-existing warnings; driver `kssh` text/data/bss 216,903 / 2,928 / 1,048,648); kms, admin, init, kernel: built by every B0 run |
| C-host control | done against the real OpenSSH_9.9p2 fixture, results in `c-host/` (`strings.txt` table); the inferred wording was right for `Accepted publickey`, `Postponed publickey`, `Failed publickey`, `Connection closed by authenticating user ... [preauth]`; `StrictModes no` + bind-mounted key files work; no harness change needed beyond the judge's pattern table |

## 3. B0 results

Five boots, all TCG/QEMU 10.2.0 (`/home/dmin/.cache/cellos/qemu-10.2.0`, sha256 `169fb4ea...241fe`), about 110-190 s each (a whole T0 run ends in ~3 minutes). Budget used: I-T0 3 of 3, I-F 2 of 3. Per-boot excerpts: `smoke-b0.txt`; full text under `b0-*/`.

| Boot | Variant | Class (no evidence value) | Cause / fix |
|---|---|---|---|
| `b0-it0-1` | I-T0, calibrate | C | kssh-s06d FAIL (see 3.2), calibration keys not authorized on the server (cal-R1 probe-rejected, `kdf-error`), judge expected the wrong ledger count (1 vs 3) and did not know `session-a-close`. All harness issues; fixed (judge, keys, driver), none a product finding |
| `b0-it0-2` | I-T0, calibrate | **A** (57 checks) | |
| `b0-it0-3` | I-T0, calibrate, final code | **A** (58 checks) | the frozen state |
| `b0-if-1` | I-F | **A** (45 checks) | |
| `b0-if-2` | I-F, final code | **A** (47 checks) | the frozen state |

Before B0 boot 1, two runs stopped before QEMU (not counted, no token): Docker down (started by the user's authorization), then F1 refusal at signing (section 5), then a runner bug (stale `$KEYS` between runs) fixed.

### 3.1 What the boot logs prove at B0 level (no evidence value, but the shape the RT sets will repeat)
- KMS starts: `[kms] fail-closed node identity service starting`, `[kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf` (the feature build is the one that boots); `[policy] loaded + verified (59 entries, flags 0x00)`; `[selftest] boot-ceiling: PASS`; both new cells spawn (`/bin/kms` via the services row, `/bin/kms-ssh-admin` via the shell edge: `kssh-s01a` `status` reports `readiness: Ready, keys: 0`, so it received flag 7). The loaded v8 policy leaves the lane healthy (policy fallback not needed); it narrows only P-TRUST rows of nvme/e1000/input (`[policy] narrowed ...`).
- Admin import path reachable: import of `ka16` by paste, `fingerprint:` equals the host `ssh-keygen -lf` value and the blob prefix equals the host-computed prefix (`kssh-obs s01 fp=... prefix=...`, judged), delete confirmation word not echoed (`delete-word-echo echoed=0`), refusals S01b/S09 as the plan.
- Login with a KMS-held key: `Accepted publickey ... ED25519 SHA256:<FP_A>` exactly once per accepted attempt, no password lines; wrong passphrase, Escape, unknown identity, unauthorized key, limiter (5 failures then `unlock_locked` for the correct passphrase), pipeline stage cases and all three connection classes were observed by the judge's connection-sequence oracle.
- I-F: S05a/b -> `kms_input_mismatch`, S05c -> `kms_bad_signature`, no signature reaches sshd (`probe-ok` connections only), control login accepted, S11 rekey `ssh-rekeys 64`, sink 1,048,576 bytes with the zero-MiB digest `30e14955...fcb58`, `peak_pending=2048 peak_chunk=16384`.
- Secrecy: driver self-scan `transcripts scans=52 hits=0 canaries=50`; host scan of serial, sshd log, trust disk with the full canary set (879 needles in `b0-it0-3`, all forms) found nothing, with positive controls passing (`controls.json` in each `b0-*` dir).

### 3.2 Findings of the B0 rehearsal (reported, none hidden)
1. **S06(d) was mis-specified, not a product finding.** The typed `/bin/cellbox ssh ...` is stripped by the shell itself (`oneshot.rs`: word `/bin/cellbox` is `Prefixed::Strip`), so it is the normal ssh path: the passphrase prompt is shown (`s06d-typed form=prompt-shown`, one `probe-ok` connection). The driver now (a) launches `/bin/cellbox` directly from the driver with argv `ssh ...` (the same ELF at a non-ssh path, from a non-shell caller) and observes `s06d-form=kms_denied` with no sshd connection, which is the K3-deferred proof "a shell-path copy does not carry flag 6", and (b) keeps the typed form as an observation (`s06d-typed`).
2. **KDF on the TCG target (K5 D-5.8, K6 bounds).** Typed-Enter to prompt-return of a wrong-passphrase attempt (stage + KDF + AES + failure + exit): rounds 1: 63-72 ms, rounds 16: 244-267 ms, rounds 64: **865 ms** (`b0-it0-2`: 865, `b0-it0-3`: 865), rounds 4: 92-104 ms; locked answer (no KDF) 45-54 ms. Host reference (K5): 4.70 / 74.15 / 294.97 ms at 1/16/64, so TCG is about 3-13x host for the whole interval. K5 acceptance (<= 10 s at rounds 64) is met by a factor above 11; rounds 16 and 4 are far inside `SSH_SIGN_TICKS` (60 s) and the Auth budget; a whole ssh attempt takes about 3 s end to end; commit/status/delete frames were well within `SSH_FRAME_TICKS` (5 s) in every run (no `kms_timeout`). KMS loop stall length is the KDF interval above (single-threaded; not otherwise measured).
3. The kernel selftest `[selftest] COMPLETION-QUEUE: FAIL — an older waiter erased its replacement` is printed in every boot. It is **pre-existing**: the same two lines are in the S10 L5 log (`s10-3b-evidence/rerun/lanes/default-p09-nofaults/serial.log`). It is not a K8 or K3-K7 defect and the judge does not gate on it; Main may want to know that the kernel baseline has a failing self-test.
4. `ssh-keygen -Z` works on this host (`-p -Z <cipher>`), so the two malformed key files are genuine aes128-ctr and chacha20-poly1305 containers; both are refused at import (`kssh-s01b`, format-refusal 572-583 ms each: no KDF).
5. The guest's source address at sshd is always the Docker gateway `172.17.0.1`, so connection grouping uses the source port only.

## 4. Scenario coverage in B0 vs the plan
All of S00, S01a, S01b, S01, S09, S10, S04a, S04b, S06a-d, S02, S02p, S03, S03b, S02', S08, S15 (I-T0) and S00, S01, S05a-c, S02, S11 (I-F) executed and passed in the final boots. S06b: the forged-trailer frame reached the service at the full 4,096 bytes (`s06b reach=full code=2`, i.e. `PermissionDenied`, so the kernel-written trailer overrides the forged one). S06c: both silent (flags set -> no reply) and the control frame answered `PermissionDenied`. Nothing in `[INFERENCE]` of CP1 section 20 remains open except those listed in section 7.

## 5. F1 admission gate: private per-run waiver (user/Main decision)
`cellos-sign --sign` runs the tree-wide F1 check and refuses at HEAD, for the stock L5/L5f lane too (reproduced on `git archive` of `2f0bc0659`, `492774edf`, `070f0d0a1`): `cells/tests/*` became git-tracked in `2f0bc0659` (2026-10-29), after the S10 lanes passed. Violations: `app-phase02-session-child`, `app-phase02-session-parent`, `app-phase03-filter`, `app-phase04-terminal-driver`, `app-top-load` (crate roots lack `forbid(unsafe_code)`, no allowlist entry), `service-input/src/session.rs` and `app-cellbox/src/rg/tests.rs` (`unsafe` keyword, no allowlist entry), plus K7's `app-kms-ssh-admin` (fixed in-repo by K7Impl). Decision (recorded in `k8-cp1-approval.md`): the runner passes `--allowlist $W/unsafe-allowlist.toml` in kssh mode only = the repo allowlist verbatim + 5 `[[crate]]` + 2 `[[file]]` entries (`review_by 2026-10-31`); the diff is `f1-waiver-unsafe-allowlist.diff`. **F1 is not satisfied by the tracked tree at HEAD; K8 images are signed under a private per-run waiver for these items.** The repo allowlist and signing code are untouched.

## 6. Problems met (all resolved unless stated)
Docker engine down in WSL (authorized start); F1 refusal (section 5); runner reused a stale `$KEYS` dir (fixed: fresh key dir per kssh run); judge expectations vs real driver behaviour (marker `session-a-close`, ledger counts 3/1+1, calibration keys, S06d ids, classes after the first sign epoch keep D); S06d redesign (3.2.1). One process deviation: a one-line runner edit (extending the dirty-file allowlist with the two K7 files) was made with a throw-away python replace instead of the edit tool; content is the same.

## 7. Limits that remain for the evidence boots (unchanged from CP1 section 12, plus)
No T1/reboot, no RG, `qemu64` software AES only, no RAM-residue scan, no `policy-required` posture, B0 numbers are not evidence. Not exercised here: serial/GUI paste and burst throughput of the import (R-4: paste goes through `FrontendHost::submit_paste`, which the B0 import of `ka16` shows working at ~600 B), TCG KDF variance across runs (only 2-3 samples per round count), the fixture on a different host. The K8 images carry the F1 waiver (section 5), a lane kernel (`phase02-terminal-profile,phase04-driver-profile`), a lane cellbox for I-F, and `/bin/kms`+`/bin/kms-ssh-admin` built from the landed trees: not the promoted bytes.

## 8. Go / no-go for the evidence boots
**GO**, subject to Main's authorization and with these conditions: (1) nothing under `frozen-sources.sha256` may change (the runner verifies it before building; a change is a new gate); (2) each evidence set runs exactly once, no rerun; (3) `docker` must be up with the pinned image present (else exit 3, not a run), QEMU 10.2.0 first in `PATH`, no stray QEMU/container; (4) the working tree may differ from HEAD only by the K8 closure and the two K7 files (runner preflight enforces it; committing them is fine).

Commands (each creates its `$W` first; token `RUN_CONSUMED-k8-<set>` is claimed with O_EXCL just before QEMU; output archived under `reports/k8-evidence/<set>/`; expect ~3-5 minutes each, window 7,200 s):
```
cd /home/dmin/cellos
mkdir -m 700 /tmp/cellbox-phase09-kssh-rt0
PATH="$HOME/.cache/cellos/qemu-10.2.0/bin:$PATH" CELLBOX_PHASE09_WORK=/tmp/cellbox-phase09-kssh-rt0 \
  bash scripts/qemu-phase09-ssh-test.sh --kssh --set rt0
mkdir -m 700 /tmp/cellbox-phase09-kssh-rt0f
PATH="$HOME/.cache/cellos/qemu-10.2.0/bin:$PATH" CELLBOX_PHASE09_WORK=/tmp/cellbox-phase09-kssh-rt0f \
  bash scripts/qemu-phase09-ssh-test.sh --kssh --faults --set rt0f
```
Exit 0 = class A; otherwise read `<set>/classification.json`. A KSSH_KEEP_SECRETS=1 environment variable keeps `$W/keys` etc. for inspection (default: deleted after the archive scan). Afterwards: delete `/tmp/cellbox-phase09-kssh-*`, re-run the census (`kssh_judge.py census`, done by the runner per set) and write part 2 of this dossier (classes, summary.json, evidence.sha256).

## 9. Files under `reports/k8-evidence/`
`preflight.txt`, `h0-gate.txt`, `c-host/` (client output, sshd.log, configs, strings table, public keys), `smoke-b0.txt`, `b0-it0-{1,2,3}/`, `b0-if-{1,2}/`, `B0_BOOT-*` budget markers, `census-before/after(-compare).json`, `frozen-sources.sha256`, `frozen-trees.txt`, `f1-waiver-unsafe-allowlist.diff`. No key, image, ISO or disk is in the repository; the B0 `$W` keys/ISO/trust disk/kernel_fs.img were deleted by the runner after each archive scan (the cargo target dirs and `evidence/` logs remain under `/tmp/cellbox-phase09-kssh-b0`; the C-host key dir was deleted).
