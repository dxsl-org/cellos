# T3 CP2 dossier, part 1: T3-A, T3-B, T3-C (executed 2026-10-30 by T3Exec; HEAD `17db84bd4`; no commit, no push)

Basis: `cp1-draft.md` (sha256 `2f7ae851…12ba8dc`) as approved in `cp1-approval.md`. Evidence: `evidence/` (text only; no key, password, seed or image). Boot-stage lane evidence: `evidence/t3-bc/` (boot 2, passing), `evidence/t3-bc-boot1/` (boot 1, STOP-2). Gate A: `evidence/gate-a.txt`. Fetch records: `evidence/apk-fetch-*`. Work dir `/tmp/cellbox-tier3-261030a` (700, not a symlink); apk files, cargo target dirs and `evidence/` kept there for T3-D, keys/ISO/images deleted.

## 1. Outcome in one line

**T3-A GO met (Gate A passed). T3-B and T3-C PASSED on the second boot (B1, B2, C1, C2 all satisfied; no STOP rule left standing).** The AMD SVM VMM at HEAD boots the Alpine PVH guest under QEMU-TCG 10.2.0, the guest runs OpenSSH 9.9p2 from the pinned apks, and a Cellos cell (`cellbox-ssh` through the Phase04 driver) reached it through the one loopback `hostfwd` (R1). The R1 loop is proven; the Net-cell hairpin (R2) is not needed. One scenario of the reused set (`ssh-first-trust-yes`) failed in the boot stage because of a guest-image defect that is fixed offline in `guest-init.sh` and **not yet booted** (section 6).

## 2. What was built (SHA-256)

| Item | Path | sha256 |
|---|---|---|
| apk pins (produced by A-NET) | `tests/guests/x86-sshd/apk-pins.sha256` | `4edae50a39fe265492f1c22ee4d312ad83c3faf9e6ad925719573ee408db8870` |
| guest init (**after** the post-boot fix, section 6) | `tests/guests/x86-sshd/guest-init.sh` | `6ff396d99dbd71d41b7e1e9b67f03f0774f60af6398dc648a390f24e3f53def0` |
| guest init as booted in boot 2 | (in the initramfs as `/bin/virtio-e2e-init`) | `3a8190d48d3a291126642104152b441d309c6942379f8a2516f0e136c8b6efcd` |
| fetch script | `scripts/fetch-tier3-sshd-apks-x86.sh` | `80e02e105f228e7b07c84c0ef534ca19159431b9ff97f0f19767f4cdfbdaf841` |
| initramfs recipe | `scripts/prepare-tier3-sshd-initramfs-x86.sh` | `630ded8c3b5b6388a771e38145bdb5c56aef22b890a45c21e766b743c65d8ab5` |
| lane (`--stage boot`, `--build-only`; `--stage full` refuses) | `scripts/qemu-tier3-ssh-guest-x86.sh` | `33e51de4b5674aa33fd1839691f094331ddd76f70dfb405089a7ec50d35bb360` |
| judge (`boot`, `archive`) | `scripts/tier3_ssh_guest_judge.py` | `29af52b522eab9057e6baca1dec923b9f05f30a02f4a9d943f1a0b7dcc251030` |
| judge tests (22) | `scripts/test_tier3_ssh_guest_judge.py` | `79290f21d62f99482440d5999995e074b244f053b20de40c6a1f827f7fc03b8b` |
| driver readiness | `cells/tests/phase04-terminal-driver/src/ssh/guest.rs` | `7053ea70a6ec4b425926969a036161040685388690b8c94e12b27eef94aa0c3b` |
| driver `ssh.rs` (pre-edit `efca2f77…93ed32`) | `…/src/ssh.rs` | `fc630672528369865d446a4ca7c60769efc903371942c7986537e768817c351c` |
| driver `Cargo.toml` (pre-edit `8040b18e…6eebc90`) | `…/Cargo.toml` | `e4a29891653f5c9143681914735d1571c12897da0057e10d39d24cb3077c2dec` |

Driver edits (the T3-B subset of CP1 §6.2): feature `guest = ["phase09-ssh"]` (no dependency; `Cargo.lock` unchanged); `pub(crate) mod guest` under the feature; `SSH_MS` 600 000 ms under the feature (provisional, see §7); readiness call `guest::wait_ready` after the fixture block; `s96_checks` not called in the guest build. `guest_checks` and the four guest scenarios (T3-D) are **not** written.

