# GP dossier: publickey pipeline exec and exact non-UTF-8 stdin for `cellbox ssh` against the Tier-3 guest sshd

Status: **executed under the approved CP1** ([guest-pipeline-cp1.md](guest-pipeline-cp1.md), approval [approval.md](approval.md) item 2). **Evidence boot result: `MARKER-FAIL` (marker `ssh-guest-pipe-roundtrip` only; 7 of the 8 markers PASS).** No product file edited. Date 2026-10-30. Run HEAD `c0ae115dc062e8120b4e0eb4776c6c0eb310c0d1` (the CP1 base `952d84c8a` plus docs-only commits).

Ceiling (CP1, verbatim in spirit): Linux guest running OpenSSH on the existing AMD SVM VMM under QEMU-TCG 10.2.0 (`qemu64,+svm`), reached from a Cellos cell by one loopback-only QEMU `hostfwd`, private lane-only image. Not Intel VMX/EPT, not KVM, not production, not matched-candidate or promoted bytes, no remote or C2C use. The credential is a **plain (unwrapped) KMS-held ed25519 key at `DEV_POSTURE`** (volatile `ssh-dev-custody`).

## 1. What was done

Lane-only closure (CP1 §6.1), nothing else touched (`git status` outside `.agents/` lists exactly these eight paths):

| Path | SHA-256 before (CP1 §6.1, re-verified at start) | SHA-256 after |
|---|---|---|
| `scripts/qemu-tier3-ssh-guest-x86.sh` | `8b3a8b0cb440e8fd92cfcfe19a5bc58d8121ed302564fb6bab2bcf2cea1fcb45` | `cab760151d35d82e0015b9344b6976f6b342fbebd35f75b34bd9535ffbddb603` |
| `scripts/tier3_ssh_guest_judge.py` | `93bbcb2d44f5d09050e00fec759c0f245a2f908555bd058598ddfcd64ef376b4` | `5f5de879f88b817fdf5992dad9632eb64c78b0fa882776a1f60848556b03d2aa` |
| `scripts/test_tier3_ssh_guest_judge.py` | `994db398f075616b366db398236c7dd99d8fdd2aae44196f63ccbef26eb55249` | `f7135293d0be783511f802139b135d4bf13eb419ca2e837a84fb6ed122810ad9` |
| `scripts/prepare-tier3-sshd-initramfs-x86.sh` | `e95c327ca6e67bd4b3d0dd187bd3e16daaa1f069cb79c82ef4d21e893d2017d4` | `de46476e4ad6a8bc05f1607ea7116d3aaf3692e44a38123589926e3b9f846788` |
| `tests/guests/x86-sshd/guest-init.sh` | `6ff396d99dbd71d41b7e1e9b67f03f0774f60af6398dc648a390f24e3f53def0` | `4e69ca6b44b9093776445779a1c0e2f27ec12657ea4d2a54597164be5f8c6b22` |
| `cells/tests/phase04-terminal-driver/Cargo.toml` | `b959b11a6c270a59c5f01bcb54487bc5618fd99fdb43a25df5bde41dacc3706d` | `14af10d75a50a9d0b406ee3623ab1d80989ff536414316cd361bf14f30803bef` |
| `cells/tests/phase04-terminal-driver/src/ssh.rs` | `756dee495eb760004914a03c821681d51c34224670e1bbcab02c0e6a204afdd4` | `1b3f1f9ef2e5fe12aa909313fab9fb84b0948406466f28e13377b389a4c2b67f` |
| `cells/tests/phase04-terminal-driver/src/ssh/guest.rs` | `dc4ce10454932f532273599c8b452209e23f6141c0856b935d32ab223388a727` | `8ab32452d417fa9d4036e14959f7fb610d6d16337d92f06c84d010e94d6da490` |

`CHANGELOG.md`: not touched (main agent). The lane and judge bytes are identical in both boot tokens (`lane_sha256=cab76015…`, `judge_sha256=5f5de879…`).

