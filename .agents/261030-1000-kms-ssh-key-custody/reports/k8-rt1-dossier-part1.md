# K8 RT1 dossier, part 1: development, H0 and B0 (no RT1 evidence set; no `RUN_CONSUMED-k8-rt1` token claimed)

Authority: `k8-rt1-amendment.md` (SHA-256 `7ee815762db3be6a5d729e48c82d195ef2a559b03c35fca82e06fefa1a1a6a2c`) approved in `k8-rt1-approval.md` (D-RT1-1..7 as recommended). Written 2026-10-30 at HEAD `07c436ef4` plus the uncommitted RT1 closure. Nothing was committed or pushed. Scope of this part: lane development, H0, B0. The RT1 evidence set is **not** run; the command is in section 7.

**Ceilings that apply to every RT1 statement (K4b CP1 section 0):** (1) no rollback protection; (2) no crash or power-loss durability claim ("persisted" = `VfsRequest::Write` Ok plus an immediate same-boot byte-equal read-back; boots end by host `SIGTERM`); (3) the dev root MAC key is a public constant co-resident with the data; (4) confidentiality at rest is the passphrase and the bcrypt rounds only (TEST key); `Plain` is never written; (5) failure counter, unlock slot, import slot and replay floors are volatile; (6) the volume is the shared `/srv` CellosFS (a write failure poisons it for every user). Label `DEV_POSTURE`, never `ProductionQualified`. **F1 is not satisfied by the tracked tree at HEAD; K8 images are signed under a private per-run waiver for these items** (`app-phase02-session-child`, `app-phase02-session-parent`, `app-phase03-filter`, `app-phase04-terminal-driver`, `app-top-load`, `service-input`/`session.rs`, `app-cellbox`/`rg/tests.rs`); the repo allowlist is untouched and the runner checks that `cellos-sign --check` lists exactly those 7 items.

## 1. What was built (closure of amendment 4.1/4.2; all lane-only, no product byte)

| File | Change | SHA-256 (final) |
|---|---|---|
| `scripts/qemu-phase09-ssh-test.sh` | `--kssh --persist [--set b0\|rt1]` (variant `it1`, `service-kms --features ssh-dev-persist` in its own invocation, sibling checker on the signed `service-kms` (`checker.txt`: persist 0 / custody 1 / none 1 required), `kssh.conf` mode `t1`, `kssh_boots.py` in place of the single-boot step, T1 judge, archive); `frozen-sources-rt1.sha256` instead of the T0 freeze file; ancestors + `55a31fa97 86076fc66`; dirty allowlist + the four new scripts; extra preflight: W rules (rt1: exactly `/tmp/cellbox-phase09-kssh-rt1`, mode 700, not in the repo), tree digests before/after, service-vfs built without `test-hooks` (cargo fingerprint of the exact build), F1 = exactly the 7 known items + F5; per-set tokens `B0_BOOT-it1-N` / `RUN_CONSUMED-k8-rt1`; T0 modes keep their text and behaviour | `89b7d76062e0fd20741acde18e97e0bc764013b05362a824d060911af9bd0fc7` |
| `scripts/kssh_keys.py` | `gen`/`conf --mode t1`, record expectation in `expected.json`, `leak` needles, `public_expected` | `d4777e5eccc02d29b2318089e90dea6df3a254fd8671a3ce9553753dc34e3220` |
| `scripts/kssh_judge.py` | mode `t1` (per-boot oracles, cross-boot checks, record/disk oracles, classes), archive for t1 | `8f2e06e0eb506040d306cb45c7204178eaeffb287b1994890e8a4d8b2d0dd732` |
| `scripts/test_kssh_judge.py` | +69 T1 tests (100 T0/F tests unchanged) | `6576a4bf39749f7c0f3979c9f56340b918aea974310e0369eafd1db752396c65` |
| `cells/tests/phase04-terminal-driver/src/ssh/kssh.rs` | `mode t1`, roles 1-5, `kssh-obs` lines, S00 role read (97-byte command line at a five-digit port) | `7474e8b9761c62b24c0499050b7d2e0bb1d5cc6e0bfc37bd9ad09ef6e3288a50` |
| `CHANGELOG.md` | one entry | `5c2022ac045b81cae884e8204994d0ffd4f2b116633d4f1181d89ccb87dceeb1` |
| new `scripts/kssh_boots.py` (+ `test_kssh_boots.py`) | five-boot orchestrator: role via `docker exec`, T0 QEMU argv, SIGTERM + exit wait, settle rule, S1/S1r sparse copies, per-boot record oracle and whole-disk scan with controls | `651ebadb...2b2` / `9a39c93a...c2` (full values in `frozen-sources-rt1.sha256`) |
| new `scripts/kssh_records.py` (+ `test_kssh_records.py`) | independent Python decoder/encoder of the K4b record v2 (goldens equal the Rust ones), raw finder, `evaluate_set` | `e4524866...730` / `990c3d8e...c90` |
| `reports/k8-evidence/frozen-sources-rt1.sha256` | 78 lines (T0 freeze minus `CHANGELOG.md`, plus the new scripts, checker, allowlist, `tools/cellos-fs-provision`, the K4b KMS files, `docs/code-standards.md`); SHA-256 of the file `c56d06b0a55eefe9638b44b5a22113688544a92bdc3ad5ff4993145608b269f1`; every value I compared against the amendment's section 4.3 (Cargo.lock, policy.rs, kms `Cargo.toml`, `record.rs`, checker, allowlist, fixture script, provision tool, embedded `init`) is equal | |

