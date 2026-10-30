# T3 CP1 (diag) DRAFT: observe why the guest `yes` producer survives a client cancel, and take the cooperative-close evidence (S10-11 T3-D2) (status AWAITING CP1; documents only; nothing edited, built, fetched or run)

Date 2026-10-30. Author T3Diag. HEAD `3af9a6516` (clean tree). Basis: [cancel-diagnosis.md](cancel-diagnosis.md) (read it first), the approved CP1 [cp1-draft.md](cp1-draft.md) (`2f7ae851e6c8f80fd8960ff8598cc717d3be0a6b822afe7994c09314a12ba8dc`, approved in `cp1-approval.md`), the T3-D dossier [cp2-dossier-part3.md](cp2-dossier-part3.md). CP1 §5 forbids a rerun after a marker FAIL without a new CP1; this is that CP1. The approved ceiling statement (§1 of the original CP1: SVM VMM, QEMU-TCG 10.2.0, one loopback hostfwd, password, private lane-only image, not Intel VMX, not KVM, not production) is unchanged and is repeated verbatim in any record this run produces.

## 1. Why a new CP1, and what the evidence already says

The T3-D run (`evidence/t3-d/`, HEAD `ac03f419`, one planned run, consumed) failed one marker, `ssh-guest-cancel-producer`: client `st=130 outcome=cancelled`, but guest `yes` alive 3 × 20 s later and no `Connection closed` line for the cancelled connection. [cancel-diagnosis.md](cancel-diagnosis.md) determines the cause from the existing logs and source:

- the cancel is the kernel **killing** the `cellbox-ssh` cell, so the client never calls `tcp_close_graceful` (`cellbox/src/ssh.rs:1260`);
- the Net cell **cannot reclaim** a dead owner's socket: the kernel grants owner-death watches only to the VFS cell, the Net cell logs `[net] owner death watch unavailable; socket reclaim deferred` (`serial.log:314`), `release_owner` is unreachable, and a plain removal is silent anyway;
- hence no FIN and no RST ever leaves the Net cell; slirp, hostfwd and the guest sshd see a healthy idle connection; `yes` blocks on a full pipe and stays alive. The same defect is in the other-host Docker lanes (`L5f serial.log:305`), whose cancel evidence is client-side only.

What is **inferred, not observed**: the state of that smoltcp socket after the kill, and that a cooperative close really does stop a guest `yes` (no positive control exists in the run). This CP1 asks for one lane-only boot that observes both, and takes the claim-bearing evidence around them. It does **not** change a line of product source.

## 2. What is requested

Approve **T3-D2**: one new lane stage `--stage diag` (same image recipe, same guest, same QEMU/KVM-free ceiling, same one hostfwd), with:

1. all reused scenarios and the two other guest scenarios exactly as in T3-D (`ssh-guest-fault-may-have-run`, `ssh-guest-no-replay-ledger`, `session-a-close`);
2. a **positive control** `ssh-guest-consumer-close-producer` (cooperative consumer close: the client ends the session by its own `--timeout`, goes through the graceful-close path, the guest `yes` must stop);
3. the **cancel scenario repeated unchanged** (`ssh-guest-cancel-producer`), now an **observation** with instrumentation around it;
4. instrumentation, all host-side or guest-side and lane-only: a pcap of the slirp netdev (`-object filter-dump`), host `ss` snapshots of the two loopback sockets, guest snapshots of `/proc/net/tcp` and the process chain, and a post-kill snapshot.

Budget: **one build-only run, at most 2 boots**: boot 1 planned; boot 2 only for a host/environment error (tooling failure, QEMU crash before the scenarios), **not** for an unwelcome result, same rule as S10-4.17/K8. No `RUN_CONSUMED-1` reuse; a new token `evidence/t3-d-runs/DIAG_CONSUMED-N`, claimed with O_EXCL like the T3-D token.

## 3. The experiment

### 3.1 Scenario order in the diag build (driver feature `guest-diag = ["guest"]`, no new dependency, `Cargo.lock` unchanged)

