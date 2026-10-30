# T3-D `ssh-guest-cancel-producer` MARKER-FAIL: diagnosis (documents only; nothing edited, built, run, fetched or committed)

Author T3Diag, 2026-10-30, HEAD `3af9a6516` (clean tree; the T3-D run itself ran at `ac03f419`, and `git diff ac03f419 HEAD -- kernel cells/services cells/tools libs` is empty, so every product file read here is the file that ran). Scope: read the cancel path end to end, explain why the guest `yes` producer and its sshd connection were still alive, rank the hypotheses, and propose the next step. Companion: [cp1-diag-draft.md](cp1-diag-draft.md). No acceptance row, dossier part or source file was touched.

Evidence base (all read, SHA-256): `evidence/t3-d/serial.log` `e5aabde1…de5b`, `evidence/t3-d/verdict.json` `ea2bbc0f…c0a9`, `cp2-dossier-part1.md` `1076a7e3…23f4`, `cp2-dossier-part3.md` `2b8b39bb…4452`, `cp1-draft.md` `2f7ae851…12ba8dc` (the approved digest), the other-host lane `L5f-p09-faults` of `261020-1750-cellbox-cli/reports/s10-3d-evidence/rerun/lanes/` (`serial.log`, `sshd.log`), and the source files listed in §9 (full digests there).

## 1. Verdict in one paragraph