Untouched (verified by the freeze, `git status`, the tree digests and the census): `Cargo.lock`, `kernel/**` (incl. `embedded-*` and `main.rs`), `cells/services/**` (KMS, VFS), `libs/**`, `cells/tools/**`, `tools/**`, `config/**`, assembler, signing, fixture script, S10 census (470/470 equal at start and at end).

## 2. H0

All green; details in `k8-evidence/h0-gate-rt1.txt`: judge 169 (100 + 69), records 49, boots 49, checker 13, guards 27 + 18, sign 71, assemble 40, `bash -n`, `kssh_judge.py selftest`, sign-policy blob `4ee6f63e...c56c579` unchanged; cargo host tests persist `--lib` 191 + 1 ignored, custody 122 + 1, default 65, cellbox 226, admin 71; bare-metal builds of `service-kms` persist and of the driver `--features kssh` finish.

## 3. B0 (3 of 3 sequences used; all class A, no evidence value; tokens `B0_BOOT-it1-1..3`; evidence `k8-evidence/b0-it1-1/`, `-2/`, `-3/`)

Run 1 reused the old T0-B0 work dir (incremental build, 334 s); run 2 a fresh work dir (cold build, 270 s total); run 3 the final frozen tree in a fresh `/tmp/cellbox-phase09-kssh-rt1` (it verified `frozen-sources-rt1.sha256` before and after the run and the tree digests; the directory was deleted afterwards). Each: five boots, `CLASS A`, 261 checks, 0 failed, `first_sign_epoch_passed` and `first_persistence_epoch_passed` true, archive scan clean with controls (860-866 needles), census 470/470, sshd five-boot totals as amendment 8.2.