| # | Marker | What | Expected (H1) |
|---|---|---|---|
| 1 | reused set | `ssh_checks` unchanged (17 markers, as T3-D) | PASS |
| 2 | `ssh-guest-consumer-close-producer` | `ssh --user tester --auth password -p P --timeout 8 10.0.2.2 --command 'echo p09-started; p9 produce'` → expect `st=124`, record `mode=exec phase=running dispatch=accepted outcome=timeout`; then `follow_up p9 gone` must print `p09-gone` (`p9 gone` waits up to 20 s) | PASS |
| 3 | `ssh-guest-cancel-producer` (observation) | the T3-D scenario verbatim: Ctrl-C 500 ms after `p09-started`, then `p9 gone` | client part true, `gone` = `p09-alive` (FAIL, recorded, not counted as a claim) |
| 3a | `P04 INFO ssh-guest-diag-mark cancel-done` | printed right after the cancel returns | |
| 3b | marks `snap-a` (+3 s), `snap-b` (+25 s), `snap-c` (+70 s) | each mark is printed, then a guest login runs `p9 snap`; the host lane reacts to the mark with `ss` (§3.3) | guest: connection `ESTABLISHED`, `yes` alive, in all three |
| 4 | `ssh-guest-fault-may-have-run`, `ssh-guest-no-replay-ledger`, `session-a-close` | unchanged | PASS |

Why this order: the control runs while no leaked connection exists, so a failure of the control cannot be blamed on the cancel leak; the observation is last of the guest-process scenarios so the leak cannot disturb earlier ones. The leaked connection did not disturb the T3-D follow-ups (`serial.log:710-794`). The cancel scenario is **not** weakened: it keeps `cancelled && gone_ok` as before, and the FAIL is the expected recorded fact, not hidden. The follow-up description in the FAIL line is lengthened (the T3-D line was truncated at 256 characters).

### 3.2 Guest helper `p9` (new verbs; `produce`, `gone`, `selfkill`, `drain`, `bulk` unchanged; no ledger writes)

- `p9 snap`: prints, compactly: uptime; the content of `/tmp/p09-prod.pid`; the `/proc/PID/stat` fields `pid (comm) state ppid` of every process whose comm is `yes`, `sh`, `sshd` or `sshd-session`; the lines of `/proc/net/tcp` with local port `0016` (state `01` ESTABLISHED, `08` CLOSE_WAIT, `06` TIME_WAIT, queues `tx:rx`), with remote address and port decoded in hex. Read-only; adds one more login per snap (counted in `ssh-guest-logins`).
- `p9 produce` additionally appends `pid=$$ ppid=$PPID` to `/tmp/p09-diag.txt` (not the ledger), so a wrong-pid hypothesis (H4) is directly excluded.
- the guest init and `sshd_config` are unchanged (same init digest as boot 2).

### 3.3 Host instrumentation (lane `--stage diag` only; every observation is a local file or process: no network beyond the existing loopback)

- **pcap**: one extra QEMU argument `-object filter-dump,id=fd0,netdev=net0,file=$W/net0.pcap`. It records the frames between slirp and the e1000 on the outer netdev: flow **A** (`10.0.2.15:eph ↔ 10.0.2.2:P`, the Net cell's smoltcp socket) and flow **B** (`10.0.2.2 ↔ 10.0.2.16:22`, slirp's hostfwd side to the nested guest). It answers the decisive question: after the cancel does 10.0.2.15 ever send FIN or RST on A; does it ack slirp's probes with a zero window (socket alive in smoltcp); does slirp ever send FIN/RST towards the guest on B. The raw pcap stays in `$W` (it carries SSH ciphertext) and is deleted with the other secrets; the judge archives only a derived `pcap-summary.json` (per flow: SYN/FIN/RST counts, first/last time, direction of the first FIN/RST, window values and a packet list after the cancel mark; no payload).
- **host sockets**: on each `P04 INFO ssh-guest-diag-mark NAME` line the lane runs `ss -tnoi '( sport = :P or dport = :P )'` and writes `host-ss-NAME.txt` (wall time, offset from QEMU start). `SA` (QEMU → `127.0.0.1:P`) and `SB` (accepted by the hostfwd listener) are the two sockets; their state/queues say whether slirp A and B still hold the connection. A final snapshot `host-ss-final.txt` is taken just before termination and `host-ss-after-kill.txt` just after, which also settles when the Docker-lane `Connection closed` line is produced (teardown). `ss` is installed (`/usr/bin/ss`); `tcpdump` is not, so the summary is parsed by a small stdlib python parser inside the judge (unit-tested on a synthetic pcap).
- **argv invariant**: the T3-D invariant (one user netdev, one `fwd`, no `restrict`, one e1000, `-m 1G`, TCG, no KVM, pinned QEMU 10.2.0 `169fb4ea…1241fe`) is kept and extended by exactly one allowed addition in the `diag` stage: that one `-object filter-dump,id=fd0,netdev=net0,file=$W/net0.pcap`; `-monitor none` stays. Any other deviation is a STOP.

