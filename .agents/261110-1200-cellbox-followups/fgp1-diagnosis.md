# F-GP-1 diagnosis: `Bad packet length 2270388607` in the second 512 KiB stage (documents only)

Status: **diagnosis + CP1 draft; nothing edited, built, booted or fetched.** Date 2026-10-30. HEAD `8d9f3276f8b8d129c7a506a33ca9a47543c1da55`. Inputs: [guest-pipeline-dossier.md](guest-pipeline-dossier.md) §3/§4 F-GP-1, `evidence/t3-pipe/{run,b0-1}/serial.log`, `verdict.json`, `timeline.json`, `qemu-argv.txt`, and the code read for this note (paths and lines below, working tree as of this note). Ceiling is the dossier's (Linux guest + OpenSSH on the AMD SVM VMM under QEMU-TCG 10.2.0, one loopback hostfwd, private lane image; not KVM/Intel/production).

Labels: **[FACT]** = read in a file/log named here; **[INFERENCE]** = reasoning from facts, unobserved.

## 0. Verdict in five lines

1. **No product defect is determinable from the code as the cause of F-GP-1.** I read every hop of the client→guest-sshd byte path (vissh pump/drive, sunset output/encrypt, `NetTransport`, `NetClient::tcp_send_some`, Net `TcpSend`, IPC exact calls, the Net↔e1000 command protocol, the e1000 driver, the VMM virtio-net header) and found no path that returns "accepted" for bytes that were not queued, queues bytes twice, or reorders them (§3). Three real but non-causal hazards were found (§4).
2. **The log signature is more specific than the dossier says.** OpenSSH ChaCha20-Poly1305 decrypts the 4-byte length with the sequence number as nonce *before* the MAC is checked, and the MAC is checked only after a full packet. `Bad packet length` therefore means the sshd decoded garbage **at the start of a packet** (wrong nonce = a whole SSH packet lost, duplicated or reordered, or a counter skew), or the four length bytes themselves were hit. A byte lost or inserted *in the middle* of a packet ends in `Corrupted MAC on input` instead, because sshd waits for the full (wrong) length and the MAC fails first; a bit flip mid-packet likewise. [INFERENCE from OpenSSH `packet.c` semantics; the sunset server in the host harness reports the same split as `Error::BigPacket` vs `Error::BadDecrypt`, `third_party/sunset/src/traffic.rs:218-223`; confirmed by test T-sig in §5].
3. **That shifts the suspicion from "bytes damaged on the wire" to "a packet-granular event"**: TCP checksums are verified at every TCP endpoint on the path (smoltcp, slirp, guest Linux: the VMM advertises no checksum offload, `cells/services/hypervisor/src/virtio_net.rs:16` only `VIRTIO_NET_F_MAC`), so L2 loss/dup/reorder in the Net/e1000/slirp path cannot by itself produce a clean cut at a packet boundary. The client's chunks are packet-granular by construction (one ≤512 B stdin read → one SSH packet → one IPC chunk, §2.3), which is why chunk-level loss/duplication in the client↔Net hand-off or in the slirp loopback relay under back-pressure fits the signature best, although no code path for it was found.
4. **Offset/size cannot be recovered from the evidence** (no per-byte counters, no pcap, no packet numbers; sshd's `Transferred:` line is not printed on a fatal). Timing suggests the failure was early in the data phase (§1, [INFERENCE]).
5. **Cheapest next step is host-only** (§5, slice A: lossy/partial-write mock against the existing in-process sunset server in `libs/vissh/tests/session.rs`; ~1 test file edit, minutes). The decisive step is one instrumented boot with the slirp netdev capture that the lane already supports for `--stage diag` (§6, slice B). CP1 draft in §7.

## 1. What the evidence establishes

[FACT] unless marked.

| # | Observation | Source |
|---|---|---|
| E1 | Stage "roundtrip-sum" (`cat /tmp/p09-rt.bin \| ... --command sha256sum`): guest sshd logs, after `Starting session: command ... port 43252`, `Bad packet length 2270388607.` then `ssh_dispatch_run_fatal: Connection from user tester 10.0.2.2 port 43252: Connection corrupted`; **no** `Close session`, **no** `Connection closed by`, **no** `Transferred:`; client `st=255 ms=1997 ... connection lost`, completion `phase=running dispatch=accepted outcome=unknown error_code=transport_lost`. | `run/serial.log:689-696` |
| E2 | `2270388607 = 0x8753617f`: random-looking, not ASCII, not a plausible length. | arithmetic |
| E3 | Same stage in `b0-1` (rehearsal, same lane/judge/driver bytes): clean, `Transferred: sent 2000, received 545624`. The two boots' blobs differ (`sha b63006…` vs `23674f…`); the SSH stream is ciphertext under per-connection keys, so plaintext content cannot shape a framing error. A content-dependent cause is excluded for every layer below SSH; above SSH it could not present as a framing error. | `run/pipe-blob.json`, `serial.log:653 / b0-1:653` |
| E4 | Every 512 KiB stage carries ~21 bytes of SSH overhead per 512 data bytes: `received 545696` (exact-stdin, 524288 data), `545944` (stage 1). 21,408/1024 = 20.9 B/packet. So the client sends **one SSH packet per ≤512-byte stdin read** (`Endpoint::read` caps at 512, `libs/terminal/src/stream.rs:44`), ≈1,024 packets of ≈545 B per stage. | `serial.log:649,672`; `stream.rs:43-44` |
| E5 | Stage 1 (up + echo back, 1 MiB on the wire) took 6,180 ms; stage 2 failed after 1,997 ms **including** process start, KMS preflight/sign, TCP connect, KEX, auth, exec. [INFERENCE] The data phase of stage 2 was a fraction of a one-way transfer: the failure is early (dozens to a few hundred packets, possibly the first packets after the exec request; the client starts sending stdin at `Dispatch::MayHaveRun`, before the exec reply, `libs/vissh/src/session.rs:602-639`). | `serial.log:696` |
| E6 | The Net cell printed `[net-bridge] NIC driver reply timeout; frame not acknowledged` (a once-only line, `FIRST_REPLY_FAILURE`) at **`run/serial.log:609`, during the pipe stages** (after task 16 exit, while the guest closes the pk-exec session). In `b0-1` the first (and printed) one is at `serial.log:436`, during the guest kernel boot. The run had **no** such line during guest boot, so in `run` the first reply timeout happened *under pipe-stage load*. The count of timeouts is not logged in either boot (`DRV_TIMEOUTS` is printed only by the `loop-trace` feature, `interface.rs:77-78`, `service-runtime.rs:645-651`). | `run/serial.log:609`, `b0-1/serial.log:436` |
| E7 | No `[net] owner death:` / `[net] graceful close deadline:` line in either boot; `verdict.json net.owner_death_lines = []`. The NET-OD reaper aborted nothing and no socket was released with an owner. | `verdict.json`, `grep` of both logs |
| E8 | Stage tasks: 16, 17 (refusals), 18 (stdin-exact), 19 (round-trip-up), 20 (sum, exit 255). Stage 20 is the only one with a non-zero exit after the refusals. Stages 18-19 passed on the same boot. | `serial.log:608-692` |
| E9 | QEMU: TCG, `-m 1G`, one e1000 on one user netdev, hostfwd `127.0.0.1:44767 → 10.0.2.16:22`; **no capture**. The client connects to `10.0.2.2:44767`, i.e. slirp NATs the Cellos connection (leg A) to the host loopback, where QEMU's own hostfwd listener accepts it and slirp opens a **second** TCP connection (leg B) to the guest `10.0.2.16:22` (the sshd log shows source `10.0.2.2 port 43252`, the host-side ephemeral port). Both legs and the L2 bridge to the nested guest run inside one QEMU/one slirp. | `qemu-argv.txt`; `serial.log:595,668,689` |

## 2. The path that was read

### 2.1 Client (all [FACT], line numbers at the hashes in §8)

- `libs/vissh/src/client.rs:103-135` `pump`: sends `runner.output_buf()` in a loop, `consume_output(n)` with the count `send` returned, breaks on 0. Correct for partial writes.
- `libs/vissh/src/session.rs:610-639`: at most one unsent chunk `out[unsent.0..unsent.1]`; `write_channel` return value `w` is honoured and `unsent=(w,n)`; retry path advances by `w`. Correct for partial writes.
- `libs/vissh/src/session.rs:333-365` + `RekeyPolicy::PRODUCTION` (`:158`: 1 h / 1 GiB / 2^30 packets, `:147-149`): no rekey can occur in a 2-3 s, 0.5 MiB session; `KeysSend::encrypt` ceiling patch `third_party/sunset/src/encrypt.rs:214` far away.
- `cells/tools/cellbox/src/ssh.rs:482-486` `NetTransport::send`: `chunk = data[..min(len, NET_TCP_INLINE_DATA_MAX=3840)]`, returns the `usize` `tcp_send_some` yields. `ssh.rs:488-504` `recv`: ≤2048 B (`RECV_CHUNK`).
- `libs/ostd/src/clients/net.rs:99-114` `tcp_send_some`: `Data(n)` → `n` if `n <= len` else `Err(IO)`; `Ok` → `len`; `Err` → error.
- `cells/tools/cellbox/src/ssh.rs:941-1008` `PipeTerm::input`: `Endpoint::read` ≤512 B, `Ok(n)` only if `0 < n <= buf.len()`; a short read is `Input::Idle`/`Data(n)` correctly.
- `third_party/sunset/src/traffic.rs:522-552,619-639`: one append-only output buffer; `output_buf = buf[idx..len]`, `consume_output` advances `idx` and returns to `Idle` only at `idx==len`; `encrypt` leaves buffer and state unmodified on `NoRoom` (`encrypt.rs:340-365`, `traffic.rs:443-449`), the sequence number is advanced exactly once per encrypted packet (`encrypt.rs:217-223`). The profile is chacha20-poly1305 only (`PATCHES`: 0002), so the length is encrypted with `seq` as nonce (`ssh_chapoly.rs:107-112`): **a sequence skew between client and server shows up exactly as `Bad packet length`**.
- `traffic.rs:456-470`: a non-deferrable packet (ChannelData) that hits `NoRoom` returns `BusySend` (fatal `ClientError::Protocol` → `Ending::Error`), never a silent drop. The observed ending was `TransportLost`, not `Error`.

### 2.2 Net cell and the NIC path

- `cells/services/net/src/handlers/tcp.rs:65-84` `TcpSend`: `send_slice(data).unwrap_or(0)` → replies `Data(n)`, `n` = bytes smoltcp enqueued (partial allowed); socket buffers are 4,096/4,096 (`handlers.rs:86-89`). A 3,840-byte chunk is therefore accepted partially whenever the 4 KiB TX buffer is not empty, i.e. partial acceptance is the *normal* case under any back-pressure, and passed in 3 of 4 big transfers here.
- Request buffer: the kernel's caller-identity trailer occupies the last 32 bytes of the 4,096-byte buffer (`libs/api/src/abi/caller_identity.rs:11-24,98`); 3,840 B + postcard framing stays below `4096-32` (`libs/api/src/services/ipc.rs:23-25`): no overlap.
- Exact-call replies are kernel-correlated (`libs/ostd/src/ipc.rs:318-392`, "only this operation's terminal result is taken"); a late reply cannot be consumed by a later call. `ServiceRef::call` never retries an accepted request (`libs/ostd/src/service.rs:156-159`).
- `cells/services/net/src/interface.rs:376-396, 504-543, 445-467`: TX frames wait in a FIFO (`queue_tx`, `TX_QUEUE_LIMIT=32`), are sent via an untagged one-command-in-flight driver protocol; a **reply timeout or a refusal re-sends the frame** (`settle_tx_verdict`, up to `MAX_TX_REFUSALS=8`), so the failure modes of this layer are **duplicate, late and lost L2 frames**, never altered bytes. When the cached driver tid is gone the "legacy" branch pops and drops a frame (`:478-486`). All of this is below TCP.
- `cells/drivers/e1000/src/controller.rs:267-315,320-354` and `dispatch.rs:49-77`: TX copies into a per-slot DMA buffer and polls `DD` before returning `Status(0)`; RX copies out then recycles the slot, `RDT=head`. TCG's e1000 transmits synchronously on the `TDT` write. No buffer reuse before completion was found.
- `cells/services/hypervisor/src/virtio_net.rs:16,127-148`: RX header carries only `num_buffers`; no `DATA_VALID`/`NEEDS_CSUM`; Linux verifies every TCP checksum.

### 2.3 Why the client's units are whole packets

Every `drive` turn reads one ≤512-byte stdin chunk, writes **one** SSH packet and then runs `pump`; `output_buf` therefore usually holds exactly one ~545-byte packet (or a few, when the TCP buffer was full for several turns) and the IPC chunk equals whole packets; only a *partial acceptance* cuts a packet mid-way. This is the only place where "unit = packet" survives on the way to the wire (E4). [INFERENCE for the "usually"; the 512 B and one-packet-per-read parts are [FACT] from E4 + `stream.rs:44`.]

## 3. Hypotheses, ranked

Rank = fit to the signature (packet-granular desync, early, 1 of 2 boots) × plausibility of a mechanism in the code. "Test" = cheapest observation that kills or confirms it.

| Rank | Hypothesis | For | Against | Test |
|---|---|---|---|---|
| **H1** | **Packet/chunk-granular loss or duplication in the client→Net hand-off (or a counter skew inside sunset)**: a whole chunk is accepted twice or never, or `seq_encrypt` diverges from what reached the wire. | Exactly the `Bad packet length` signature (§0.2); chunks are packet-granular in the common case (§2.3); the failing stage is the one with a long one-way stream and back-pressure (4 KiB TX buffer, constant partial acceptance); intermittent (1 of 2). | No mechanism found: `tcp_send_some`, `pump`, `drive`, `Net TcpSend`, `sunset` encrypt/consume are all consistent with partial writes (§2.1/2.2); stage 1/exact-stdin passed with the same code; the existing host harness never exercises partial acceptance, so "no mechanism found" has **no test behind it**. | **Slice A** (§5): partial/zero/chunked mock vs in-process sunset server; counters at the client (§6). |
| **H2** | **slirp (libslirp in QEMU 10.2.0) mishandles leg A/leg B under back-pressure**: the loopback relay (A→host socket→hostfwd→B) closes the zero-window/persist or retransmission paths in ways that drop or repeat data; both legs share one QEMU thread. | A "clean cut" can arise if slirp's socket buffer (`sbuf`) loses/repeats a whole write unit; the guest is slow (TCG-in-TCG), so leg B back-pressures leg A; the first (and only) NIC reply timeout line appears under load (E6), i.e. the L2 path *was* stressed in this boot, which also creates dup/late frames that slirp must absorb. | Not verified (no network, no libslirp source here); TCP checksums are verified by slirp; no known bug cited; a byte-granular slirp bug would more likely give `Corrupted MAC`. | **Slice B**: slirp-side pcap (`filter-dump`), reassemble leg A client→slirp and leg B slirp→guest, byte-compare. |
| **H3** | **L2 anomalies (driver reply timeouts → dup/late/lost frames, bridge queue limits) trigger a latent TCP-endpoint bug** in smoltcp or slirp (e.g. in out-of-order/duplicate handling or window-probe byte). | E6 shows timeouts under load in the failing boot; frames are re-sent by design; smoltcp's reassembler and slirp's `tcp_reass` get exercised only under dup/reorder. | Needs a TCP stack bug with correct checksums; byte-granular effects would give MAC errors (§0.2); `b0-1` also saw timeouts at boot but passed (its later timeouts are unknown, not counted). | **Slice B** counters: `[net-loop] drv_to` via the existing `loop-trace` feature, pcap retransmission map, guest `/proc/net/snmp` deltas. |
| **H4** | **Guest/VMM side**: nested-guest memory or timing (TCG-in-TCG, VMM copy, guest sshd) damages bytes *after* TCP verified them, or the guest sshd/kernel hits a TCG artefact. | Failure observed on the guest; the guest is the slowest component; page-granular VMM copies are a classic source. | Same §0.2 argument: a random corruption is mid-packet with probability ≈ 99.3 % and would show as MAC error; no VMM defect located; `virtio_net.rs` copies whole frames and Linux checks them. | **Slice B** guest-only loopback through the same bridge (P2) fails without any Cellos client ⇒ guest/VMM/slirp-leg-B; pcap leg-B vs guest `snmp InCsumErrors`. |
| **H5** | **Client-side early termination**: the client ended itself (e.g. `tcp_send_some` → `Io` on a transient `Busy`, see §4 F1) *and* the server's complaint is a consequence of the cut. | `NetTransport` maps every error to `Io` (`ssh.rs:485`); `recv` maps any error + any state probe failure to `Closed` (`ssh.rs:499-502`); the outcome was `TransportLost`. | A client that stops at a packet boundary and closes gracefully makes sshd log `Connection closed by`, not `Bad packet length`; a cut mid-packet likewise ends in a close/reset. Fits at most the *client's* verdict, not the server's text. | Slice A (inject `Err(Io)` and `Busy` mid-stream; record the server error class), slice B client counters + pcap FIN/RST direction and offset. |
| **H6** | NET-OD/GCD reaper or socket-table interplay with the previous stage's lingering socket (handle reuse, stale caps, RST). | Stage 2 starts immediately after stage 1's client exited; NET-OD is new in this lane. | E7: no owner-death/graceful-deadline line in either boot; caps are monotonic (`socket_table.rs:63 next_cap`), table maps `cap→handle` and `remove_internal` clears all ledgers (`:300-307`); ephemeral ports advance (`service-runtime.rs:98-101`); a wrongly removed socket would stall or reset, not inject a framing error. | Already largely excluded; the pcap shows the 4-tuple and any RST. |
| **H7** | Data-/key-dependent defect in chacha/poly or KEX. | One-in-two intermittency can hide anything. | E3: ciphertext is key-random per connection so plaintext cannot matter; ≈2,050 client→server packets passed in stages 18-19 of this boot and ≈4,100 in the four big stages of `b0-1`; chacha encrypt is deterministic given `seq` (`ssh_chapoly.rs:99-132`). | Slice A with 10^3 seeds. |

Not ranked as a hypothesis but **recorded**: E5 says the failure probably is early; if the pcap shows the guest FIN/RST within the first ~20 KiB, H1 (state at exec time) and the exec-before-reply window (`session.rs:602-639`) become the focus.

## 4. Real but non-causal findings from the code (for the owners)

| ID | Where | What | Effect |
|---|---|---|---|
| F1 | `cells/tools/cellbox/src/ssh.rs:485`, `libs/ostd/src/clients/net.rs:99-114`, `libs/ostd/src/service.rs:186-189`, `libs/ostd/src/ipc.rs:412-417` | `NetTransport::send` maps *every* `tcp_send_some` error to `TransportError::Io`. `ServiceRef::call` returns `ViError::IO` for `IpcError::Busy`, which the IPC layer defines as "no work was accepted" (a pure admission refusal, always safe to retry). The Net service is shared with the VMM's `L2Recv/L2Send` polling. A transient admission `Busy` therefore ends a healthy SSH session as `transport_lost`. `ssh.rs:499-502` does the same on the receive side: a failing `tcp_recv` followed by a failing `tcp_state` is reported as `Closed`. | Hazard: spurious `transport_lost` in the client's verdict. **Reachability is not shown** by this evidence ([INFERENCE]: possible only if `Busy` can occur at all on this path). Does not explain the server's `Bad packet length`. |
| F2 | `cells/services/net/src/handlers/tcp.rs:96-100,207-210`, `handlers.rs:55-60` | `TcpRecv`/`TcpRecvReady` consume bytes from the smoltcp RX buffer inside the closure that sends the reply; `send_typed` ignores the `sys_send` result. If a reply cannot be delivered, received bytes are lost silently (`TcpConnect` handles this case, `:48-62`; `TcpRecv` does not). | Hazard for the **server→client** direction (the client's inbound ciphertext), which in stage 2 is only window adjusts/EOF. Not the failing direction. |
| F3 | `cells/services/net/src/interface.rs:455-466, 478-486` | Reply timeout re-sends the TX frame (duplicate frame, by design); a missing cached driver tid takes the legacy branch, which pops and drops a queued frame when `sys_net_tx` refuses. | Loss/dup below TCP only (§2.2); explains E6 under load, not the signature. |
| F4 | `libs/vissh/tests/session.rs:140-148` | The only host harness' `ToServer::send` always returns `data.len()` and its server reads 256 B per step; no test runs the client with partial/zero acceptance, a short `recv`, an `Input::Idle` stdin, or any transport error mid-stream. | Test gap behind H1/H5; closed by slice A. |

## 5. Slice A: the discriminating host-side reproduction (cheap, host-only)

**What:** extend the in-process harness (`libs/vissh/tests/session.rs`; the server side is the vendored sunset `Runner<Server>`, which separates `BigPacket` from `BadDecrypt`) with a *transport policy* and run the real `vissh::session::run` client with `PipeTerm`-like stdin:

1. **T-partial** (the H1 killer): `send` accepts `min(len, cap_i)` with `cap_i` from a seeded pattern including `0`, `1`, `7`, `300`, `545`, `3840`, `4096`; delivers only the accepted prefix; `recv` returns ≤ `k_i` bytes (1, 13, 2048); stdin source returns ≤512 B per call with an `Input::Idle` every `j`-th call (the `PipeTerm`/`Endpoint` behaviour); a 512 KiB pseudo-random blob (plus the lane's structured heads); window/echo as the existing `with_stdin`. Assert: server-side `seen.stdin == blob`, the echo equals the blob, outcome clean, **no** `BigPacket`/`BadDecrypt`. Seeds: 16 in the normal run, `VISSH_SEEDS=2000` for the soak. A failure here is a product defect in vissh/sunset and the test names the file:line by construction (first failing seed + the policy trace).
2. **T-sig** (calibration, justifies §0.2): wrap the same transport with a fault: (a) drop a whole accepted chunk at a packet boundary, (b) duplicate it, (c) drop/duplicate *k* bytes mid-packet, (d) flip one bit mid-packet, (e) flip a bit in the 4 length bytes. Assert the server's first error class: (a),(b),(e) → `BigPacket`, (c),(d) → `BadDecrypt` (that is `Bad packet length` vs `Corrupted MAC on input` in OpenSSH terms). This turns the §0.2 inference into a measured fact for the client library and is what makes the pcap analysis in slice B interpretable.
3. **T-busy** (H5): the transport returns `Err(Io)` after the *n*-th accepted chunk; assert the client ends with `TransportLost`, never `Error`, and that bytes already accepted equal a prefix of the input (no reorder).

**Expected:** T-partial passes (then H1 inside vissh/sunset is closed to the extent of that policy space and the investigation moves to the Net/slirp side with a clean conscience); T-sig is a calibration; T-busy documents F1. **Cost:** one test file, a few hundred lines, host `cargo test -p vissh --target x86_64-unknown-linux-gnu`; no boot.

**Not covered by slice A:** the Net cell's TCP socket and smoltcp (the Net handler is a three-line wrapper over `send_slice`; the existing handler tests, `cells/services/net/src/handlers/tests.rs`, cannot observe the reply because `send_typed` is a syscall), the L2 bridge, e1000, slirp, the guest. A Net-side mock would need `VirtioNetDevice::command` to take a driver trait (a product refactor), which is **not** proposed until slice B points there.

## 6. Slice B: one instrumented boot family (decisive, not cheap)

Goal: separate H1/H2/H3/H4 with data the evidence boot did not collect. All additions are lane-only; **no product file is edited in the base variant**.

| Probe | Mechanism | Separates |
|---|---|---|
| **B1 slirp capture** | `-object filter-dump,id=fd0,netdev=net0,file=…/net0.pcap`, already implemented for `--stage diag` in the lane (`scripts/qemu-tier3-ssh-guest-x86.sh:541-548`; the guard at `:545-548` allows `-object` only for `diag`, so the pipe variant needs that guard widened). Host script (new, stdlib only) reassembles per connection and direction by TCP sequence number, verifies TCP/IP checksums, lists retransmissions and **conflicting overlaps** (same sequence range, different bytes), zero-window periods, and the offset/size where the guest's FIN/RST appears. Then **byte-compares leg A (Cellos→slirp, `10.0.2.15→10.0.2.2:PORT`) with leg B (slirp→guest, `10.0.2.2→10.0.2.16:22`)**. | A ≠ B ⇒ slirp (H2). A = B and both contain a hole/duplicate relative to the expected total ⇒ before slirp (H1/H3). A = B, complete, and the guest still complains ⇒ above TCP at the guest (H4) or in the client's SSH output (H1 inside sunset); also gives the **offset** E5 lacks. |
| **B2 repeat** | The driver's `guest-pipe` feature runs the round-trip-sum stage `N` times in one boot (default 24) plus two controls (64 KiB and 128 KiB blobs, staged separately so no shell slicing is needed). The judge counts failures per stage class. | Failure rate and size-dependence (a length-independent early failure points at connect/exec timing; a size-dependent one at back-pressure). |
| **B3 guest counters** | `guest-init.sh` starts a 1 s background sampler printing the `Tcp:` and `TcpExt:` lines of `/proc/net/snmp`, `/proc/net/netstat` (`InCsumErrors`, `InErrs`, `RetransSegs`, `TCPLossProbes`, `TCPOFOQueue`, `TCPRcvQDrop`) to the UART with the existing `TIER3_SSHD_` prefix. | Guest saw checksum errors/retransmits/out-of-order (L2/slirp) vs a clean TCP layer (above TCP). |
| **B4 guest-only loopback through the bridge (optional, separate boot variant)** | A boot variant **without sshd** in which the guest runs busybox `nc -l -p 22 \| sha256sum` and then `nc 10.0.2.2 PORT < blob` (the single hostfwd sends it back to `10.0.2.16:22`, i.e. through the guest's own virtio-net → VMM → Net L2 → e1000 → slirp leg A → host loopback → hostfwd → leg B → e1000 → Net → VMM → guest path, with no Cellos client and no smoltcp). Repeated N times. [INFERENCE] the rehearsal must show whether the pinned busybox has `nc`; costs one more boot token, so it is optional. | Failure without the Cellos client ⇒ H2/H4 (slirp, bridge, VMM, guest); no failure in N while the SSH stages fail ⇒ the Cellos client/smoltcp side is implicated. |
| **B5 client counters (variant B+)** | Lane-only cargo feature in `cellbox` following the existing `p09-faults`/`kssh-faults` pattern (`cells/tools/cellbox/Cargo.toml:18-31`): in `NetTransport` count offered/accepted bytes, calls, partial/zero acceptances, errors, and an FNV-1a-64 of the accepted byte stream; print once at session end as `P04 INFO ssh-net-counters …`. Product bytes unchanged with the feature off. | Client-accepted total vs leg-A stream length from B1: equal ⇒ hand-off exact (H1 not in the hand-off); different ⇒ a lost/duplicated chunk between `tcp_send_some` and the wire (Net or smoltcp). |
| **B6 Net trace (variant B+)** | Build `service-net` with the existing `loop-trace` feature (`cells/services/net/Cargo.toml:26`) for the diagnostic image: `[net-loop] … drv_cmd drv_to rx_q guest_q` every ~2 s. Changes the Net bytes (still lane-only). | Count of driver reply timeouts during the failing stage (H3). |

Order: B1+B2+B3 first (no product feature, Net bytes = the failing boot's bytes; **comparability is the point**); add B5/B6 only if B1 does not decide.

**Sequencing:** NET-GCD (reap-and-rst) and OSD change `service-net`/kernel bytes; the lane pins them ("NET-OD kernel and service-net bytes (HEAD …)", lane `:509-513`). A diagnostic boot after GCD tests different Net bytes than the failing boot; for comparability run slice B on the HEAD-`8d9f3276f` bytes first (a `git worktree` build, as the lane builds from HEAD), or record the GCD bytes as a deliberate change in the claim map.

## 7. CP1 draft (not approved; status AWAITING CP1)

**CP1-FGP-D: isolate F-GP-1 (diagnostic only, no product fix)**

Ceiling: the dossier's. No new claim of any kind about CBX-23/24/25; the only deliverable is a classification of F-GP-1 and, if slice A finds one, a named defect.

### 7.1 Slice A: host only (recommended first; independently approvable)

Closure (pre-edit SHA-256 at HEAD `8d9f3276f`):

| Path | SHA-256 |
|---|---|
| `libs/vissh/tests/session.rs` | `31a52d78cfe7698c52d51ecf0504ad560282e43de375b59a2b70d01b5ab4f42d` |

(`libs/vissh/Cargo.toml` `0e57ce65…` unchanged: the tests need no dependency beyond the existing ones; a new `libs/vissh/tests/lossy_transport.rs` file is an alternative with no pre-image, but must then duplicate the ~150-line `ToServer`; D-FD-1 picks.)

Gates: `cargo test -p vissh --target x86_64-unknown-linux-gnu` (before: count; after: count + new tests); `cargo clippy` vs baseline for `vissh` tests; `python3 scripts/cellos-sign --check`; pinned file hashes of everything outside the closure unchanged. Run budget: host only, ≤10 min wall for the 16-seed run, soak (`VISSH_SEEDS=2000`) ≤30 min once.
Stop rules: A test that fails on T-partial/T-busy is a **finding**, not a fix: STOP, record seed + trace + file:line, new CP1 for the fix. Any path outside the closure (`libs/vissh/src/*`, `third_party/sunset/*`) is a STOP (sunset changes also need `PATCHES/cellos.diff` + `upstream-files.sha256` updates).

### 7.2 Slice B: one lane boot family (after slice A, owner decision)

Closure (pre-edit SHA-256 at HEAD `8d9f3276f`; every file is already in the GP closure):

| Path | SHA-256 | Change |
|---|---|---|
| `scripts/qemu-tier3-ssh-guest-x86.sh` | `cab760151d35d82e0015b9344b6976f6b342fbebd35f75b34bd9535ffbddb603` | `TIER3_PIPE_DIAG=1` (own tokens `T3P_DIAG_B0-1`, `T3P_DIAG_CONSUMED-1`, evidence `evidence/t3-pipe/diag-*`): widen the `-object` guard (`:545-548`) to `filter-dump` for this mode only, host analysis step, pins unchanged |
| `scripts/tier3_ssh_guest_judge.py` | `5f5de879f88b817fdf5992dad9632eb64c78b0fa882776a1f60848556b03d2aa` | `pipe-diag` verdict: per-stage-class failure counts, outcome label O-A..O-E (below); no change to `pipe`/`full`/`diag` |
| `scripts/test_tier3_ssh_guest_judge.py` | `f7135293d0be783511f802139b135d4bf13eb419ca2e837a84fb6ed122810ad9` | tests for the new label logic |
| `tests/guests/x86-sshd/guest-init.sh` | `4e69ca6b44b9093776445779a1c0e2f27ec12657ea4d2a54597164be5f8c6b22` | snmp/netstat sampler (B3); optional P2 (B4) |
| `scripts/prepare-tier3-sshd-initramfs-x86.sh` | `de46476e4ad6a8bc05f1607ea7116d3aaf3692e44a38123589926e3b9f846788` | stage the 64/128 KiB control blobs |
| `cells/tests/phase04-terminal-driver/Cargo.toml` | `14af10d75a50a9d0b406ee3623ab1d80989ff536414316cd361bf14f30803bef` | feature `guest-pipe-loop` |
| `cells/tests/phase04-terminal-driver/src/ssh.rs` | `1b3f1f9ef2e5fe12aa909313fab9fb84b0948406466f28e13377b389a4c2b67f` | cfg wiring |
| `cells/tests/phase04-terminal-driver/src/ssh/guest.rs` | `8ab32452d417fa9d4036e14959f7fb610d6d16337d92f06c84d010e94d6da490` | repeat loop + controls |
| *new* `scripts/tier3_pcap_streams.py` (+ `scripts/test_tier3_pcap_streams.py`) | n/a (new) | stdlib pcap reassembly/compare (B1) with a synthetic-pcap unit test |
| Variant B+ only: `cells/tools/cellbox/Cargo.toml` | `9a130f44e4fc35b3b222b5dbbc2e3f80d24605965ed2862128f6001b93a878fd` and `cells/tools/cellbox/src/ssh.rs` `b71ebc459312c34e4282c93bc5140206b43033181b68fe2fb5414929ec170401` | `p09-xfer-count` lane-only feature (B5); and `cells/services/net` build with `loop-trace` is a lane build flag, no source edit (B6) |

Not in closure (STOP if touched): `libs/vissh/src/*`, `libs/ostd/*`, `third_party/sunset/*`, `cells/services/net/*` (also dirty from NET-GCD/OSD siblings), kernel, hypervisor, e1000, `CHANGELOG.md` (main agent).

Offline gates: `bash -n` on the shell files, `py_compile`, `python3 -m unittest scripts.test_tier3_ssh_guest_judge scripts.test_tier3_pcap_streams`, `--stage pipe --build-only` with the new mode (pins, tree digests, FAT layout), `cellos-sign --check` rc 0.

Run budget (QEMU 10.2.0 pinned, TCG, ≈35 s boot + stages): **≤1 rehearsal** (`T3P_DIAG_B0-1`, no evidence value: confirms the pcap is written and parsed, `nc` availability, sampler output), **1 evidence boot** (`T3P_DIAG_CONSUMED-1`; N=24 round-trip-sum + 2×N/4 control stages ≈ 3-6 min of stages; window cap 1800 s, as the GP lane), **≤1 confirmation** only for an instrument defect whose raw data already satisfy the criteria. Statistical expectation [INFERENCE]: one failure in two boots suggests a per-stage rate in the 0.1-0.5 range; with N=24, P(≥1 failure) ≥ 92 % at p=0.1 and ≥ 99.9 % at p=0.3. If no failure appears the record is "not reproduced in 24 stages (95 % upper bound ≈ 12 %)" and nothing else is claimed.

Outcome map (pre-registered, one label per boot):

| Label | Observation | Reading |
|---|---|---|
| O-A | failure; leg A client→slirp stream ≠ leg B slirp→guest stream (bytes) | slirp/QEMU environment; no Cellos product claim; record the QEMU/slirp version |
| O-B | failure; A = B; guest `InCsumErrors`/`TCPOFOQueue`/retransmits anomalous or pcap shows conflicting overlaps | L2/e1000/bridge/VMM path stressing a TCP stack; follow-up CP1 on Net↔driver (tagged replies) |
| O-C | failure; A = B clean; (B+) client accepted-bytes ≠ leg-A length | chunk loss/dup between `tcp_send_some` and the wire: product defect in Net/smoltcp hand-off |
| O-D | failure; A = B clean; counters equal; guest snmp clean; (B4) guest-only loop also fails | guest/VMM above TCP; not a Cellos client defect |
| O-E | failure; everything above clean, guest-only loop passes | client SSH output desync (sunset/vissh) or guest sshd; follow-up with slice-A style trace in the cell |
| O-F | no failure in N | not reproduced; no claim |

STOP rules: STOP-D1 secret in any archived text (same scans as the GP lane); STOP-D2 protected digest or `Cargo.lock` change; STOP-D3 `[net] owner death watch unavailable`; STOP-D4 a second boot without a new token; no fix-and-rerun inside the slice. The pcap contains only ciphertext and the public handshake; it is archived under the evidence root, size-capped (≈2-4 MiB expected for N=24).

### 7.3 Decisions requested

- **D-FD-1** Slice A only, as a new test file or edits to `tests/session.rs`? Recommend: edit `tests/session.rs` (single closure file, reuses `ToServer`).
- **D-FD-2** Authorise slice B at all, and whether the `-object filter-dump` guard may be widened for the pipe lane (the lane ceiling text says "one user netdev with exactly one hostfwd"; a capture adds no network surface, it is already allowed for `diag`). Recommend: yes, after slice A.
- **D-FD-3** Variant B+ (lane-only product feature and `loop-trace` Net build): only if B1 does not decide; changes product-adjacent bytes (feature off = unchanged).
- **D-FD-4** Sequencing vs NET-GCD/OSD (Net bytes): run slice B on HEAD `8d9f3276f` bytes first.
- **D-FD-5** F1/F2 (§4) to the owners of `NetTransport`/Net handlers as separate fixes; not part of this slice.

## 8. File hashes at the time of reading (HEAD `8d9f3276f`)

Read, unmodified by this note:

| Path | SHA-256 |
|---|---|
| `libs/vissh/src/client.rs` | `0b0f3f5d33af2896a93e7b22383070ec8ff5abfe68dee670dc95fdf0c5ea5ad5` |
| `libs/vissh/src/session.rs` | `a3d51d33b057c2cfb9f61b02394391693722a85934b479bad475dedd292368b1` |
| `libs/vissh/tests/session.rs` | `31a52d78cfe7698c52d51ecf0504ad560282e43de375b59a2b70d01b5ab4f42d` |
| `cells/tools/cellbox/src/ssh.rs` | `b71ebc459312c34e4282c93bc5140206b43033181b68fe2fb5414929ec170401` |
| `libs/ostd/src/clients/net.rs` | `2344128b467f292f42d4dc4be6a2b45f56c62c4b6983dbca7be34bf156de54b4` |
| `cells/services/net/src/interface.rs` | `08d0a7a8742248396380c1aeed045cc5580294bb3ceba4fa508750f995233684` |
| `cells/services/net/src/handlers/tcp.rs` | `3f1a7611f121586ab278eda239290db3d79a7777b9f7fd3e04fbc91487b72f98` |
| `third_party/sunset/src/traffic.rs` | `ceb75e01c72c7c537882a7387ad7d78a7739aab3eb2c72859d104b581c32aa60` |
| `third_party/sunset/src/runner.rs` | `5cd9660d11e971205dc81599c7d049f68d9aca6164da0ee9daf0084ebf0b4f8a` |
| `third_party/sunset/src/encrypt.rs` | `eaa64854c069e2886c45d8a87f2065ed86df65da4e943a53384bfcc765f88bcd` |

Caveat: `cells/services/net/src/{service-runtime.rs,socket_table.rs}` were already modified in the working tree by the NET-GCD/OSD lanes when read (`git status`), so statements about the reaper (§3 H6) rest on the evidence (E7) and on the committed design, not on those two files' current bytes; the Net service bytes of the failing boot are the HEAD bytes, not the working tree's.

## 9. What this note does not claim

- No cause is asserted. §0.1: no product defect is determinable from the code; §3 ranks what remains.
- The `Bad packet length` ⇔ packet-start reading is inferred from OpenSSH behaviour and the sunset analogue, and is **to be measured by T-sig** before anyone leans on it; if T-sig contradicts it, H2-H4 (byte-granular causes) move up and §0.3 is withdrawn.
- No rate, no offset, no claim about QEMU/slirp/libslirp internals (not read, no network).