Fetched apk set (Alpine v3.21 main x86_64; `evidence/apk-fetch-record.json`): `APKINDEX.tar.gz` `cfad68d8…037fcb` (487 444 B), `openssh-server-9.9_p2-r0` `8e3c3f20…47482a`, `openssh-server-common-9.9_p2-r0` `71c76f2b…ed0014`, `openssh-keygen-9.9_p2-r0` `a0bdfc84…d9d3`, `busybox-binsh-1.37.0-r14` `cdb0e74a…10996`, `musl-1.2.5-r12` `abfcfe82…89133e`, `libcrypto3-3.3.7-r2` `3c6fb1c9…96a`, `zlib-1.3.2-r1` `52ed3729…fd5e`, `busybox-1.37.0-r14` `e0c42bc2…d4717` (full digests in the pins file). Cache inputs unchanged: `vmlinuz-virt` `26bf81ad…e624ca`, `initramfs-virt` `e2562e01…acdf`, `vmlinux` `2d62d53b…ec07`; QEMU `169fb4ea…1241fe` (10.2.0).

Boot-2 artifacts (`evidence/t3-bc/provenance.sha256`): ISO `30e9b956f0faf6550c76732df4b473bece734a600209df4b076b1ac9abc5af33`, kernel `032945fa6a4a7b561a2343f6a3207280ea8bc53a0fd3181b92f97794713979cd`, `kernel_fs.img` `9f57c89f5f9e32183254b0672c30ef11eb34673f9f98c60ace31ee8269b78c2d`, initramfs `1b1fa9fe436a1d37f8048e5f0b0da91187c392eb53b0b6cc5b3afe024b92230a`, hypervisor cell `d479fe7a…28b9`, driver `0d89ba90…ed07`, cellbox `b27beec8…c705`. Lane-only bytes: `service-hypervisor --features volatile-disk`, driver `--features guest`, private-fixture signing (`CELLBOX_PRIVATE_FIXTURE_SIGNING=1`, random per-run seed), kernel features `phase02-terminal-profile,phase04-driver-profile`, no `/POLICY.BIN` (kernel `Absent` branch). Not a product or promoted image, no candidate import.

## 3. Gate A (offline) — PASS (`evidence/gate-a.txt`)

`bash -n` on the guest init, the three scripts; `python3 -m py_compile` on judge and test; host tests 22/22; `sha256sum -c apk-pins.sha256` (9 OK); read-only cpio audit JSON (177 entries, 14.56 MB unpacked, only `etc/t3-host_ed25519` and `etc/t3-password` carry secrets, both mode 0600, no sshd in the base initramfs, `problems: []`); protected-path digests and `Cargo.lock` unchanged before/after (`protected-before.txt`, `protected-final.txt`). Beyond CP1: all 8 apks verified with `apk verify` (apk-tools 2.14.6 from the pinned initramfs via its musl loader, initramfs keys), and the offline `apk add --no-network --repository /apks openssh-server` was rehearsed in a user-namespace chroot of the repacked initramfs (8 packages, `sshd -V` OpenSSH_9.9p2, `sshd -t` OK).

## 4. The boots

Budget: 2 boots (one planned, one after an offline fix). Both used. No third boot was started.

### Boot 1 (planned) — STOP-2 (guest init defect), 2026-10-30T09:29:35Z, `evidence/t3-bc-boot1/`

QEMU argv as boot 2 (port differs). Timeline (s since QEMU start): SVM root 1.9, hypervisor starting 3.4, vCPU ready 7.0, guest kernel banner 7.1, `TIER3_SSHD_GUEST_INIT_START` 9.6, `CRNG_READY` 9.6, `NET_UP` 9.7, `GATEWAY_PING_PASS`, `APK_INSTALLED` 10.5, `TIER3_SSHD_FAIL: sshd-exited` 11.7. Cause (serial): `sshd requires execution with an absolute path` (init called `sshd`). B1 passed, B2 failed, STOP-2 judged; the lane stopped QEMU at 14.7 s. Per STOP-2 one offline iteration was allowed: init now runs `/usr/sbin/sshd` (iteration 1, recorded).

### Boot 2 (iteration 1) — gates satisfied, 2026-10-30T09:31:16Z, `evidence/t3-bc/`