### 3.4 Judge (`diag` stage)

Required: every marker exactly once (the control and the cancel observation each once; PASS or FAIL both accepted for the observation, FAIL for anything else is a claim failure), the four gates B1/B2/C1/C2 as before, the argv invariant of §3.3, secret scan (seed, password, host private key) over every evidence file including the new ones, protected digests unchanged, all `host-ss-*` files and `pcap-summary.json` present and non-empty, three `snap` outputs present. It then **classifies** (data, not pass/fail): `control_ok`, `cancel_conn_state_at_snap_{a,b,c}` (guest), `yes_alive_at_snap_{a,b,c}`, `host_ss_established_at_{a,b,c}`, `net_fin_rst_after_cancel` (pcap, flow A), `slirp_fin_rst_to_guest_after_cancel` (pcap, flow B), and prints one of the outcome labels of §5.

## 4. Closure (all SHA-256 at HEAD `3af9a6516`, taken 2026-10-30; anything else under `git status` at execution time is a STOP)

### 4.1 Existing files to edit (lane-only; hash = pre-edit digest)

| Path | SHA-256 | Edit |
|---|---|---|
| `scripts/qemu-tier3-ssh-guest-x86.sh` | `b34354e06681f10164ca1965f0069dad56f99ae1f0d9d8fc4af2ba4551833eb8` | stage `diag`: token `DIAG_CONSUMED-N`, evidence dir `evidence/t3-d2/`, the one extra `-object filter-dump`, mark-triggered `ss` snapshots, final and after-kill snapshots, argv invariant of §3.3, window = 3 × time-to-listening + 20 min, hard cap 1 h (T3-D took 127 s) |
| `scripts/tier3_ssh_guest_judge.py` | `d540e69dc1c35286cfc5c323b9a0b5f77391e9b8ac1f9739bef74d84bddf82e7` | `diag` stage (§3.4), pcap summary parser, archive scan for the new files |
| `scripts/test_tier3_ssh_guest_judge.py` | `3ffde5ba6867b6073a25bcf8828abae611148e4966c54d03c68a12536965dbe1` | host tests for the diag stage and the pcap parser (synthetic pcap with FIN/RST/zero-window cases) |
| `scripts/prepare-tier3-sshd-initramfs-x86.sh` | `34ecc945ebbbc55eac9f3567cd17f05a98365828abafa17c5defca6b50930a29` | `p9 snap` and the `produce` diag line (the helper is embedded in this script) |
| `cells/tests/phase04-terminal-driver/src/ssh/guest.rs` | `d5e4d95c1547379bf15e1929ca1863f85afaf935141370daaa57c46a4c51fab0` | `diag_checks` (control, observation, marks, snaps) behind `guest-diag`; `guest_checks` unchanged |
| `cells/tests/phase04-terminal-driver/src/ssh.rs` | `69207cd52b00fe4e905d223a3bba0d77219e40fecfe9daf994d38bdfc984d937` | one `#[cfg(feature = "guest-diag")]` call site next to the existing `guest_checks` call |
| `cells/tests/phase04-terminal-driver/Cargo.toml` | `e4a29891653f5c9143681914735d1571c12897da0057e10d39d24cb3077c2dec` | `guest-diag = ["guest"]` (no dependency; `Cargo.lock` must not change) |