What the edits are:
- **Lane** `--stage pipe [--build-only]` (additive; `boot`/`full`/`diag` code paths and tokens untouched): builds `service-kms --features ssh-dev-custody` and `app-kms-ssh-admin` each in its own `cargo` invocation with `CELLOS_PRODUCTION` unset, the driver with `--features guest-pipe`; image gains `/bin/kms`, `/bin/kms-ssh-admin`, `/t3p/{blob.bin,pipe.key,pipe.conf}`, the K8 `kms` services row (`service_id = 13`) and `kms` in the driver's `after`; **no `/POLICY.BIN`** (D-GP-4 A); throw-away ed25519 key and a 512 KiB blob (`00 00 ff ff` head; `C0 AF`, `ED A0 80`, `F8 88 80 80 80`, `FE FF`, `80 BF`; `03`, `04`, `0D 0A 0D`, `1B 5B 41`, `7F`; 4,096 NUL run ending and 4,096 `FF` run starting at the 64 KiB boundary; tail `00 FF 0A 00`; asserted non-UTF-8, NUL and `0xFF` present); preflight second pin block of the six product tree digests (CP1 §6.3, all OK); tokens `T3P_PIPE_CONSUMED-1`, `T3P_B0_CONSUMED-1/-2` (`TIER3_PIPE_B0=1`), `T3P_CONFIRM_CONSUMED-1` (`TIER3_T3P_CONFIRM=1`, only after the stage token exists), all O_EXCL under `evidence/t3-d-runs/`; evidence root `.agents/261110-1200-cellbox-followups/evidence/t3-pipe/{b0-N,run,confirm}`; window default 1800 s, cap 3600 s; STOP-7 and driver-verdict end as for `full`.
- **Judge** `pipe` subcommand (`P:` checks, `STOP-P1` for a missing/duplicated `[kms] DEV_POSTURE ssh-dev-custody` banner, MARKER-FAIL otherwise), `--blob` (the judge recomputes sha256/length/UTF-8 validity/NUL/`0xFF` itself and writes `pipe-blob.json`), `--policy-shape absent|loaded`, pipe private key added to the secret scan.
- **Guest**: `T3_PUBKEY` switches the staged `sshd_config` to `PubkeyAuthentication yes` / `PasswordAuthentication no` and stages `/etc/t3-authorized_keys`; `guest-init.sh` installs it for `tester`.
- **Driver** `guest-pipe` feature (no new package, `Cargo.lock` unchanged): `ssh_checks` splits the reused S9-3 group into `s93_checks` (not part of the `guest-pipe` build); `guest::pipe_checks` runs markers 3-7 (secret discipline of `kssh.rs`: key read with `cat`, pasted at the echo-off prompt, transcripts cleared and scanned for the key's body lines).

## 2. Verification (offline gates exactly as CP1 §7)

| Gate | Result |
|---|---|
| G1 | `bash -n` on the three shell files OK; `py_compile` OK; `python3 -m unittest scripts.test_tier3_ssh_guest_judge`: **117 tests OK** (the 98 existing plus 19 new pipe tests: marker missing/duplicate/FAIL per marker, extra marker, final verdict, digest differs by one nibble, length differs, round-trip digest differs, blob property flips, wrong fingerprint in the sshd log, logins count, `Accepted password`/`Failed password`/`Invalid user`, connection inside `neg-ab`, `Accepted`/`Starting session` inside `neg-c`, missing windows, one session per connection, completion records, KMS banner 0/2, `[policy] loaded` present/absent per shape, secrets as STOP-5, STOP-7, scan-hit, window cap, boot checks, `full` unchanged, CLI, archive) |
| G2 | `--claim-only` against scratch `TIER3_TOKEN_DIR`s under `/tmp`: `T3P_PIPE_CONSUMED-1` claims once and a second claim refuses; confirm without the stage token refuses; confirm after it claims `T3P_CONFIRM_CONSUMED-1` once, a second confirm refuses; `T3P_B0_CONSUMED-1`, `-2` claim in order, a third refuses; a rehearsal after the stage token refuses; the pipe variables are refused for stage `full`. The real token directory was untouched by G2 |
| G3 | `python3 scripts/cellos-sign --check` rc 0 (`OK: F5`, `OK: F1`, 134 crates / 928 files, 67 allowlisted files; no Rust under `cells/`/`libs/` other than the private test cell was edited). Log `evidence/t3-pipe/logs/g3-cellos-sign-check.txt` |
| G4 | `--stage pipe --build-only` rc 0: 18 file pins OK, 6 tree digests OK, `cellos-sign --check` rc 0, apk pins OK; FAT layout has `/bin/kms` (170,408 B), `/bin/kms-ssh-admin` (124,296 B), `/t3p/{blob.bin (524,288 B),pipe.key,pipe.conf}`, boot-config `services.toml` has the `kms` row, driver `after = ["input","net","kms"]`, no `sshd` in the outer image, no `POLICY.BIN`. First attempt failed to compile (`pub(super)` re-export of `pipe_checks`), fixed to `pub(crate)`, second build OK. Log `logs/gp-g4.log` |
| G5 | blob/property tests: the 19 judge tests above plus `pipe_blob_facts` on the real G4 blob (len 524288, `utf8_valid` false, NUL and `0xFF` present, digest equals `pipe.conf`, the offsets above hold) |
| G6 | prepare-script regression with a fixed scratch `$KEYS`, HEAD script vs edited script, default mode: **every staged entry of `audit.json` is byte-identical except `bin/virtio-e2e-init`**, which differs by design (the 11-line authorized_keys block in `guest-init.sh`, +623 bytes), so `output_sha256` is not equal (CP1's "[INFERENCE: compare the staged non-secret entries only]" branch applies). `T3_PUBKEY` mode differs by exactly `etc/t3-authorized_keys` (added) and `etc/t3-sshd_config`, `problems: []`. Audits in `logs/gp-g6-*.audit.json` |

## 3. Boots (QEMU 10.2.0 pinned, TCG, one loopback hostfwd)

| Boot | Token | Evidence | Result |
|---|---|---|---|
| Rehearsal 1 (`TIER3_PIPE_B0=1`) | `T3P_B0_CONSUMED-1` | `evidence/t3-pipe/b0-1` | judge exit 0, **8/8 markers PASS**, all `P:` checks PASS, driver verdict at 34 s (elapsed 37.0 s). **No evidence value** (CP1 §7). Rehearsal 2 not needed, not used |
| **Evidence boot** | `T3P_PIPE_CONSUMED-1` | `evidence/t3-pipe/run` | judge exit 1, `stop_rule` **`MARKER-FAIL`**, driver verdict at 30 s (elapsed 33.0 s) |
| Confirmation boot | `T3P_CONFIRM_CONSUMED-1` | - | **not run** (see §5) |

Evidence boot details (`evidence/t3-pipe/run/{serial.log,verdict.json,timeline.json,pipe-blob.json,pipe-key.fingerprint,...}`, 21 text files; no key, no blob content; secret scans PASS; the key, ISO, images and `$W/t3p` were deleted by the lane and verified absent afterwards):

PASS: B1, B2, C1, C2, argv invariant, pins, no-sshd-in-outer-image, both secret scans, protected paths unchanged, `run-ended-by-driver-verdict`, `window-within-pipe-cap`, `net:no-owner-death-watch-unavailable`; markers `shell-prompt`, `ssh-conf`, **`ssh-guest-pk-import`** (KMS import of the throw-away key, `custody: plain`, DEV_POSTURE warning, fingerprint equal), **`ssh-guest-pk-exec`**, **`ssh-guest-pipe-refusals`** (password pipeline refused status 2 with no connection and no prompt; unknown identity `identity_unknown`, no connection; unknown host `host_key_no_frontend` after key exchange only, no `Accepted`), **`ssh-guest-pipe-stdin-exact`** (remote SHA-256 `b63006467ea413fd5cb50022d6d40ae1aeeface0325eda920cd95ae6ebca79f2` and length 524288 equal the judge's own recomputation from `$W/t3p/blob.bin`; status 0, no prompt, empty stderr; `Transferred: sent 2044, received 545696` at the guest sshd), `session-a-close`; `P:digest-stdin-equals-host`, `P:blob-is-non-utf8-with-nul-and-ff`, `P:kms-banner-once` (exactly one `[kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf`; `[policy] no /POLICY.BIN in VIFS1 — absent`), `P:no-policy-loaded-line`, `P:sshd-accepted-publickey-equals-logins` (4 `Accepted publickey for tester ... ED25519 SHA256:WmGOsuHh...` all equal to the run key's host fingerprint), `P:no-password-auth`, `P:neg-ab-no-connection`, `P:neg-c-no-accept`, `P:one-session-per-connection`.

FAIL: `ssh-guest-pipe-roundtrip` (hence `P:markers`, `P:driver-final-verdict` = `FAIL failures=1 checks=8`, `P:digest-roundtrip-equals-host`, `P:completion-records` = 3 clean exec exits instead of 4). Inside it, the first stage (`cat blob | ssh ... --command cat > /tmp/p09-rt.bin`) passed (status 0, `remote_exit` 0, guest `Transferred: sent 533824, received 545944`), the second stage (`cat /tmp/p09-rt.bin | ssh ... --command sha256sum`) failed after 1,997 ms with status 255, `ssh: connection lost; the command may have run`, completion `phase=running dispatch=accepted outcome=unknown error_code=transport_lost`; the guest sshd logged, after `Starting session`, **`Bad packet length 2270388607.`** and **`ssh_dispatch_run_fatal: Connection from user tester 10.0.2.2 port 43252: Connection corrupted`**.

## 4. Claim map result (CP1 §4, pre-registered)

Row "marker 7 FAIL only" applies: **the stdin claim stands (marker 6 PASS); no stdout-redirect claim; product finding recorded; no rerun.**

Allowed: `cat FILE | cellbox ssh --auth publickey --identity KEY_ID ... --command C` delivered a 512 KiB blob (NUL, `0xFF`, invalid UTF-8, terminal-control bytes) to the guest unchanged with one EOF at the ceiling above: remote SHA-256 and length equal the host's, status 0, no prompt, stderr empty, `Accepted publickey` with the key's fingerprint, no password login; the password pipeline forms were refused (status 2, `` `ssh` is an interactive session ``) before any connection.

Not claimed: byte-exact stdout of a stage on the guest and the 512 KiB round trip (marker 7), password mode, wrapped keys, first-trust in a stage, PTY, `--timeout`/cancel/rekey of a stage, heads above 1 MiB, production custody, canonical/promoted/candidate bytes, Intel VMX.

### Finding F-GP-1 (product or platform; cause not determined; recorded, not tuned)

In the second consecutive 512 KiB stage of marker 7 the guest sshd decoded a corrupt packet from the client (`Bad packet length`, `Connection corrupted`) about 2 s into the connection and the client reported `transport_lost`. The same sequence passed in the rehearsal boot (`b0-1`, same lane/judge/driver bytes, 8/8). One failure in one evidence boot: no rate can be stated. A length-field decode failure means the ciphertext stream to the server was corrupted or desynchronised (client encryption/sequence state under flow control, the Cellos Net/e1000/smoltcp path, or the slirp/TCG environment are all consistent with the data; nothing here separates them). The evidence boot is spent and STOP-P5 / D-GP-10 forbid fix-and-rerun; isolating it needs a new CP1 (e.g. repeat stage 7b N times in one boot with the slirp-netdev capture of the T3-D2 diag mechanism and the client-side byte counters).

### Observation F-GP-2

`shell: pipeline operation close failed: WouldBlock` is printed after every pipeline stage in both boots, including the stages that PASS (the `completion` record of those stages is clean). Not judged; not part of any claim; recorded for the owner of the shell pipeline code.

## 5. STOP rules applied

- STOP-P5 (marker FAIL on the evidence boot, `MARKER-FAIL`): applied literally; no fix-and-rerun, no rerun after the product outcome.
- The confirmation boot is allowed only for a host/environment error, a recorded lane/judge/driver instrument defect whose raw data already satisfy the criteria, or the loaded-policy switch (§4 row 4). None holds: KMS import and publickey login worked policy-less (no `kms_denied`/`kms_unavailable`), the judge and driver behaved as specified, and the raw data do not satisfy the marker-7 criterion. It was therefore not used (`T3P_CONFIRM_CONSUMED-1` does not exist).
- STOP-P1 (banner), STOP-P2 (secret), STOP-P3 (protected digest/`Cargo.lock`), STOP-P4 (`owner death watch unavailable`): not triggered.
- Tree state: every dirty path outside `.agents/` was inside the closure when each boot started (P2Marker's four paths were committed by main before my G2).

## 6. Deviations from the CP1 text

1. **HEAD** moved from `952d84c8a` to `c0ae115dc062` by docs-only commits (two follow-up CP1 drafts, approvals, P2Marker); all CP1 pre-edit hashes matched at start.
2. **Evidence root** is `.agents/261110-1200-cellbox-followups/evidence/t3-pipe/` (task instruction) instead of the CP1 §6.2 `261110-0900-tier3-guest-endpoint/evidence/t3-pipe/`; the **tokens** stay under `261110-0900-tier3-guest-endpoint/evidence/t3-d-runs/` (`T3P_B0_CONSUMED-1`, `T3P_PIPE_CONSUMED-1` exist). Dossier name per task: `guest-pipeline-dossier.md` (CP1 §6.2 said `guest-pipeline-cp2-dossier.md`).
3. **Preflight dirty-tree regex**: CP1 L3 said it "gains the new closure paths"; none was needed, the lane's existing allow-list already contains every closure path (`.agents/*`, `CHANGELOG.md`, `tests/guests/x86-sshd/*`, the three scripts, the three driver files); unchanged.
4. **Round-trip digest line** carries no length (`P04 INFO ssh-guest-pipe-digest which=roundtrip sha=... len=-`): the CP1 stage command is `sha256sum`, which cannot report a length; the judge compares the SHA-256 (equality over the 524,288-byte blob implies the length). The stdin line has both.
5. Marker 5(a) command line is `cat /t3p/blob.bin | ssh --user tester --auth password -p PORT 10.0.2.2 --command cat` (CP1 elided the host/command); 5(b)/(c) use `--auth publickey --identity nope|t3k` so that they are stages, as the CP1's own prose requires.
6. **Additions beyond the CP1 text, inside the closure**: judge `--blob`/`--policy-shape` options, `TIER3_T3P_POLICY=loaded` in the lane (the CP1 §4 row 4 switch; never exercised), `P:driver-started`, `P:boot-ceiling-selftest-pass`, `P:trust-store-volume-mounted`, `P:no-weak-entropy`, `P:fixture-kernel-features-recorded`, `P:no-secret-scan-hit-in-driver`, `window-within-pipe-cap`; the `ssh.rs` edit is the `s93_checks` split (a cfg'd function) instead of an early `return` (avoids unreachable-code warnings).
7. G6: `output_sha256` equality is impossible by construction (the init script changes); see §2.
8. `provenance.sha256` of stage `pipe` additionally lists `service-kms`, `kms-ssh-admin` and `blob.bin`.

## 7. Recommended row wording (text only; the main agent edits the rows)

The outcome is the CP1 §9 "marker 7 FAILs only" case. D-GP-8 A applies regardless (records fix of the other-host half).

**`.agents/261020-1750-cellbox-cli/acceptance.md`, CBX-23 row.** Replace exactly the sentence

> the S9-3 pipeline-exec and exact non-UTF-8 stdin limitation is retained at this ceiling by the same disposition (not proven for the guest).

with (this is also the retained-limitation sentence for the rest of the row's history):

> **Exact non-UTF-8 stdin and pipeline exec (S9-3 deviation 3), re-opened for publickey, partly met on the guest (GP program, `261110-1200-cellbox-followups/evidence/t3-pipe/run`, [dossier](../261110-1200-cellbox-followups/guest-pipeline-dossier.md), judge `MARKER-FAIL` on one marker):** `cat FILE | cellbox ssh --auth publickey --identity KEY_ID … --command C` passes the upstream bytes unchanged with one EOF (other host: K6b, K8 S15 RT0/RT1 PASS, 4,096 B; Tier-3 guest: 512 KiB blob with NUL/`0xFF`/invalid UTF-8, remote SHA-256 and length equal to the host's, four `Accepted publickey` with the key's fingerprint, no password login) at the SVM/TCG ceiling (OpenSSH in the nested guest, one loopback hostfwd, private lane-only image, QEMU-TCG 10.2.0), plain KMS identity at `DEV_POSTURE`. **Not proven on the guest (finding F-GP-1, no rerun by STOP-P5):** byte-exact stdout of a redirected stage and the 512 KiB round trip (the second consecutive stage ended `transport_lost` with the guest sshd logging `Bad packet length`/`Connection corrupted`; the rehearsal boot passed it; cause undetermined). **Remains refused by design (not a defect):** password mode as a pipeline stage or with a redirect (`` `ssh` is an interactive session ``), exact non-UTF-8 stdin in password mode (typed text only), wrapped keys and unknown hosts in a stage; not claimed: Intel VMX, KVM, production custody, cancel/timeout/rekey of a stage, heads above 1 MiB.

The status word of the row is not decided here (it follows the owner's disposition).

**`.agents/261020-1750-cellbox-cli/phase-10-qualification.md` criterion 1 (`:31`) and closure text (`:388`).** Replace "S9-3 pipeline-exec/non-UTF-8 stdin limitation retained" by: "S9-3 pipeline-exec/non-UTF-8 stdin: met for publickey stdin (other host K8; guest GP at the SVM/TCG ceiling, 512 KiB); stdout round trip of a stage on the guest not proven (finding F-GP-1); password mode refused by design".

**`phase-09-ssh-client.md:101` and `phase-10-s0-proposal.md:3043`.** Strike "pipeline exec and exact non-UTF-8 stdin … stay open" and write "publickey stdin met on the guest (GP, 512 KiB); stage stdout round trip on the guest open (F-GP-1); password-mode pipelines refused by design"; keep "publickey half of CBX-24 …" as already dispositioned.

**`docs/cellbox-cli.md:10` (records fix D-GP-8, independent of the run; stale since K6/K8 and T3).** Replace the line with:

> - **SSH (Phase 09, partial):** password, known-host and confirmation to another host work; `--auth publickey --identity KEY_ID` works with a plain key held by the key service at `DEV_POSTURE` (other host: K8; Tier-3 guest at the SVM/QEMU-TCG ceiling: password login and, for `cat FILE | cellbox ssh --auth publickey … --command C`, exact 512 KiB non-UTF-8 stdin proven, the stdout round trip of a stage on the guest not proven, finding F-GP-1); the Tier-3 guest endpoint is met at that ceiling after NET-OD; password mode as a pipeline stage or with a redirect is refused by design, as are wrapped keys and unknown hosts in a stage; not Intel VMX, not KVM, not production custody (CBX-23/24/25 partial).

`plan.md`, `verification.md`, `reports/rollout-containment.md`, `.agents/plan-portfolio.md` (`:129`, `:472-473`): one clause each with the same facts (publickey stdin met on the guest; stdout round trip open, F-GP-1; password pipelines refused by design). `command-catalog.md` and `command-contracts.md` are not edited. CHANGELOG (main agent), one entry: "tier3: pipeline stage of the guest lane (`--stage pipe`, KMS `ssh-dev-custody` + K7 admin cell + throw-away key, publickey-only guest sshd): exact 512 KiB non-UTF-8 stdin to the guest proven; stage-stdout round trip failed once (F-GP-1, no rerun); no product change."

## 8. Follow-ups the owner may decide (none started)

1. A new CP1 to isolate F-GP-1 (repeat the round-trip stage in one boot, slirp-netdev capture, client byte counters, a control with a 64 KiB and a 128 KiB blob), including whether it is the client, the Net/e1000 path or the slirp/TCG environment.
2. The `WouldBlock` close message (F-GP-2) belongs to the shell pipeline owner.
3. Option C (password-mode stdin) remains not recommended and not started.

## 9. Cleanup

Deleted: per-run keys (`$W/keys`, including the throw-away pipe key and the private copy `/t3p/pipe.key`), `tier3.iso`, `trust-fixture.img`, `embedded/kernel_fs.img`, the guest initramfs, `$W/t3p` (blob), scratch G2/G6 directories. Kept: private build caches `cells-target` and `kernel-target` under `/tmp/cellbox-tier3-261030a` (no keys, no images) and the text logs archived in `evidence/t3-pipe/logs/`. Evidence tree: `evidence/t3-pipe/{b0-1,run,logs}`.