QEMU: `qemu-system-x86_64 -machine q35 -accel tcg -cpu qemu64,+pdpe1gb,+svm,+rdrand,+rdseed -m 1G -display none … -netdev user,id=net0,hostfwd=tcp:127.0.0.1:43263-10.0.2.16:22 -device e1000,netdev=net0 -drive …nvme` (`qemu-argv.txt`; one netdev, exactly one `fwd`, no `restrict`, no KVM, pinned binary).

Serial markers (exact lines in `serial.log`, prefix `USER: ` omitted):

| Gate | Marker (first sighting, s) |
|---|---|
| B1 | `[INFO] x86 virt: root operation active (SVM)` (1.8); `[hv-x86] hypervisor cell starting (SVM PVH)` (3.9); `[hv-x86] vCPU ready — entering run loop` (8.5, exactly once); none of `KERNEL PANIC`, `[fault] Cell`, `create_vm failed`, triple-fault, `[hv-x86] …fail/error/unexpected` |
| B2 | `[guest-uart] … Linux version 6.12.81-0-virt` (8.5); `TIER3_SSHD_GUEST_INIT_START` (11.9); `TIER3_SSHD_KERNEL 6.12.81-0-virt`; `TIER3_SSHD_CRNG_READY` (11.9; `random: crng init done` at guest t=0, rdrand); `TIER3_SSHD_NET_UP eth0 10.0.2.16/24 gw 10.0.2.2`; `TIER3_SSHD_GATEWAY_PING_PASS`; `TIER3_SSHD_APK_INSTALLED OpenSSH_9.9p2, OpenSSL 3.3.7 7 Apr 2026` (12.8); `TIER3_SSHD_HOSTKEY 256 SHA256:3dibqu4R…MgY` (= the pre-generated key); `Server listening on 0.0.0.0 port 22.` (13.1; also `:: port 22.`); `TIER3_SSHD_LISTENING` (16.8) |
| C1 (R4 probe, not claim evidence) | `ssh-keyscan -t ed25519 -p 43263 127.0.0.1` at 16.8–18.2 s returned `SHA256:3dibqu4RL2D9c0wS2kQU1ZE5nVsBmnCgbQboUxi7MgY` = the guest host key (`c1-keyscan.json`) |
| C2 (R1, cell side) | driver `P04 INFO ssh-guest-ready attempts=1` (13.5), i.e. the first `cellbox ssh … -p 43263 host` from the Cellos cell printed `HOST KEY CHANGED` naming exactly that fingerprint; guest sshd logged `Connection from 10.0.2.2 port 43430 on 10.0.2.16 port 22` then `Connection closed by 10.0.2.2 port 43430 [preauth]` (before the keyscan's connection, port 42004) |

Timing: **time to `TIER3_SSHD_LISTENING` marker 16.8 s; sshd's own listening line 13.1 s; first cell-side route proof 13.5 s; vCPU ready 8.5 s.** The lane stopped QEMU at 21.4 s. The 60-minute / 4-hour caps are irrelevant at this speed. (The marker trails sshd by a `sleep 1` polling loop; C1 starts only on the marker.)

Judge (`evidence/t3-bc/verdict.json`): gates `{B1,B2,C1,C2}` all true, `stop_rule: null`, all 26 checks PASS (argv invariant, pinned hashes, no sshd in the outer FAT layout or boot config, no secret in any evidence file or the raw serial, protected digests unchanged).

## 5. STOP rules

STOP-1 not triggered (VMM boots a guest at HEAD). STOP-2 triggered in boot 1 (guest init bug), cleared by the one allowed offline iteration. STOP-3 and STOP-4 not triggered. STOP-5 (secrets): none found; the lane archive scan passed. STOP-6: `protected-final.txt` equals `protected-before.txt` (kernel, hal, libs, cells/services, cells/tools, config, scripts/cellos_sign, scripts/x86, tools, `Cargo.lock`). No Docker, no substitution.

## 6. Deviations and findings (all of them)

1. **A-NET used two HTTPS sessions, not one.** Session 1 fetched `APKINDEX.tar.gz` and aborted offline: my resolver found `/bin/sh` ambiguous (busybox-binsh | dash-binsh | yash-binsh) before any `.apk` GET. Session 2 (approved by Main, recorded in `cp1-approval.md`) fetched exactly the 8 packages, reusing the on-disk index; GET only, `dl-cdn.alpinelinux.org` only. The script now prefers `busybox-binsh` and reuses a lone `APKINDEX.tar.gz`. The closure also pulls `busybox`, which replaces the initramfs busybox (same 1.37.0 line) at install time.
2. Pre-boot host error before boot 1: the first lane invocation stopped at the trust-disk step (stale `srv-receipt.json` from `--build-only`); no QEMU started, the lane now removes it (`evidence/pre-boot-host-error-1.log.txt`). Two `--build-only` runs preceded it (first stopped by a compile error in `guest.rs`, `sys_set_timer` takes ticks; second passed).
3. The lane's own QEMU-running precondition was ineffective (`pgrep -x` on an 18-character name never matches); fixed to `qemu-system-x86` before the boots, and `pgrep -af` showed no QEMU before and after.
4. **Judge rule corrected after boot 2 and the same evidence re-judged.** Original judge (`verdict.run-judge-v1.json`) reported STOP-4 on a single check I had added, `C2:ready-after-listening`, which compared the cell probe (13.5 s) with the init's polling marker `TIER3_SSHD_LISTENING` (16.8 s). The CP1 C2 criterion has no such rule; the correct reference is sshd's own `Server listening` line (13.1 s). The judge now uses it, and `--carry-secret-checks-from` re-judges the retained evidence with only the two secret-scan results carried over (keys were already deleted). `verdict.json` is the re-judged result; the original is kept beside it. No new boot.
5. Scenarios ran briefly in the boot stage (not claim evidence, not judged): because C1 waited for the late marker, the driver ran on for ~8 s after readiness: `ssh-refusals`, `ssh-changed-host-key`, `ssh-first-trust-no`, `ssh-first-trust-paste-refused` PASS (these four are exercised against the real guest); `ssh-first-trust-yes` **FAIL**: the guest accepted the password (`Accepted password for tester from 10.0.2.2`, so password authentication against OpenSSH works) but the session failed with `Could not chdir to home directory /home/tester: Permission denied` and `/bin/sh` could not run. Cause: the Alpine initramfs cpio carries `.` with mode 0700, so `/` is root-only. **Fix applied offline after the boots** (`guest-init.sh`: `chmod 755 /`, `chmod 1777 /tmp`; hash in §2). It has **not been booted**; the next boot will be the T3-D planned run. This is a recorded guest-image defect, not a hidden retry: no extra boot was spent. Main must decide whether the post-boot init edit is acceptable for the T3-D budget (it changes the booted image bytes; B/C are re-verified at the start of every full run by the readiness wait).
6. `[net-bridge] NIC driver reply timeout; frame not acknowledged` appears once in each boot (after the first connections); no fault followed; recorded as an observation.
7. The CP1 §6.1 path `reports/t3-{a,bc,d}-evidence/**` was replaced by `evidence/` as the assignment instructed. The three guest scripts and `.alpine-cache-x86` were not modified; no file outside the closure was touched (`git status`: only the driver `Cargo.toml`/`ssh.rs` modified plus the new files).

## 7. Go / no-go for T3-D

**GO**, with T3-D still to be implemented (nothing of it exists yet). Evidence for go: VMM boots at HEAD, guest sshd reachable from a cell through R1 in 13.5 s, password authentication against the guest works, four reused scenarios already pass, nested TCG is fast (a full SSH exchange takes about 1–2 s), so the 4-hour cap is not a risk; the provisional `SSH_MS` of 600 000 ms can stay or drop (T3-B shows handshakes in seconds; T3-D should record the measured value).

Remaining T3-D work (not done here): `ssh/guest.rs` scenarios (`ssh-guest-cancel-producer`, `ssh-guest-fault-may-have-run`, `ssh-guest-no-replay-ledger`, readiness already done), `ssh.rs` `guest_checks` call, full-stage judge (markers exactly once, QEMU argv invariant already coded, ledger rule), lane `--stage full`, then a CP2 for the rows. Window rule: 3 × 16.8 s + per-case budgets, hard cap 4 h.

Exact command once the above exists: `CELLBOX_TIER3_WORK=/tmp/cellbox-tier3-261030a bash scripts/qemu-tier3-ssh-guest-x86.sh --stage full` (the script currently refuses `--stage full`). Today's runnable commands: `… --stage boot` (build + one boot + judge + archive) and `… --stage boot --build-only`.

Not done and not claimed: any T3-D run, acceptance rows, docs, CHANGELOG, Intel VMX, KVM, publickey to the guest, Net-cell hairpin, canonical or candidate promotion.