### 4.2 New files

`evidence/t3-d2/**` (small text only: serial log, `qemu-argv.txt`, verdict, `host-ss-*.txt`, `pcap-summary.json`, guest snaps, provenance, protected digests), `evidence/t3-d-runs/DIAG_CONSUMED-N`, and, after the run, `cp2-diag-dossier.md`. No other file.

### 4.3 Reused unchanged (digest-pinned; any change is a STOP)

`tests/guests/x86-sshd/guest-init.sh` `6ff396d99dbd71d41b7e1e9b67f03f0774f60af6398dc648a390f24e3f53def0`; `tests/guests/x86-sshd/apk-pins.sha256` `4edae50a39fe265492f1c22ee4d312ad83c3faf9e6ad925719573ee408db8870`; `scripts/fetch-tier3-sshd-apks-x86.sh` `80e02e105f228e7b07c84c0ef534ca19159431b9ff97f0f19767f4cdfbdaf841` (never runs); the approved CP1's §6.3 closure (templates, repack/FAT tools, guards G1–G5, `qemu-phase09-ssh-test.sh` `620b735d…1dc2`, QEMU binary `169fb4eaaa79f7ad1f3a2d1a1f3534a0807354eabdedaf6a51b6f4b1412241fe`, cached Alpine kernel/initramfs pins of `cp2-dossier-part1.md` §2). The offline apk set under `/tmp/cellbox-tier3-261030a/apk-fetch` (present) is re-verified by `sha256sum -c apk-pins.sha256`. **No network access is needed or requested** (no A-NET session); keys are regenerated per run (A-KEY as before) and deleted after capture.

### 4.4 Product and protected files: read-only (listed so a stray edit is caught)

The whole protected-path manifest of CP1 §6.3 (`kernel`, `hal`, `libs`, `cells/services`, `cells/tools`, `config`, `scripts/cellos_sign`, `scripts/x86`, `tools`, `Cargo.lock`), with the digests recorded before and after; the files this diagnosis depends on and which therefore must be byte-identical to the T3-D run: `cells/services/net/src/service-runtime.rs` `80c64227c53cf28fa01d20dd09094528f631d1419a4be81f832ba03c50b027ff`, `…/socket_table.rs` `76e8724b5d79ace59ec797a0001fab2fb38ce21fc89aec16f1061de3134f882d`, `kernel/src/task/scheduler.rs` `2065efd47e19277b6dd22a6bbeb5665cf1426015720388165d1507f88cf1b82a`, `kernel/src/task/syscall.rs` `a23eb08cd9e3b7bc19c3b1ac161b5b174c38ed0c93ab517ce4df851226e2e7c3`, `cells/tools/cellbox/src/ssh.rs` `b71ebc459312c34e4282c93bc5140206b43033181b68fe2fb5414929ec170401`. The kernel, Net and hypervisor cell bytes of the new image are rebuilt from these sources; the dossier records their new digests next to the T3-D ones.

### 4.5 Gate A (offline, before the boot)

`bash -n` on the lane and prepare scripts; `python3 -m py_compile` on judge and tests; host tests pass (existing 22 + the new ones); `sha256sum -c apk-pins.sha256`; `--stage diag --build-only` (cargo `--locked --offline`, `Cargo.lock` unchanged); protected-path digests before = after; read-only audit of the new cpio (only `etc/t3-host_ed25519` and `etc/t3-password` secret, mode 0600).

## 5. Expected outcomes per hypothesis (what each result would mean)

Hypotheses are those of [cancel-diagnosis.md §6](cancel-diagnosis.md). "A" is the outer flow, "B" the slirp hostfwd flow to the guest.