**The first live `VfsBlobIo` run (amendment 14.1/14.2): it works; none of the failure signatures occurred.**
- Boot 1, blank `/srv` (only `.cellos-trust`): first `status` `readiness: Ready`, `keys: 0`, posture volatile (230-281 ms: `ensure_dirs` mkdir of `/srv/cellos` and `/srv/cellos/kms`, two absent-file reads, listing); `import ka16` (ct_len 2,032, the largest K5-allowed shape) answered `custody: passphrase-wrapped`, the host fingerprint and blob prefix, posture `persistent` (741-742 ms with write + read-back); no `PersistFailed`, no exit 124, no `[policy] narrowed "/bin/kms"` line, no panic or fault line; KMS kept answering. Stack margin remains unmeasured; only "the exercised paths did not fault" is observed.
- Reload (boots 2, 3, 5): first `status` `Ready`, `keys: 1`, persistent, `custody=wrapped`, fingerprint equal to the host's, 119-120 ms; boot 5 (replay of the end-of-boot-1 image) has facts identical to boot 2.
- Delete (boot 3): 983-1,053 ms, `keys: 0`, posture still `persistent`; boot 4 (tombstone boot): `Ready`, `keys: 0`, posture `volatile`, both identities `no such key`, both ssh attempts `no such identity`, no sshd connection. Boot 3's plain `kp0` import: custody plain, volatile, disk unchanged.
- Boot 3 login after boot 2's exhausted limiter succeeded (the counter does not persist: a limit); boot 2 limiter: five `unlock failed`, the sixth correct passphrase `key locked` (45 ms, no KDF).
- Boot 5 (S1r = copy of the end-of-boot-1 disk): the key deleted in boot 3 loads and signs again: rollback is not protected (as designed, shown).
- From the shell (boot 2): `cat /srv/cellos/kms/ssh-a.bin` exit 1 and no `CKMS`/ciphertext on the serial; `ls /srv/cellos/kms` exit 1 (`ls=error`, observation only).
- Timings (ms, three sequences): first-status 230/240/281, import 742/741/742, reload 119-120, delete 983/1,023/1,053; all well under the 5 s frame bound.

**Record at rest (host decoder, independent of the Rust code).** The layout caveat is settled: records are contiguous in the P5 image. After boot 1: exactly one valid record, **R1** slot A, at file offset 476,721,152 (P5 base 476,708,864 + 12,288), 2,281 bytes (217 + 2,032 + 32), present, name `ka16`, custody 2, rounds 16, revision 1, `previous` zero, MAC valid, **whole record equal to the host's independent prediction** (`equals_predicted: true`); the host ciphertext is found in the image (positive control). After boot 2: same set. After boot 3: **R1 + R2**, R2 slot B at offset 476,728,384, 249 bytes, tombstone, revision 2 (= R1 + 1, the amendment's [INFERENCE] holds), `previous` = BLAKE2s-256 of R1. After boot 4: same two. S1r after boot 5: R1 only. No MAC-failing candidate, no stale duplicate, no `CKMS\x01` record in any image; D0 before boot 1 has none. Whole-disk file hashes: boot 1 = boot 2 (the reload boot wrote nothing), boot 3 = boot 4 (read-only boot), S1r after boot 5 = S1 (pristine copy, equal before and after).
Secret scan of the six images and of every serial/sshd/ledger/argv file: no canary hit; the ciphertext and `CKMS` headers do not appear on any serial; controls (planted forms in a copy of a data-bearing region, unaligned and chunk-straddling plants) passed before and after every scan.

## 4. Deviations from the amendment text and decisions taken (all inside the closure)