**The cause is determined from the existing evidence at the design level; one link is inferred, not observed.** The Ctrl-C cancel does not make the client close its TCP connection. It makes the kernel **kill the `cellbox-ssh` cell** (the shell's own comment: "killed by the kernel, so it wrote no final frame"), so the client's `tcp_close_graceful` at `cells/tools/cellbox/src/ssh.rs:1260` never runs. The only other thing that could end the connection is the **Net service reclaiming the dead cell's socket, and that reclaim cannot happen**: the Net cell asks the kernel for an owner-death watch, the kernel grants that only to the registered VFS cell (`kernel/src/task/scheduler.rs:663`, `syscall.rs:6352/6405`), the watch is refused, and the Net cell prints `[net] owner death watch unavailable; socket reclaim deferred` (`serial.log:314`, also `L5f serial.log:305`). With no watch, `release_owner` (`service-runtime.rs:409`) is never reached, so the dead owner's smoltcp socket stays `ESTABLISHED` forever (no FIN, no RST ever leaves the Net cell; `TcpClose` itself is documented as "the peer is not told", `libs/ostd/src/clients/net.rs:150`). The outer slirp, QEMU's hostfwd and the nested guest `sshd` therefore see a healthy, idle connection. sshd logs nothing, and `yes` stays blocked on a full pipe: alive. It is a **product defect** (Net service plus kernel watch policy; locked scope), not a lane, slirp, guest-image or `p9` defect. What is not observed: the state of that smoltcp socket after the cancel (a pcap or a guest `/proc/net/tcp` dump would show it). That is the whole content of the proposed diagnostic (CP1 draft).

Two further findings that change how earlier evidence reads: (a) the "other-host Docker lane already passed cancel" evidence (S9-6 `ssh-cancel-exec`) is **client-side only** and never showed the server noticing; the same refused watch line is in that lane's serial log (§4). (b) The guest FIN path through slirp hostfwd and the nested virtio-net is **positively proven by this very run** (§2.5), which eliminates the "FIN does not traverse slirp" hypothesis in the dossier.

## 2. The cancel path, end to end

### 2.1 Driver and shell

`ssh-guest-cancel-producer` (`ssh/guest.rs:81-86`) runs `ssh --user tester --auth password -p P 10.0.2.2 --command 'echo p09-started; p9 produce'`, waits for `p09-started` on stdout, sleeps 500 ms and submits Ctrl-C through the terminal host. The shell builtin `cmd_ssh` (`cellbox/src/ssh.rs:61-82`) spawns `/bin/cellbox-ssh` as a **foreground child** (`executor.rs:3699`, `run_foreground`, loop `3768-3795`). Ctrl-C is the input owner's cancellation of the child's **operation**, not a signal the client handles.

### 2.2 Kernel

`cell_session.rs` `cancel` (`:336`) marks the operation, revokes every endpoint holder and, for live committed children, sets `kill = true` (`:398`); the next `sweep` then runs the canonical kill funnel `sched.exit_task(tid, usize::MAX)` (`:1568`). The shell reads the child's terminal record (`child.wait_status()`, `executor.rs:3803`) and maps the kill to status **130**; `settle` (`cellbox/src/ssh.rs:96-103`) turns the last progress frame into `outcome=cancelled, error_code=cancelled` at the phase/dispatch the child last reported (`running`/`accepted`). That is exactly the T3-D record, and it is produced by the **shell**, not by the client cell. So "st=130 outcome=cancelled" proves that the foreground operation ended and says nothing about the TCP connection.

A killed cell does not go through `Syscall::Exit`, which is why the serial log has **no** `Syscall::Exit: task 22` line for the cancelled client (`serial.log`: task 21 at line 690, task 23 at 721; every other `cellbox-ssh` run has its exit line). The other-host lane shows the same gap at its cancel (§4).

### 2.3 What the client would have done (and did not)

On every normal end `cellbox-ssh` calls `net.tcp_close_graceful(socket)` (`cellbox/src/ssh.rs:1260`, comment: "FIN, not a silent discard: the server must see the connection end"). That path ends in `TcpCloseGraceful` → smoltcp `close()` (FIN queued) → `reap_graceful_closes` (`service-runtime.rs:306`). A killed cell never reaches that line; no cooperative-cancel handler exists in `libs/vissh/src/session.rs` or in `cellbox/src/ssh.rs` (grep for cancel/close/abort/linger there finds only the `Credential`/`Ending` enums and the two graceful-close calls). There is no linger or RST logic anywhere on the client side; the socket is a Net-service capability, not a kernel file descriptor.

### 2.4 Net cell

The socket lives in the Net cell's smoltcp `SocketSet` (`socket_table.rs`, owner = `(cell_id, generation)`). The Net service reclaims a dead owner only through `release_owner` (`service-runtime.rs:409-442`), and that is reachable only from two places, both guarded by the owner death watch: the watch-death message branch (`:597-603`) and the stale-owner sweep (`:634-640`), which sits inside the `Some((principal, token))` arm of `sys_watch_cell_owner` (`:631-645`). The kernel refuses that syscall for the Net cell:

- `scheduler.rs:663`: `if !crate::fast_ipc::is_registered_vfs_cell(watcher.cell_id.0 as usize) … { return None; }`; comment at `:386` "Token-indexed **VFS-only** root-death subscriptions".
- `syscall.rs:6352` and `:6405` gate `ResolveCellOwner*`/`WatchCellOwner*` the same way.
- Net prints the refusal once (`WATCH_REFUSED`, `:643`): `[net] owner death watch unavailable; socket reclaim deferred`. It is in **this** run (`serial.log:314`, right after DHCP) and in the L5f lane (`:305`).

Two more facts make the defect worse than "deferred":

- Even if the watch were granted, the Net main loop receives with `sys_try_recv_attested` (`:588`), whose kernel path (`syscall.rs:5098-5214`, `task.rs:2644` `ipc_try_recv`) consumes only wire messages and never `pending_owner_deaths`/`pending_deaths` (those are consumed by `Recv`/`RecvTimeout`, `syscall.rs:4544`, `:2805`). A queued owner death would not reach the Net loop. [INFERENCE from reading; the VFS cell, which blocks in `Recv`, is what the design served.]
- `release_owner` and `SocketTable::remove_owner` do `sockets.remove(handle)` (`:436`; `socket_table.rs:207`). smoltcp 0.11 `SocketSet::remove` "without changing its state" (`socket_set.rs:120`) drops the socket and emits nothing; an immediate drop is therefore also silent (the API doc says so: `TcpClose` "The peer is not told (no FIN or RST)"). A fix that only removes the socket would still send no RST.

Result: after the kill, the dead client's socket stays in smoltcp `ESTABLISHED` with a 4 KiB receive buffer (`handlers.rs:86`), no timeout and no keepalive (none is set), until the Net cell restarts. It also burns one of the 258 per-owner / 290 total socket slots (`socket_table.rs`).

### 2.5 slirp, hostfwd and the nested guest (the part that works)

Chain: `cellbox-ssh` → Net cell smoltcp socket → e1000 → QEMU slirp conn **A** (`10.0.2.15:eph → 10.0.2.2:P`) → host socket **SA** `connect(127.0.0.1:P)` → QEMU's own hostfwd listener accepts **SB** → slirp conn **B** (`→ 10.0.2.16:22`) → same e1000 NIC → Net-cell L2 bridge → hypervisor virtio-net → nested guest sshd. A close propagates only if the Net cell emits a FIN or RST for A. When it does, this run shows it reaching the guest immediately:

- client-first closes logged by the guest sshd right before the driver's `PASS` line: `Connection closed by 10.0.2.2 port N [preauth]` at `serial.log:551, 553, 564, 574, 584` (client refusals, task exits 255), `:627` (wrong password), and `:691` before `:692 Close session` for `ssh-pty-escape-disconnect` (client disconnect with an open session).
- the `Close session` / `Connection closed by` pairs of the other passing scenarios (`:597-600`, `:613-616`, `:640-644`, `:656-660`, `:680-683`).

So **FIN from the cell through slirp A, SA/SB loopback, slirp B, the NIC bridge and virtio-net to sshd works**, including with an open session. The failing case differs in exactly one thing: nobody sends the FIN.

### 2.6 What slirp does after the silent drop (inferred, why nothing self-heals)

smoltcp answers any segment for an unknown socket with RST (`iface/interface/mod.rs:1215`, `rst_reply`) and the socket is not unknown here: it is alive. It acks slirp's persist probes with window 0. During the flood the 4 KiB receive window is closed (the client drains stdout through a serial terminal), slirp A sits in persist state with probes backing off to 60 s (libslirp `tcp_timers.c`, TCPTV_PERSMIN 5 s .. PERSMAX 60 s [INFERENCE: libslirp source not read here]), SA/SB kernel buffers fill, slirp B closes the window towards the guest, sshd-session stops reading `yes` once the SSH window is spent (it still polls the connection socket for EOF), `yes` blocks writing to a full pipe. Everything is stable and quiet, which is what the log shows: **no line at all about port 42770 after `Starting session: command` (`serial.log:708`)** through the end of the run.

## 3. Was the close ever logged, "even at end of run / shutdown"?

No, and the log cannot show it. The serial log has no per-line timestamps; the only clocks are the gate times in `timeline.json` (ready 12.0 s, `driver-terminal` 124.0 s, elapsed 127.1 s). The cancel itself completed in 1.76 s (`ms=1763`). After it the driver made three follow-up logins, each of which ran a full 20-iteration `p9 gone` (the pid arithmetic of §5 shows all three did 20 iterations) and then fault-may-have-run and ledger logins, so **at least ~62 s and probably 70-90 s** passed between the cancel and the verdict [INFERENCE from the scenario sequence]. The run ends at the driver's `P04-TERMINAL` line; the lane then `proc.terminate()`s QEMU (`qemu-tier3-ssh-guest-x86.sh:404-407`; the T3-D lane `proc.terminate()`) with no guest shutdown, so a close that slirp issues at teardown could never be logged by the guest. Therefore the absence of a close line for port 42770 means "not within ≥ ~60 s of the cancel", never "not at teardown".

## 4. How the earlier lanes "proved" cancel, and what differs

Earlier evidence: S9-6 `ssh-cancel-exec` (`ssh.rs:660-668`), in L5, L5f and default lanes of S10-3/3b/3d, `s9-close-evidence`, `s10-0-evidence`.

- Scenario: `echo p09-started; p9 cancel-exec; sleep 30`, Ctrl-C 500 ms after `p09-started`.
- Markers: `st=130`, completion record `mode=exec phase=running dispatch=accepted outcome=cancelled remote_status=null remote_signal=null`, and `run.ms < 30_000` (the **shell returned before the remote `sleep 30` could end**).
- Server evidence used by the judge: the remote ledger `p09x-cancel-exec` exactly once (`qemu-phase09-ssh-test.sh:795-798`, no-replay). That proves the command ran once; it does not look at the connection.
- The Docker `sshd.log` of L5f does contain, as its last connection (172.17.0.1:46008), `Starting session: command … Connection closed by … Close session: …`. But the lane saves `sshd.log` only **after** it has `proc.terminate()`d QEMU (`qemu-phase09-ssh-test.sh:601` terminate, `:614` ledger, `:618` fixture stop, `:623` sshd.log copy), and QEMU teardown makes slirp close every host socket, so that close line is explained by teardown and no timing is recorded. [INFERENCE: the order of lines "Connection closed" before "Close session" shows the client side closed first; it does not show when.]
- The same Net-cell line `[net] owner death watch unavailable` is in the L5f serial log (`:305`), and the cancelled child there has no `Syscall::Exit` line either (task 41 at `:686`, then straight to task 8 at `:701`).

So the earlier lanes passed because they never asked the server-side question; there the leaked socket is invisible. `phase-09-ssh-client.md` item 5 ("close resources on cancellation or consumer death") and its S9-6 checkbox (line 33, "cancel 130") are consistent with what was observed (the client operation was cancelled) but are **not evidence that the TCP connection is closed**. I did not edit those records.

Differences that do not matter: nested path (the failing hop is before slirp), hostfwd (FIN path proven working, §2.5), guest sshd wording (same OpenSSH 9.9p2, same `Connection closed by` lines when a FIN arrives), `yes` writing to a pipe (see §5). The one difference that matters is that T3-D measures the server side.

## 5. The `p9` helper: `produce` and `gone`

`produce`: `echo $$ > /tmp/p09-prod.pid; exec yes p09y`. `gone`: read the pid, poll `/proc/$pid/stat` field 3 once a second for 20 s, `p09-gone` if it is missing or `Z`, else `p09-alive`, exit 9.

How sshd runs the command: sshd-session forks the user child; for a no-pty exec it runs `$SHELL -c 'echo p09-started; p9 produce'` with the user's shell `/bin/sh` (busybox ash) and pipes for stdin/stdout/stderr. In busybox ash the last command of a `-c` list is run without a fork (EV_EXIT); `p9` is a `#!/bin/sh` script, so the kernel re-execs `/bin/sh /usr/bin/p9 produce` in the same pid, and `exec yes` replaces that process. Even if ash did fork, `$$` inside the p9 script is that script process's own pid and `exec yes` keeps it. So **the recorded pid is the pid of `yes`**; the hypothesis "the pid file holds the shell while `yes` is a child" does not hold.

The serial log corroborates this by pid arithmetic from the `User child is on pid N` lines (each connection consumes: sshd-session A, preauth child A+1, user child A+2; the user child forks the command shell; each `p9 gone` iteration forks 2 processes, `$(cut …)` and `sleep 1`):

| connection | user child pid | next connection's user child | consumed between |
|---|---|---|---|
| cancel (42770) | 496 | 500 | 497 = the command process (shell → p9 → `yes`, one pid), 498-499 = next sshd-session + preauth |
| follow-up 1 | 500 | 544 | 501 shell, 502-541 = 40 forks = 20 `gone` iterations, 542-543 |
| follow-up 2 | 544 | 588 | same pattern, 20 iterations |
| follow-up 3 | 588 | 632 | same pattern, 20 iterations |

So `yes` really was pid 497, the file named it, and it stayed in state R/S through three full 20 s waits; "alive, not a zombie" is simply true, not a helper artifact. Other helper checks: the pid file is written before the 500 ms Ctrl-C delay can matter in the follow-ups (they ran seconds later); a missing/stale pid file would print `p09-gone` (the `cut` fails, `s` empty), the opposite of what was seen; `cut -f3` is the state field because the comm `yes` has no spaces. The only helper weakness is cosmetic: the driver's FAIL detail is truncated at 256 chars (`err=tester@10.`), so the follow-up's own description was lost; the diagnostic run should widen it.

## 6. Hypotheses, ranked

| # | Hypothesis | Verdict | For | Against |
|---|---|---|---|---|
| H1 | The cancel kills the client without a FIN/RST, and the Net cell never reclaims the dead owner's socket (owner death watch refused for non-VFS cells; try-recv loop would not deliver it anyway; immediate removal is silent). The connection stays up end to end. | **Determined by code and log; socket state not observed** | `serial.log:314` (and L5f `:305`) watch refusal; `scheduler.rs:663`, `syscall.rs:6352/6405`; `cellbox/ssh.rs:93-103` ("killed by the kernel"); `ssh.rs:1260` is the only close on the client; `service-runtime.rs:631-645`, `:409-442`; `TcpClose` doc "peer is not told"; no `Syscall::Exit` for task 22; no guest log line for 42770; `yes` alive; every cooperative close in this run reaches the guest at once | none found; the only gap is that no run looked at the socket |
| H2 | A FIN does not traverse slirp hostfwd / nested virtio-net (dossier candidate 2) | **Refuted for the FIN path** | §2.5: 5 client-first `[preauth]` closes, the wrong-password close and the pty disconnect all reached the guest sshd immediately | an unobserved RST/half-close under flood could still differ (only matters once H1 is repaired) |
| H3 | sshd behaviour: it sees the close but does not log it, or does not stop `yes` | Unlikely | sshd with `LogLevel VERBOSE -e` logged every other close; closing the connection ends the session and EPIPE/SIGPIPE ends `yes` [INFERENCE: OpenSSH behaviour, no positive control of `yes` + FIN exists yet] | the run has no positive control for "cooperative close → `yes` stops"; that is the one place H3 could hide |
| H4 | `p9` records the wrong pid | Refuted | §5 pid arithmetic; `exec yes` keeps the pid | none |
| H5 | The cell was not killed (still running) | Refuted | the shell obtained a terminal record 130 (`wait_status`), the next logins started | none |
| H6 | FIN was sent but lost in the nested NIC bridge (`[net-bridge] NIC driver reply timeout; frame not acknowledged` exists once per boot) | Possible only as a secondary effect | that line occurs once, at `serial.log:655` in `ssh-exec-stdin-eof`, and the connection still closed | smoltcp retransmits a lost FIN; no FIN was sent here anyway (H1) |
| H7 | Net cell dropped the socket but the RST never reached slirp | Contradicted by §2.4 | a dropped socket would RST the next probe; but reclaim is unreachable (H1) | the `service-runtime.rs` code path cannot run |

Probabilities: H1 ≈ 95 %; the remainder is split over H3/H6 and "something I read wrongly". Decisive observation that would overturn H1: the guest connection disappearing within ~15 s in a run that has the same Net cell.

## 7. What would a fix look like (product; locked paths; needs owner decisions)

The defect is in `kernel/**` and `cells/services/net/**` (relations L32/L37 adjacent, CP1 §9 contingent handoff template). The CP1 for the guest endpoint explicitly did not authorise those edits. Minimal shapes, in increasing scope [all INFERENCE, design only]:

1. **Make the Net cell learn that an owner died, and make the reclaim visible to the peer.** (a) kernel: allow the registered Net cell to use `WatchCellOwner*` (`scheduler.rs:663`, `syscall.rs:6352`, `:6405`, `fast_ipc::is_registered_vfs_cell`); (b) deliver the death to a polling receiver (`TryRecv`, or have Net use a bounded `RecvTimeout`); (c) `release_owner` must `abort()` the TCP socket (RST) and keep it until it has been polled out or a short deadline (the way `reap_graceful_closes` keeps graceful closes), instead of `sockets.remove` immediately. Files: `kernel/src/task/scheduler.rs`, `kernel/src/task/syscall.rs`, possibly `kernel/src/task.rs`, `cells/services/net/src/service-runtime.rs`, `socket_table.rs`. Consequence: Net is a payload-matched cell of candidate `4fae8b20…`, so L5/L5f matched-candidate evidence no longer covers the changed bytes (a new candidate/importer decision), and the L5f-style cancel evidence would then also have to be re-run with a server-side check.
2. **Cooperative cancel**: give the client a grace period to close (kernel kill delayed until the child ends or a deadline). Larger kernel/cell-session change and still racy.
3. **Do nothing in product**, record the limitation, and prove "producer stops on consumer close" with a cooperative close (timeout, EOF), which is lane-only (§8, option B).

None of these is something a worker agent can do inside the current approval.

## 8. Recommendation

- The marker `ssh-guest-cancel-producer` is a **true product finding**, not a lane bug; no lane-only change can make it pass honestly (a lane that closes the connection itself, or a `p9` that kills `yes`, would be a substitute, forbidden by D79-2 and CP1 §11).
- The existing evidence is enough to name the cause. Remaining uncertainty is one unobserved socket state plus the missing positive control (H3). A single lane-only diagnostic boot (≤ 2 boots allowed; [cp1-diag-draft.md](cp1-diag-draft.md)) closes both: it adds a cooperative-close positive control (`--timeout`, graceful FIN path), repeats the cancel with a pcap on the slirp netdev, host `ss` snapshots and guest `/proc/net/tcp` + process snapshots, and records the T3-D claim set around them.
- Decision table (full text in the CP1 draft §9):

| Option | What | Cost / risk | Recommendation |
|---|---|---|---|
| A | Product fix slice (§7.1) with owner handoff, then T3-D2 with the original cancel scenario | locked kernel/net edits, candidate-evidence consequences, long | later, as its own CP1; do not wait on it for the rest of T3-D |
| B | Lane-only: add the cooperative consumer-close marker and record the cancel finding; CBX-23 "producer traffic stops on consumer close" claimed only for a cooperative close, with the cancel half stated as not met | needs the user's wording decision; no source edit | **yes, together with C** |
| C | Lane-only diagnostic boot (this CP1 draft) | ≤ 2 boots, lane files only, no product source | **yes** |
| D | Rerun T3-D unchanged | would fail again deterministically (same Net cell) | **no** |
| E | Treat the cancel marker as passed because other lanes did | not honest: §4 | **no** |

## 9. Reading list with full digests (SHA-256, HEAD `3af9a6516`)

| File | SHA-256 |
|---|---|
| `scripts/qemu-tier3-ssh-guest-x86.sh` | `b34354e06681f10164ca1965f0069dad56f99ae1f0d9d8fc4af2ba4551833eb8` |
| `scripts/tier3_ssh_guest_judge.py` | `d540e69dc1c35286cfc5c323b9a0b5f77391e9b8ac1f9739bef74d84bddf82e7` |
| `scripts/test_tier3_ssh_guest_judge.py` | `3ffde5ba6867b6073a25bcf8828abae611148e4966c54d03c68a12536965dbe1` |
| `scripts/prepare-tier3-sshd-initramfs-x86.sh` | `34ecc945ebbbc55eac9f3567cd17f05a98365828abafa17c5defca6b50930a29` |
| `tests/guests/x86-sshd/guest-init.sh` | `6ff396d99dbd71d41b7e1e9b67f03f0774f60af6398dc648a390f24e3f53def0` |
| `cells/tests/phase04-terminal-driver/src/ssh/guest.rs` | `d5e4d95c1547379bf15e1929ca1863f85afaf935141370daaa57c46a4c51fab0` |
| `cells/tests/phase04-terminal-driver/src/ssh.rs` | `69207cd52b00fe4e905d223a3bba0d77219e40fecfe9daf994d38bdfc984d937` |
| `cells/services/net/src/service-runtime.rs` | `80c64227c53cf28fa01d20dd09094528f631d1419a4be81f832ba03c50b027ff` |
| `cells/services/net/src/socket_table.rs` | `76e8724b5d79ace59ec797a0001fab2fb38ce21fc89aec16f1061de3134f882d` |
| `cells/services/net/src/handlers/tcp.rs` | `3f1a7611f121586ab278eda239290db3d79a7777b9f7fd3e04fbc91487b72f98` |
| `kernel/src/task/scheduler.rs` | `2065efd47e19277b6dd22a6bbeb5665cf1426015720388165d1507f88cf1b82a` |
| `kernel/src/task/syscall.rs` | `a23eb08cd9e3b7bc19c3b1ac161b5b174c38ed0c93ab517ce4df851226e2e7c3` |
| `kernel/src/task/cell_session.rs` | `090f79071060a76b209fbe323ab8b822aacd2c2dd2c4ab05a3875452a93eb591` |
| `kernel/src/task/tcb.rs` | `160628a31c159e58f0a4066f9bc1034156e87d817be62351d23108fbc8cd7859` |
| `kernel/src/task.rs` | `a27bc5d110f03cb3db428d6013ea3c9ac3a1bf0426cc099283944a3ba06d512d` |
| `kernel/src/fast_ipc.rs` | `4676aa66513c9f938544a46a69272d8f59818a648ea75db79a10b21747042d68` |
| `cells/tools/cellbox/src/ssh.rs` | `b71ebc459312c34e4282c93bc5140206b43033181b68fe2fb5414929ec170401` |
| `cells/tools/cellbox/src/executor.rs` | `083f1a6c0107418d6013cbb52234f5527feb4debb5c89a80b827c49b2c4dd2a5` |
| `libs/vissh/src/session.rs` | `a3d51d33b057c2cfb9f61b02394391693722a85934b479bad475dedd292368b1` |
| `libs/ostd/src/clients/net.rs` | `2344128b467f292f42d4dc4be6a2b45f56c62c4b6983dbca7be34bf156de54b4` |
| `libs/ostd/src/syscall.rs` | `95e4beb7e60561a061a6aed841bf2fc981a7a99e0c54716fcd3db4df7708c944` |
| `libs/api/src/services/ipc.rs` | `53600a6a38bf48f72600af4d884035726af21db23c05bd16189610e9a54e9ba4` |
| `scripts/qemu-phase09-ssh-test.sh` | `620b735d404037b79c883fde5272e0f1c3cd11c4432ac786003f031ccc071dc2` |
| `evidence/t3-d/serial.log` | `e5aabde11ab2b47a3b5a64a9950043d3f3177e228762b10ce6ccc9393fb5de5b` |
| `evidence/t3-d/verdict.json` | `ea2bbc0f044ad6cf77ddb9a3df1bd666e4b3df2f01aa498061863d462c78c0a9` |
| `cp2-dossier-part1.md` | `1076a7e3f7b75eb6176a05fb9d8e2fe11269e6702443dfc3f92ed532f88723f4` |
| `cp2-dossier-part3.md` | `2b8b39bb6e541f6c8fae8477353ddcf4df032f7a96b919f2c934c7eca8c04452` |
| `cp1-draft.md` | `2f7ae851e6c8f80fd8960ff8598cc717d3be0a6b822afe7994c09314a12ba8dc` |
| `.agents/261020-1750-cellbox-cli/reports/s10-3d-evidence/rerun/lanes/L5f-p09-faults/serial.log` | `b38a788eb9d1d2777c6a976d81685125917a1e3d0b14d9daac9b52d6f8021cff` |
| `.agents/261020-1750-cellbox-cli/reports/s10-3d-evidence/rerun/lanes/L5f-p09-faults/sshd.log` | `3bf0aed88677b435e1f6c5bd31b3e4604e7d26c7a81416c362622b6b3352c93f` |

smoltcp 0.11.0 `src/iface/socket_set.rs:120-130` and `src/iface/interface/mod.rs:1180-1220` were read from the local cargo registry copy (not hashed). libslirp was not read; statements about its timers are marked INFERENCE.