| Label | Control (`--timeout`) | Cancel observation | pcap after the cancel | Host `ss` | Guest snaps | Meaning |
|---|---|---|---|---|---|---|
| **O1 (H1 confirmed)** | `p09-gone`, `ssh-guest-consumer-close-producer` PASS, guest logs `Connection closed` for it | `p09-alive` | flow A: **no FIN and no RST from 10.0.2.15**; slirp probes (if any) answered with ACK window 0, or silence; flow B: no FIN/RST towards the guest | SA/SB `ESTAB` with queues at `snap-a/b/c` | cancel connection `01` ESTABLISHED, `yes` state `S`/`R` at all three | cause proven: the Net cell holds the dead owner's socket; cooperative close works end to end |
| O2 (H3: sshd/`yes` does not follow a close) | `p09-alive` although the control's connection closed (`Connection closed` logged, or guest connection gone) | `p09-alive` | FIN from 10.0.2.15 on A in the control | SA/SB gone for the control | `yes` alive with ppid chain showing why (`produce` pid, ppid 1, or SIGPIPE ignored) | helper or sshd semantics; fix is offline in `p9`; the claim needs one more run |
| O3 (FIN lost under flood) | `p09-alive`, no `Connection closed` for it | `p09-alive` | FIN from 10.0.2.15 on A in the control but not forwarded on B; or B sends FIN that never arrives | SA/SB state shows where it stops | guest connection `ESTAB`/`CLOSE_WAIT` for the control | transport defect (slirp half-close under flood or the NIC bridge); H2/H6 revived; deeper capture needed (an `info usernet` monitor, argv change) |
| O4 (delayed reclaim) | PASS | `p09-gone` later than 20 s (first alive at `snap-a`, gone by `snap-b` or `snap-c`) | RST from 10.0.2.15 on A at a persist-probe time, then FIN on B | SA/SB closed after the RST | connection disappears at a time ≥ 20 s | something does reclaim the socket, late; the cancel marker would pass with a wait of ≥ 100 s; the diagnosis (§2.4) would have to be corrected |
| O5 (environment) | n/a | n/a | n/a | n/a | n/a | tooling (`ss`, pcap file), QEMU or hypervisor failure before the scenarios: boot 2 allowed; nothing is claimed |
| O6 (surprise) | any other combination | | | | | stop; record the raw data; no further run without a new CP1 |

Decision recorded from the result: O1 → finding for the owners (cancel → leaked socket, kernel watch policy and Net reclaim), and the T3-D2 evidence set below is what CBX-23 gets; O2 → offline helper fix (lane-only) and **no** automatic rerun; O3/O6 → stop and report; O4 → amend the diagnosis and the cancel wait.

## 6. What the result is worth for the acceptance rows (no row is edited by this CP1)

| Claim | Evidence in this run | Status |
|---|---|---|
| CBX-23 guest half except "producer stops on consumer close" | the 20 passing markers of T3-D, re-observed | as T3-D (met at the ceiling once CP2 passes) |
| CBX-23 "producer traffic stops on consumer close" | `ssh-guest-consumer-close-producer` PASS: a cooperative close (client timeout, graceful FIN) stops the guest `yes` through the hostfwd and nested path | claimable **only with the user's wording decision D-DIAG-2** |
| "producer stops on a client **cancel**" | `ssh-guest-cancel-producer` (O1: FAIL, recorded) | **not met**; product defect recorded (cancel leaks the Net socket); stays open until a product fix |
| CBX-25 | as T3-D | as T3-D |

Proposed wording for the cancel half (if D-DIAG-2 = A, recorded at CP2): "The Ctrl-C cancel of a guest `ssh --command` ends the client operation (status 130, record `cancelled`) but does **not** end the TCP connection: the Net service has no owner-death reclaim for a killed cell (kernel owner-death watches are VFS-only), so the guest sshd and the producer are not notified. This is not met at this ceiling and is a product finding, not a lane artifact." The earlier other-host cancel evidence (S9-6 `ssh-cancel-exec`) is client-side only; the CP2 states that and does not edit that record.

## 7. Reruns, gates and what follows