1. **`ssh-keygen -C` truncates the comment at 1,023 bytes on this host (OpenSSH_9.6p1), so the 1,900-byte comment of D-RT1-4 A cannot come from `-C`.** `kssh_keys.py` verifies the length, rewrites the plain container with the full 1,900-byte comment and wraps it with `ssh-keygen -p -a 16` (the wrapped container is genuine `ssh-keygen` output); `expected.json` records `comment_len: 1900`, `comment_via: container-rewrite`. The key is accepted by the paste, K5 parser, import and login. D-RT1-4 A therefore holds; the fallback C was not needed.
2. The F1 `cellos-sign --check` assertion runs after the signing call in the runner (a literal `scripts/cellos-sign` earlier in the file would break the order checked by `test_phase10_canonical_drive.py`/`test_phase10_candidate_import.py`); it still runs before the token claim and before any QEMU.
3. The service-vfs "no `test-hooks`" check reads the cargo fingerprint of the exact lane build (`cargo tree -e features` does not show the root package's own features, so the amendment's [INFERENCE] command form cannot decide it).
4. Boot 4's `kssh-t1-absent` also requires posture `volatile` (amendment 8.1); observed `volatile` in all three B0 runs.
5. The archive allowlist (t1) additionally keeps `frozen-trees*.txt`, `cellbox-ceiling.txt` and `sshd-full.log`; `expected.json` is replaced by `expected-public.json` (record salt/ciphertext reduced to length + hash); the F1 diff is archived as `f1-waiver-unsafe-allowlist.diff`.
6. `frozen-sources-rt1.sha256` leaves out `CHANGELOG.md` (documentation, not a lane input; the amendment's 4.3 table does not list it). The T0 freeze file is untouched.
7. `kssh_boots.py` decisions where the amendment is silent: unclean QEMU (SIGKILL escalation or survivor) = no disk work and class hint E; a failed disk-scan control stops the sequence as E (also at the baseline); S1 is taken after any clean boot 1; the settle poll interval is 1 s.
8. Judge decisions where the amendment is silent: `first-status`/`reload` over 5,000 ms is class D (`import`/`delete` are recorded only because they include typing); `trust-store-volume-mounted`, `t1-iso-same`, `t1-s1-pristine`, a missing `boot.json` or a wrong disk image are class E; a gate stop without a failing row, or missing boots without a recorded stop, can never come out as A.
9. Not observable from B0: whether any wording of an unrun failure path matches; the `[policy] narrowed` lines for nvme/e1000/input are the same as in T0.

## 5. Problems and open points

- The B0 budget (3 sequences) is used up; any further rehearsal needs a new decision.
- Nothing in B0 stops RT1 for K4b/VFS/K6: no product finding was seen.
- Any edit of a frozen file before RT1 (including the runner or the driver) invalidates `frozen-sources-rt1.sha256`; regenerate it with the snippet of section 6 and re-verify.
- The evidence tree also contains the three B0 directories (no evidence value, secret-free, archive-scanned).

## 6. Regenerating the freeze (only if a frozen file must change)

`cd /home/dmin/cellos && { awk '{print $2}' .agents/261030-1000-kms-ssh-key-custody/reports/k8-evidence/frozen-sources-rt1.sha256; } | xargs sha256sum > /tmp/new-freeze && mv /tmp/new-freeze .agents/261030-1000-kms-ssh-key-custody/reports/k8-evidence/frozen-sources-rt1.sha256` (re-hashes the same 78 paths).

## 7. Go / no-go and the RT1 command

**Go** for the RT1 evidence set, subject to the main agent's own checks: Docker up with image `cellbox-p09-sshd:5bdd6545...` present, no stray `qemu-system-x86_64` or `cellbox-p09-sshd-*` container, `/tmp` free >= 8 GiB, `git status` limited to the RT1 closure plus `.agents/**`, the freeze verifying. No `RUN_CONSUMED-k8-rt1` exists. Expected run time about 5-6 minutes (cold build about 4 minutes, five boots about 100 seconds).

```
cd /home/dmin/cellos
mkdir -m 700 /tmp/cellbox-phase09-kssh-rt1
export PATH="$HOME/.cache/cellos/qemu-10.2.0/bin:$PATH"; unset CELLOS_PRODUCTION
CELLBOX_PHASE09_WORK=/tmp/cellbox-phase09-kssh-rt1 bash scripts/qemu-phase09-ssh-test.sh --kssh --persist --set rt1 > /tmp/cellbox-phase09-kssh-rt1.log 2>&1; echo exit=$?
```
The runner claims `RUN_CONSUMED-k8-rt1` with `O_EXCL` just before boot 1, writes the archived evidence to `reports/k8-evidence/rt1/` and exits 0 only for class A. One evidence run, no rerun for any class.