- The T3-D run is consumed; **this stage is the new gate**. After it, no further guest run without another CP1, whatever the result (the same rule as before).
- If O1 and the user wants the cancel half met: a **product slice** is required (cancel-diagnosis §7.1: kernel watch policy for the Net cell, owner-death delivery to a polling receiver, `abort()` with a kept socket in `release_owner`; files `kernel/src/task/scheduler.rs`, `kernel/src/task/syscall.rs`, maybe `kernel/src/task.rs`, `cells/services/net/src/service-runtime.rs`, `socket_table.rs`). It needs the contingent handoff of the original CP1 §9 (named file owner, bounded exception, controller-plan adoption), its own CP1, the consequence for the matched candidate `4fae8b20…` (Net is payload-matched) and, afterwards, **one more guest run (T3-D3)** with the original cancel scenario plus the server-side observation. Not requested here.
- If the user prefers B only (cooperative close claimed, cancel recorded not met): the T3-D2 evidence set is complete for CP2; no further run.

## 8. STOP rules (additions to CP1 §5)

| Rule | Value |
|---|---|
| STOP-D1 | any protected-path digest or `Cargo.lock` changes; any file outside §4.1/§4.2 differs in `git status` |
| STOP-D2 | any secret in any evidence file (including `host-ss-*`, `pcap-summary.json`, guest snaps): scrub, do not archive |
| STOP-D3 | QEMU argv differs from §3.3's invariant (one extra `-object filter-dump` allowed, nothing else) |
| STOP-D4 | B1/B2/C1/C2 fail as in STOP-1..4 of the original CP1: record, no substitute, no extra boot beyond the one allowed for environment errors |
| STOP-D5 | the run exceeds the 1 h cap |
| No substitution | a closure made by the lane (a host-side `kill` of the connection, a `p9` that kills `yes`, a doctored `gone`) is never an acceptable way to make the cancel marker pass; the marker stays what the observation shows |

## 9. Decision table

| ID | Question | Options | Recommended |
|---|---|---|---|
| **D-DIAG-1** | Run the lane-only diagnostic boot (§2) as the new gate (≤ 2 boots, §4 closure) | **A** yes; **B** no boot, record the finding from existing evidence only (the cause is determined by the logs and source; the control and the observation stay unobserved); **C** product fix first, rerun later | **A** (cheap, no product edit, observes the one missing link and takes the cooperative-close evidence) |
| **D-DIAG-2** | Claim wording for CBX-23 "producer traffic stops on consumer close" | **A** claim it for the cooperative close (client timeout) and record the cancel half as not met + product finding; **B** keep the whole phrase open until a product fix makes the cancel stop the producer; **C** drop the sentence from the guest scope | **A** (honest: states exactly what was shown and what was not) |
| **D-DIAG-3** | Product defect route | **A** record the finding and open a separate slice with owner handoff (cancel-diagnosis §7.1) later; **B** open it now as a parallel CP1; **C** accept as retained limitation | **A** |
| **D-DIAG-4** | The other-host lanes' cancel evidence | **A** note in the CP2 that S9-6 `ssh-cancel-exec` is client-side only and the same Net defect applies; do not edit those records now; **B** open a records-correction item for `phase-09-ssh-client.md` (item 5, S9-6 checkbox) and the L5f dossier; **C** nothing | **A** now, **B** when the owner slice is chosen |
| **D-DIAG-5** | Budget and caps of §2/§8 (one build-only, ≤ 2 boots, token `DIAG_CONSUMED-N`, 1 h cap) | **A** accept; **B** one boot only; **C** more | **A** |

## 10. Not authorized by this draft

Any edit, build, QEMU run, fetch or key generation before CP1 approval; any edit of `kernel/**`, `hal/**`, `libs/**`, `cells/services/**`, `cells/tools/**` or another protected path; a Net-cell, kernel or cellbox-client change; Intel VMX, KVM; a new network authorization; a Docker fixture or host process standing in for the guest; a lane-side connection kill; an acceptance, dossier or phase record edit; commits and pushes.
