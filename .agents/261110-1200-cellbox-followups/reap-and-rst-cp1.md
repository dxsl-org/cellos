# NET-GCD CP1: the graceful-close deadline aborts (RST) instead of removing silently, and the Tier-3 control flow's late RST explained (final text; decisions in section 16; documents only; nothing applied)

Written 2026-10-30 at HEAD `952d84c8aa6b6ede349c0447e965d1448d231463` (`docs(cellbox): CBX-24 met at stated ceiling; Phase 10 closed at stated ceilings`; `git status --short` empty at binding). Documents only: no source, script, lock, plan, image, key, network, build, test, Docker or QEMU action was made to produce it. Everything cited as `file:line` was read in this session; `[INFERENCE]` marks anything not observed in a file or command output; `OPEN (decider)` marks a decision for the named owner. Form follows `net-owner-death-cp1.md` (sections 7, 11, 12, 18) and `k4b-cp1.md`.

Inputs read (SHA-256): `net-owner-death-cp1.md` `c40fdf98ebaee4de89bb13acc64a2bc5a03ec4a2d83bae6cb059729f61224965` (section 5.4, 5.7, D-NOD-4, D-NOD-7), `net-od-cp2-dossier.md` `d47ecfc9d1cab241f917d22bcc7ba365b0354e31968f5362ea5e151600066dc0`, `cp2-dossier-part5.md` `39e8aaae308749ee689d73a8dac7fd15b8bdc94407d868e0ed08b16e573511af` (section 5 last paragraph, section 12), `cp2-dossier-part4.md` `1cf7203a746311d75f96e84c1ba85aeccbaa43e5e6b2779cf6f9e869a2bb8f09` (section 5 probe table), `rerun-cp1.md` `9b54396e249a9a0b7c7ca3c9edb86dac866bc9a6616a9e5d078fbda30a48123f` (6.2 pins), `k4b-cp1.md` `e44e301751b6fe96f4ea242f2f50e3c73e64fdd0a73ea86d6a5ff38a12b13fba` (form), `261030-1000-kms-ssh-key-custody/cleanup/ci.yml.md` `fdbc8e1040a3db52d7f517cd5e2fadf4fb294c86993086e1524ff810037ea14f` (not relevant to this slice), `261006-0934-httpd-single-cell-concurrency/phase-03-network.md` `ac00a027b46eb3200e1e3aaa6fcadb99dd22a8c5770b4a2b9e3ba1fd2ca679bd` (step 5), `.agents/cell-contract-0024-locks.md` `1f674e010ab0715bf7b66c554e5d125b2d35b999fb7b1668441abda7227274c0`, `docs/code-standards.md` `9cb004adc0db8f4b1da77b78419507c3c936d76b23370e68752be54007da1185`. Evidence read (all under `.agents/261110-0900-tier3-guest-endpoint/evidence/`): `t3-d3/diag/pcap-summary.json` `5a1f4a5b5d8ab1b7dc0828535f3099ebc25876ab5b3fe353d2496d89f9ca9f26`, `t3-d3/diag/serial.log` `f1bd30f19dfd849dbdde2fe18ed60d806d936207b4e095834ed3c687c5de0a46`, `t3-d3/diag/verdict.json` `92d0bfc8820f6498acf5f969f5c8be244ac4eef9fb1e9529ecd6fd0a9c3087e2` (plus `timeline.json`, `diag-marks.json`), `t3-d2/pcap-summary.json` `05e5b1adce879904fabafebb05037a07f6ab68fd9e2f88f0f79cb67fb168329d`, `t3-d2-boot1/pcap-summary.json` `511ff2a1474713c1132dab04629332aff0d97c085f6c48fe61061baa63ce371d`. `k6-cp2-dossier.md` named in the assignment does **not exist** under `261030-1000-kms-ssh-key-custody/` (`sha256sum: No such file or directory`); nothing here depends on it. smoltcp 0.11.0 source (`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/smoltcp-0.11.0`, `Cargo.lock` checksum `5a1a996951e50b5971a2c8c0fa05a381480d70a933064245c4a223ddc87ccc97`): `socket/tcp.rs` `8cca5acf165fe9665f24d14b43162577b827f54c4e426da2b7c55c04f0f9de4d`, `iface/interface/mod.rs` `32a8b9104b303c6a473fd92e1c66531e5b6ccd063da3c2e0d348aa2a4ab8445a`.

## 0. Summary

| Item | Result |
|---|---|
| (a) The defect | `reap_graceful_closes` (`cells/services/net/src/service-runtime.rs:310-327`) removes a socket from the graceful-close ledger at its deadline with `SocketSet::remove`, which emits nothing, although its own doc comment says "the finite deadline is an explicit abort" and the public docs promise it (`libs/api/src/services/ipc.rs:808` "net reclaims at terminal/deadline"; `libs/ostd/src/clients/net.rs:165-167` "or aborts it after its own deadline"). The peer is never told by the service; it learns only if one of its later segments reaches the interface, which answers it with an RST (smoltcp `iface/interface/mod.rs:1207-1217`) at a time the peer chooses |
| (b) The Tier-3 RST | **Same defect, not a second one.** The control connection's flow A in T3-D3 carries one RST from the cell 11.250 s after its FIN (`pcap-summary.json` control A: `first_fin` 51.104, `first_rst` 62.354). It is the interface's unknown-segment RST for the first slirp segment that arrived after `reap_graceful_closes` had silently dropped the stuck FIN-WAIT-2 socket at its 10 s deadline (nominal). The same RST, 14.617 s and 22.848 s after the FIN, is in the two T3-D2 boots, which ran **before** NET-OD existed, so NET-OD's abort path is excluded. Ranked hypotheses and the evidence: section 3. Cause established by code reading plus three independent captures plus a negative control; **not** directly observed (the Net cell logs nothing at a graceful reap): confidence high, labelled accordingly |
| Fix | At the deadline the reaper decides by TCP state: `Closed` without a tuple or `TIME-WAIT`: free silently (nothing is owed); any other state (`FIN-WAIT-1`, `FIN-WAIT-2`, `CLOSING`, `LAST-ACK`, and a `Closed` socket that still owes an RST): `abort()` and hand the cap to the existing NET-OD abort ledger, so exactly one RST leaves on the next network turn and the slot is freed when it has left. One new pure decision function (`graceful_end`), a new `now` parameter on the reaper (as `reap_aborted` already has), one table method (`graceful_to_abort`), one guard (`begin_graceful_close` ignores a cap already aborting), one log line |
| What changes on the wire | The RST appears **at the deadline** (+ at most one to two loop turns, about 0.1-0.3 s) instead of at the peer's next probe (T3: 1.25 s to 12.9 s after the deadline). Every closed-in-time connection is unchanged (flush, FIN, orderly end) |
| Contract | None changes: the code is brought to what `ipc.rs:808`, `clients/net.rs:165-167` and the comment at `service-runtime.rs:310-311` already say. No `libs/api`/`libs/types` byte, no ABI, no kernel path (sections 6 and 11) |
| Files | 4 production/test paths in `service-net` plus `docs/network-api.md` and `CHANGELOG.md`; no new file (section 6, 7) |
| Proof | host tests only (section 8, 9). A QEMU observation is **not** needed to close the defect and is **not** buildable without a new lane CP1, because the T3 lane pins the bytes of two of these files (section 9.2) |
| Effort | S: about 45-65 production lines, 250-330 test lines [INFERENCE from the NET-OD sizing, not measured] |

## 1. Scope and non-scope

**In:** the deadline behaviour of the graceful-close ledger; the ledger consistency it needs; the one log line; host tests; docs; the diagnosis of the Tier-3 control-flow RST; the lock handoff.

**Out (each one is a separate decision, section 13):** legacy `TcpClose` / `TcpStream::drop` silence (`handlers/tcp.rs:111-120`, NET-OD 5.7 item 2); TLS handshake-in-flight silent drops; keepalive/user timeout; httpd's own close retry; the kernel; the T3 lane, judge, tokens; canonical or promoted image bytes; any `libs/` change; any edit by this document.

**Serves:** `httpd` phase-03 step 5 ("abort explicitly on deadline", `phase-03-network.md:22`) for the Net half; `net-owner-death-cp1.md` 5.7 item 1 and D-NOD-7 (the follow-up that approval deferred); Cellbox `cellbox-ssh`, which ends every session with `tcp_close_graceful` and exits (`cells/tools/cellbox/src/ssh.rs:1192,1260`). Not an Intel C2C outcome; not a CBX row change.

## 2. Finding (a): what the code does today (all read)

| Item | Fact | Where |
|---|---|---|
| Graceful request | `TcpCloseGraceful`: `socket.close()` then `begin_graceful_close(cap, now + 100_000_000)` (10 s at the NET 10 MHz timebase: `sys_get_time()` is HPET milliseconds times 10 000, `service-runtime.rs:105-108`, kernel `GetTime` op 1 reading `hal::hpet::now_ns()`, `syscall.rs:7225-7226`), reply `Ok`. A cap already in the ledger keeps its first deadline. `TcpSendReady` on it is refused; `TcpRecvReady` is not | `handlers/tcp.rs:245-262`, `:222-226` |
| States after `close()` | `Listen`/`SynSent` -> `Closed`; `SynReceived`/`Established` -> `FinWait1`; `CloseWait` -> `LastAck`; the others unchanged. So a graceful cap can only be in `Closed`, `FinWait1`, `FinWait2`, `Closing`, `LastAck`, `TimeWait` | smoltcp `tcp.rs:892-913` |
| The reaper | per loop turn, up to 8 times: pick the first ledger cap whose handle is gone **or** whose state is `Closed` **or** whose deadline passed; `remove_internal` + `sockets.remove`. Nothing is sent | `service-runtime.rs:312-327`, called at `:666` |
| Silent removal | `SocketSet::remove` "without changing its state"; the smoltcp interface does not dispatch for a socket that no longer exists | smoltcp `iface/socket_set.rs:120-130` (per NET-OD CP1 section 3), `iface/interface/mod.rs:1195-1217` |
| Why the deadline is reached | `FIN-WAIT-2` has no timer in smoltcp and the Net service sets no `set_timeout`/`set_keep_alive` (grep over `cells/services/net/src` outside tests: no match). A peer whose data is queued ahead of its FIN behind a zero window nobody drains cannot deliver its FIN, so the socket sits in `FIN-WAIT-2` until the ledger deadline. The owner is typically gone (`cellbox-ssh` exits right after the reply), so nothing ever reads the 4 KiB receive buffer | smoltcp `tcp.rs:1495-1500` (window probe answered with an ACK, which is the zero-window ACK seen in the pcaps), `:1624-1627` |
| What already works | an in-time close: peer FIN arrives, `TIME-WAIT`, smoltcp's own `CLOSE_DELAY` 10 s (`tcp.rs:267`, `dispatch` `should_close` -> `reset()` -> `Closed`, no tuple), the reaper sees `Closed` and frees. Unchanged by this slice | existing test `a_graceful_close_in_flight_is_flushed_not_pre_empted_and_only_its_reaper_ends_it`, `service-runtime-owner-death-tests.rs:296-326` |
| The NET-OD ledger this slice reuses | `abort()` sets `Closed`; with a tuple the next `dispatch` emits exactly one RST (`tcp.rs:2111-2113`, `:2157-2159`) and then clears the tuple (`:2327-2330`); `remote_endpoint()` is then `None`. `reap_aborted` frees on that predicate or at `ABORT_DRAIN_TICKS` (1 s nominal) | `socket_table.rs:216-279`, `service-runtime.rs:42-47,329-345`, `:667` |
| Callers of the graceful close | exactly two: `cellbox-ssh` (`ssh.rs:1192,1260`) and `httpd` (`reactor.rs:568`). httpd's own `CLOSE_TIMEOUT` is 1 s (`reactor.rs:18`) after which it retries with the **legacy** `TcpClose` (`:567`, `:626-628`), which removes silently (`handlers/tcp.rs:111-120`); so httpd reaches the 10 s Net deadline rarely, and `cellbox-ssh` (no retry; the client is gone) is the path that does | greps over `cells libs` |
| Doc-comment drift | `service-runtime.rs:310-311` "the finite deadline is an explicit abort of a peer that never acknowledges or closes" describes the intended code; `network-api.md:203-204` records the gap: "the graceful-close deadline still removes a socket silently (not an abort)" | |

The defect is therefore: **the documented explicit abort at the deadline was never implemented; the code does the silent removal NET-OD already proved to leave the peer half-open.** Its observable form is the delayed RST of (b).

## 3. Finding (b): the control flow's RST, diagnosed from existing evidence and code

### 3.1 Observations (files and values)

All times are seconds after QEMU start from the judge's pcap summary (`first_fin.t`, `first_rst.t`; the three summaries are independent boots; direction `c2s` is 10.0.2.15 -> 10.0.2.2, i.e. the cell, because the cell sent the SYN).

| Boot | HEAD / Net build | Control A FIN (cell) | Control A RST (cell) | RST - FIN | slirp FIN on A (`fin.s2c`) | A s2c payload | B s2c payload | not delivered to the cell | zero-window ACKs from the cell (total / after cancel-done) |
|---|---|---|---|---|---|---|---|---|---|
| T3-D2 boot 2 (`evidence/t3-d2/`) | before NET-OD (owner-death watch refused) | 46.682 | 61.299 | **14.617 s** | 0 | 639 811 | 696 021 | 56 210 | 16 / 2 |
| T3-D2 boot 1 (`evidence/t3-d2-boot1/`, times unshifted in the summary) | before NET-OD | 52.154 (-25147.846 + 25200) | 75.002 (-25124.998 + 25200) | **22.848 s** | 0 | 976 292 | 1 037 289 | 60 997 | 27 / 0 |
| T3-D3 diag (`evidence/t3-d3/diag/`) | NET-OD, HEAD `fc496bea0` | 51.104 | 62.354 | **11.250 s** | 0 | 895 874 | 937 449 | 41 575 | 10 / 1 |

The same boots on flow B (guest side): FIN from slirp and from the guest, flow over by 51.16 s in T3-D3 (`control.B.last_t`), i.e. slirp finished the guest side long before the RST. Cell-side serial for T3-D3: `serial.log:698-736` the control case, `:732` `control-client … st=124 ms=13252 … outcome=timeout` (the client ended by its own timeout, cooperatively: `tcp_close_graceful`, FIN), **no** `[net]` owner-death line between the control case and the cancel case other than the cancel client's own (`:748`, `cell 10 gen 129: aborted 1 TCP, released 0 other`).

Negative control, same mechanism without the reap (`cp2-dossier-part4.md` section 5, line 51): in T3-D2 the leaked **cancel** socket stayed alive (no graceful ledger, no owner-death) and slirp's persist probes hit it at +3.70 s and 59.29, 64.32, 70.35, 82.41, 106.54, 154.81 s (gaps 5.03, 5.03, 6.03, 12.06, 24.13, 48.27 s: a doubling backoff); the cell answered six of seven with window 0 and **never sent an RST in 106 s**. A live socket answers slirp's probes with a zero-window ACK; it does not reset.

### 3.2 Hypotheses, ranked

| # | Hypothesis | Verdict | Evidence |
|---|---|---|---|
| **H1** | The control cap sat in the graceful ledger in `FIN-WAIT-2` (our FIN acked, the peer's FIN stuck behind queued data and a zero window); `reap_graceful_closes` removed it silently at its deadline (FIN request time + 10 s); slirp's next persist probe or data segment reached the interface with no socket and got the unknown-segment RST (`iface/interface/mod.rs:1207-1217`, `rst_reply` `tcp.rs:1248-1262`) | **Cause, high confidence; not directly observed** | (1) RST - FIN is 11.25, 14.62, 22.85 s: all three are above the 10 s deadline and none is a fixed number, as expected if the RST waits for the peer's next segment after a fixed deadline (slirp's probe gap doubles, part 4 line 51: the lag after the deadline would be at most one gap). (2) 41-61 KB that the guest sent on B never arrived on A in any boot and slirp sent **no FIN on A in any boot** (`fin.s2c` 0): the FIN could not complete, so only the deadline can end the socket. (3) T3-D2 shows the RST with no NET-OD at all (watch refused, `release_owner` unreachable): NET-OD's abort cannot be the cause. (4) the cell was still answering slirp with window 0 after its own FIN: `zero_win_after_cancel` is 2 (T3-D2) and 1 (T3-D3) for control A, where `cancel-done` is 3.9 s (T3-D2: 50.558 - 46.682) and 3.6 s (T3-D3: 54.698 - 51.104) after the FIN, i.e. the socket existed and answered at least that long after the FIN; and in T3-D3 the RST is the last packet of the flow (`last_t` = `first_rst.t` = 62.354) with `zero_win_after_first_rst` 0: "alive and answering, then gone, then one RST". (5) the negative control of 3.1. (6) code: no RST source exists for a live post-close socket, rows H3 and H5 |
| H1b | The socket reached `TIME-WAIT` (peer FIN received) and smoltcp's 10 s `CLOSE_DELAY` closed it, then the reaper freed it, then the next segment was reset | Refuted | `TIME-WAIT` needs the peer's FIN; slirp sent none on A in any boot (`fin.s2c` 0). (It would be the same defect class, silent removal followed by an interface RST, with a 10 s timer instead of the ledger deadline; it is also handled by the fix, as `Free` when no frame is owed) |
| H2 | NET-OD's `abort_owner` (owner death or the stale-generation sweep) aborted the control socket | Refuted | present in T3-D2 before NET-OD existed; `abort_owner` skips caps in the graceful ledger (`socket_table.rs:245`, D-NOD-4); no `[net] owner death` line belongs to the control client (`aborted 0, released 0` prints nothing, `owner_death_line` `service-runtime.rs:493-500`); an abort would put the RST at the control client's death (about 51.1 s) or at the stale sweep triggered by the cancel client's first request (about 53.5 s), not at 62.354 s |
| H3 | smoltcp itself resets a live post-close socket (data after close, bad ACK) | Refuted by code | the only `rst_reply` sites inside a socket are `SynSent`/`SynReceived` cases (`tcp.rs:1414,1433,1444`); a segment outside the window or a window probe gets an ACK (`challenge_ack_reply`, `:1477`, `:1564`); `abort()` is the only other source and only the service calls it (reclaim paths) |
| H4 | Not the cell's RST (capture direction artefact) | Refuted | the summary classifies by the SYN sender; `10.0.2.15` is the cell and the guest is `10.0.2.16`; the cancel flow in the same capture shows the NET-OD RST with the same classification |
| H5 | a Net-side timeout or keepalive | Refuted | no `set_timeout`, `set_keep_alive` in `cells/services/net/src` (non-test grep: no match); smoltcp defaults are `None` |

### 3.3 What is not proven, and the falsifiers

The Net cell logs nothing at a graceful reap, so the removal time is inferred, not observed. The clock relation (guest HPET virtual clock against the pcap's host wall clock under TCG without `-icount`) is [INFERENCE]; the three Deltas bound it from below only. **H1 predicts, for any capture after the fix of section 5:** RST - FIN = 10.0 s to about 10.4 s on every boot, independent of slirp's probe phase (the RST is sent by the service at the deadline, not on a peer segment), and with the section-5.6 log line the serial shows `[net] graceful close deadline: aborted 1 TCP` at the same instant. If a post-fix capture still shows a spread like 11 to 23 s, H1 is wrong. (No such capture is requested: section 9.2.)

### 3.4 Is it a separate defect?

No. One cause, one fix. The RST is not harmful to any T3 criterion (the control criteria are FIN seen, producer gone, close logged, `cp2-dossier-part5.md` section 5; the judge reads `control_A_rst_from_cell` as data only, `tier3_ssh_guest_judge.py:494`). It is a symptom of the silent removal: the peer's connection is reset **by accident, late and at the peer's rhythm**, instead of **by decision, on time**. The fix makes it deterministic and, for the same reason, bounds the leak window: today a peer that never probes again (an idle half-open flow that is not behind a zero window) is never told at all.

## 4. Design constraints that fix the shape

1. The deadline is the documented bound of the "send queued output, then FIN" promise: after it, a peer that has not acknowledged or closed gets an abort (`ipc.rs:808`, `clients/net.rs:165-167`).
2. NET-OD already built the right primitive: `abort()` then the abort ledger that waits until the RST has left (`socket_table.rs:216-279`, `service-runtime.rs:329-345`). Reuse it; add no second RST path.
3. Nothing owed means nothing sent: a closed-cleanly socket (`Closed` without a tuple, `TIME-WAIT`) must not draw a gratuitous RST.
4. The reaper must stay bounded per turn (8 graceful entries, 32 abort entries) and must not call kernel services in testable code: `now` is a parameter, as for `reap_aborted` (the host `sys_get_time`/`println` are real Linux syscalls with a ViSyscall number, NET-OD dossier deviation 3).
5. A cap is in at most one ledger; an already-aborting cap must never be dropped before its RST left.

## 5. Design

### 5.1 Options considered (D-GCD-1)

| Option | What the deadline does | Verdict |
|---|---|---|
| **A (recommended)** | state-aware: free silently when nothing is owed, otherwise `abort()` and move the cap to the abort ledger (one RST, then free) | smallest change that implements the documented contract; reuses NET-OD; bounded time and slots |
| B "continued FIN retransmission" | keep the socket alive past the deadline while it is in `FIN-WAIT-1`/`CLOSING`/`LAST-ACK` (our FIN not yet acked), up to a second hard cap, then abort | rejected. smoltcp already retransmits the FIN with backoff for the whole 10 s window (`tcp.rs` retransmit timer in `dispatch`); extending it only delays the same decision and doubles or sextuples the slot hold time of every stuck peer. The slots are the resource httpd phase 03 bounds (`HTTP_OWNER_BUDGET` 258 of 290, `socket_table.rs:16-20`). It also needs a second constant and a second proof. The question it answers ("slow but alive peer acks late") is already served by the 10 s window |
| C drain-and-discard | after a graceful close, the reaper `recv`s and discards the receive buffer so a flooding peer can finish and FIN | rejected for this slice. It silently throws away peer data after the owner declared it will not read (RFC 1122 4.2.2.13: if data arrives after CLOSE "its TCP SHOULD send an RST to show that data was lost"), lets every closed socket absorb line-rate traffic for 10 s (up to 290 sockets), and changes what `TcpRecvReady` on a closing cap means. It would have hidden the T3 symptom, not fixed the contract |
| D Linux-like RST at `close()` when unread data exists | RST immediately instead of FIN if the receive buffer is non-empty | rejected: it would discard the **send** queue that `TcpCloseGraceful` promises to flush, a regression for httpd (unread pipelined request bytes at close) and `cellbox-ssh` (a server's last bytes unread at exit) |
| E leave it, document it | `network-api.md:203-204` stands | rejected: it keeps the documented-but-unimplemented abort and the slot/half-open exposure the Phase-03 plan already lists as step 5 |

### 5.2 Pure decision (testable without a socket)

```rust
/// What a graceful-close ledger entry needs from the reaper this turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GracefulEnd {
    /// Nothing is owed on the wire: free the slot.
    Free,
    /// The peer must be told: `abort()` (one RST), drain through the abort ledger.
    Abort,
    /// Still closing and inside its deadline.
    Keep,
}

fn graceful_end(state: tcp::State, rst_owed: bool, expired: bool) -> GracefulEnd {
    use tcp::State;
    match state {
        State::Closed if rst_owed => GracefulEnd::Abort, // smoltcp already aborted it and still owes the RST
        State::Closed => GracefulEnd::Free,              // peer FIN/ACK done, peer RST, or never connected
        State::TimeWait if expired => GracefulEnd::Free, // both FINs exchanged and acked: a clean end
        State::TimeWait => GracefulEnd::Keep,            // smoltcp's own 10 s timer finishes it
        _ if expired => GracefulEnd::Abort,              // FinWait1/FinWait2/Closing/LastAck (and any state not expected after close())
        _ => GracefulEnd::Keep,
    }
}
```

`rst_owed` is `remote_endpoint().is_some()`: smoltcp clears the tuple when a received RST closes the socket (`tcp.rs:1624-1627`) or `reset()` runs (`:724`), and keeps it only after `abort()` or a timeout (`:2327-2330`, `timed_out` `:1958-1963`; the Net service sets no timeout, so `Closed`-with-tuple is reachable only through `abort()` and exists to make the predicate exact rather than to cover a known path).

### 5.3 The reaper (replaces `service-runtime.rs:310-327`)

```rust
/// Graceful closes finished per loop turn.
const GRACEFUL_REAP_PER_TURN: usize = 8;

/// FIN queues output before the close. A finished close frees the slot; the finite
/// deadline is an explicit abort of a peer that never acknowledges or closes: one RST,
/// then the abort ledger waits until it has left. `now` is passed in so the deadline is
/// testable. Returns how many sockets were aborted this turn.
fn reap_graceful_closes(table: &mut SocketTable, sockets: &mut SocketSet<'_>, now: u64) -> usize {
    let mut aborted = 0;
    for _ in 0..GRACEFUL_REAP_PER_TURN {
        let next = table.graceful_closes().find_map(|(cap, deadline)| {
            let end = match table.get_unchecked(cap) {
                None => GracefulEnd::Free,
                Some(handle) => {
                    let socket = sockets.get::<tcp::Socket>(handle);
                    graceful_end(socket.state(), socket.remote_endpoint().is_some(), now >= deadline)
                }
            };
            (end != GracefulEnd::Keep).then_some((cap, end))
        });
        let Some((cap, end)) = next else { break };
        if end == GracefulEnd::Abort {
            if let Some(handle) = table.get_unchecked(cap) {
                let socket = sockets.get_mut::<tcp::Socket>(handle);
                socket.abort();
                if socket.remote_endpoint().is_some() {
                    table.graceful_to_abort(cap, now.saturating_add(ABORT_DRAIN_TICKS));
                    aborted += 1;
                    continue;
                }
            }
        }
        if let Some(handle) = table.remove_internal(cap) {
            sockets.remove(handle);
        }
    }
    aborted
}
```

Loop call (`:666`): `let aborted = reap_graceful_closes(&mut table, &mut sockets, sys_get_time()); if let Some(line) = graceful_deadline_line(aborted) { println(&line); }`, then the existing `reap_aborted` line. Order inside a turn is unchanged: `iface.poll` (top, `:657`) -> reaper aborts -> `reap_aborted` (RST not out yet: kept) -> idle wait (at most 10 scheduler ticks = 100 ms, `NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS`) -> next turn `iface.poll` emits the RST -> `reap_aborted` frees. **RST latency after the deadline: one to two turns, nominally at most about 0.1-0.3 s [INFERENCE: wake latency not measured, as in NET-OD 16].** The `hypervisor-bridge` flavour polls only inside `dispatch_admitted`; there the RST goes out at the next request or the 1 s drain fallback silently frees (same limit as NET-OD 5.1; that flavour is not built by the Tier-3 lane).

### 5.4 Table (`socket_table.rs`)

```rust
/// Queue FIN once and retain the first drain deadline. A cap whose RST is already owed
/// stays out of this ledger (a cap is in at most one).
pub fn begin_graceful_close(&mut self, cap: u64, deadline_ticks: u64) {
    if !self.aborts.contains_key(&cap) {
        self.graceful_closes.entry(cap).or_insert(deadline_ticks);
    }
}

/// The graceful deadline passed and the socket was aborted: its RST is owed. Moves the
/// cap from the graceful ledger to the abort ledger (first abort deadline wins).
pub fn graceful_to_abort(&mut self, cap: u64, deadline_ticks: u64) {
    self.graceful_closes.remove(&cap);
    self.begin_abort(cap, deadline_ticks);
}
```

`abort_owner` is unchanged: it skips `is_graceful_closing || is_aborting` (`socket_table.rs:245`), so a dead owner's graceful cap is first flushed under its own deadline (D-NOD-4) and now **ends with the RST** instead of silently. `remove_internal` already clears both ledgers (`:295-296`). `handlers/tcp.rs` is **not** edited and stays byte-identical (pinned): a repeated `TcpCloseGraceful` on a cap that is already aborting re-enters `begin_graceful_close`, which now ignores it; a legacy `TcpClose` on an aborting cap still removes it immediately (the documented silent legacy path), which can lose that one RST within the at most one-to-two-turn window between the abort and its emission, and then falls back to the pre-fix behaviour for that cap (peer reset by its next segment). Stated limit, section 12 R-4.

What an owner sees on an aborting cap during that window: `readiness` reports `ERROR` (state `Closed`, no longer graceful, `handlers/tcp.rs:359-361`) and `EOF`; before the fix the cap was already gone and readiness returned the generic error for an unknown cap (`:323-325`). Both are terminal answers for a closed cap; no owner relies on the old one (the callers are `cellbox-ssh`, gone, and httpd, which has already retried with the legacy close, section 2).

### 5.5 Wire behaviour per state at the deadline (table)

| State at the deadline | Meaning | Action | Wire |
|---|---|---|---|
| `FIN-WAIT-2` | our FIN acked, peer neither closed nor drained (the T3 control) | `abort()` + abort ledger | one RST |
| `FIN-WAIT-1` | our FIN (or queued output before it) not acked in 10 s | same | one RST |
| `CLOSING`, `LAST-ACK` | simultaneous close or passive close whose final ACK never came | same | one RST |
| `TIME-WAIT` | clean end, smoltcp's own 10 s timer still running | free silently | none |
| `Closed`, no tuple | closed in time, peer RST received, listener or never connected | free | none |
| `Closed`, tuple kept | an RST is already owed (smoltcp `abort()`) | move to the abort ledger | the owed RST |
| any state before the deadline | still inside the promise | keep | none |

### 5.6 Log line (D-GCD-5)

```rust
/// One line per reaper turn that aborted something (normal clients finish inside the
/// deadline, so it appears only for stuck peers; same rule as the owner-death line).
fn graceful_deadline_line(aborted: usize) -> Option<String> {
    (aborted > 0).then(|| alloc::format!("[net] graceful close deadline: aborted {aborted} TCP"))
}
```

It gives the otherwise missing direct observation of the reap (section 3.3) in the serial log of any later run and tells an operator why a peer saw an RST. No script greps it (the lane's judge matches `[net] owner death:` and the watch-unavailable string only; the new line contains neither).

### 5.7 Docs

`docs/network-api.md`: replace the closing paragraph of "Owner death" (`:203-206`, "the graceful-close deadline still removes a socket silently") by the corrected "Not changed" list and add a short "Graceful close and its deadline" subsection: flush then FIN; deadline 10 s nominal; at the deadline the table in 5.5; the RST goes out on the next turn; unread data and unflushed output are discarded by the abort; no FIN-retransmission extension; the legacy `TcpClose` remains silent. `CHANGELOG.md`: one bullet under `[Unreleased]` / `### Fixed`.

## 6. Exact file closure (pre-edit SHA-256 at HEAD `952d84c8a`; `git status --short` empty)

| Path | Action | SHA-256 before | Lines now | Locked surface |
|---|---|---|---|---|
| `cells/services/net/src/service-runtime.rs` | edit: `GracefulEnd`, `graceful_end`, `GRACEFUL_REAP_PER_TURN`, new `reap_graceful_closes` (replaces `:310-327`), `graceful_deadline_line`, the call at `:666` | `89d9334a01d047c93e3ff97352b7a99a460d9093d4a9dc9ac8d97044e7db811e` | 802 | Net service (L38 step 5; L37/L32 adjacency) |
| `cells/services/net/src/socket_table.rs` | edit: `graceful_to_abort`, guard in `begin_graceful_close` | `1964bc9d0ee14d9db1fd6d60715c65d4c0cfda1edda01c2999705903ff456ecb` | 307 | same |
| `cells/services/net/src/socket_table/tests.rs` | edit: one new test (section 8.2) | `28529be349558c9829d0b76dcb459fef5223efdb7b6d25d06741a1a62c20a6ef` | 307 | tests |
| `cells/services/net/src/service-runtime-owner-death-tests.rs` | edit: pass `now` at `:318` and `:323`; append the section-8.1 tests (harness `Stack` reused) | `3c12780f690adb5a1629a1ccc0fdcab9515035e66e3a917c6cc369343de807a5` | 371 | tests |
| `docs/network-api.md` | edit at CP2 (5.7) | `5879be3971ed6b7d2782e804eb316a9f2733596b757fe0c90b5960a8e234b2d3` | 296 | docs |
| `CHANGELOG.md` | edit at CP2 (one bullet) | `c5ef0411d34cd77667a91ca71f8d937fe8ff819d3c22228e036a6ede3f6aa5db` | 1155 | docs |

Four source/test paths, no new file, two docs. **Pinned, must stay byte-identical (re-verified at the start of the slice):**

| Path | SHA-256 |
|---|---|
| `cells/services/net/src/handlers/tcp.rs` | `3f1a7611f121586ab278eda239290db3d79a7777b9f7fd3e04fbc91487b72f98` |
| `cells/services/net/src/tls/dispatch.rs` | `1d3feea4cb1cc986365bb23dfbdc4829764fbb29185a4e27181655b024adc3b5` |
| `cells/services/net/src/tls_handler.rs` | `fa39844cc59179943d8b90f8c4d9e4014cd48e572b501966437966be1e837e21` |
| `cells/services/net/src/main.rs` | `76c486d06511d0ab72c567c2318b05326c88a2e795abdcc917a4116967766ced` |
| `cells/services/net/Cargo.toml` | `aaab0a0c175e3e0eb3bbfb9606caf80c188fbb57292f4a9be665299096b6f33f` |
| `Cargo.lock` | `5181251ccc712f27f71db9861b1b4feab0a8fec910e43716a7b744a4f8c81130` |
| `libs/api/src/services/ipc.rs` | `53600a6a38bf48f72600af4d884035726af21db23c05bd16189610e9a54e9ba4` |
| `libs/api/src/abi/syscall.rs` | `b202dfa28f461b2c8f9940901294187e65b6ef4ff94638dc1004c01d78325957` |
| `libs/ostd/src/clients/net.rs` | `2344128b467f292f42d4dc4be6a2b45f56c62c4b6983dbca7be34bf156de54b4` |
| `cells/tools/cellbox/src/ssh.rs` | `b71ebc459312c34e4282c93bc5140206b43033181b68fe2fb5414929ec170401` |
| `cells/services/httpd/src/reactor.rs` | `b0410e0fa451c9e4fb2d95639662617db9d7f80c01d68a735aa6e8be4aaa557f` |
| `kernel/src/task/scheduler.rs` / `syscall.rs` / `tcb.rs` / `kernel/src/main.rs` | `e49d5fea…11d2` / `c00a71b1…3b85` / `3e888023…cdf61` / `a9636fee…8c64` (full values in `rerun-cp1.md` 6.2) |
| `scripts/qemu-tier3-ssh-guest-x86.sh` / `tier3_ssh_guest_judge.py` / `test_tier3_ssh_guest_judge.py` | `8b3a8b0cb440e8fd92cfcfe19a5bc58d8121ed302564fb6bab2bcf2cea1fcb45` / `93bbcb2d44f5d09050e00fec759c0f245a2f908555bd058598ddfcd64ef376b4` / `994db398f075616b366db398236c7dd99d8fdd2aae44196f63ccbef26eb55249` |
| `libs/api` + `libs/types` tree (recipe: `find libs/api libs/types -type f -name '*.rs' \| sort \| xargs sha256sum \| sha256sum`) | `5828504db66404f3138476ccca0c84d712c9284f5d4ee30d40f9f025fe5be290` |

Any other path discovered mid-slice stops the slice and returns to the owner as an amendment. No dependency, feature, manifest, allowlist, `Cargo.lock` or `libs/` change. The Tier-3 lane's own pin list (`qemu-tier3-ssh-guest-x86.sh:135-154`, all 18 lines verified OK at this HEAD) includes **`socket_table.rs`, `socket_table/tests.rs`, `service-runtime.rs`, `service-runtime-owner-death-tests.rs`**: landing this slice makes the lane's preflight refuse (`product bytes differ from rerun-cp1.md section 6.2`) until a lane CP1 refreshes the pins (section 9.2, D-GCD-7).

## 7. New files

None. (A separate test file would need either a copy of the 110-line `Stack` harness or a visibility change on its type and fields in the existing file; D-GCD-9 records the choice.)

## 8. Tests (host; `cargo test --offline --locked -p service-net --target x86_64-unknown-linux-gnu`)

Harness: the existing `Stack` over `smoltcp::phy::Loopback` (`service-runtime-owner-death-tests.rs:23-138`: 256-byte socket buffers, `established(owner)` returns the client cap and both handles, `pump`, `advance(ms)`, `reclaim`, `sockets_alive`). `now` for the reaper is an explicit tick number in every test. Expected results are written from the code reading and the NET-OD tests that already pass over the same harness; **none was run** [INFERENCE until CP2].

### 8.1 `service-runtime-owner-death-tests.rs` (appended section "graceful close deadline"; 10 new tests, 2 call-site retargets)

| # | Test | Setup and assertions |
|---|---|---|
| G1 | `graceful_end_decides_every_state` | pure table: all 11 smoltcp states (`Closed`, `Listen`, `SynSent`, `SynReceived`, `Established`, `FinWait1`, `FinWait2`, `CloseWait`, `Closing`, `LastAck`, `TimeWait`) x `rst_owed` x `expired` against the 5.2 rules, written as an explicit expectation list (not by calling the function on itself); includes `Closed`/owed -> `Abort` even when not expired, `TimeWait` not expired -> `Keep`, expired -> `Free` |
| G2 | `fin_wait2_peer_with_data_behind_a_closed_window_is_reset_at_the_deadline` (the T3 control replica) | `established(OWNER)`; server `send_slice` 256 bytes twice with `pump` between so the client's receive buffer is full (`recv_queue()` 256, window 0) and 256 more bytes stay queued at the server; `client.close()` + `begin_graceful_close(cap, 10_000)`; `pump(10)`; assert client `FinWait2`, server `CloseWait`, server `send_queue() > 0`. `reap_graceful_closes(.., 9_999)` -> 0, cap still graceful, client `FinWait2`. At `10_000` -> 1: client `Closed` **with** `remote_endpoint().is_some()`, `is_aborting(cap)`, `!is_graceful_closing(cap)`, handle still in the table. `pump(2)`: server `Closed`, `recv` errs, client tuple gone. `reap_aborted(.., 0)` frees; `sockets_alive` back to baseline - 1; `owned_cap(OWNER)` none |
| G3 | `characterisation_a_silent_removal_resets_the_peer_only_on_its_next_segment` (documents the old mechanism and pins the smoltcp rule the diagnosis rests on) | two stacks. (i) client `close()` + FIN-WAIT-2 as in G2 (no window game), server in `CloseWait` quiet; the cap is removed with `remove_internal` + `sockets.remove` (the old reaper's action); `pump(20)`: server still `CloseWait` (nobody told). (ii) same, but the server `send_slice(b"late")` immediately before the removal and no `pump` in between; `pump(4)`: server `Closed` (RST from the interface's unknown-segment rule). This is the T3-control signature: the RST exists, but only because the peer spoke |
| G4 | `a_time_wait_or_closed_socket_is_freed_at_the_deadline_without_an_rst` | `established`; client `close()`, `begin_graceful_close(cap, 100)`; server `close()`; `pump(4)`: client `TimeWait`, server `Closed`. `reap(.., 99)` -> 0, kept; `reap(.., 100)` -> 0 (`TimeWait`, expired -> `Free`), cap gone, `aborting().count() == 0`, `graceful_closes().count() == 0` |
| G5 | `fin_wait1_closing_and_last_ack_are_aborted_at_the_deadline` | (a) `FinWait1`: `established`, `client.close()`, `begin_graceful_close(cap, 100)`, no `pump`, `reap(.., 100)` -> 1, `pump(2)`: server `Closed` (reset). (b) `LastAck`: `established`, `server.close()`, `pump(4)` (client `CloseWait`), `client.close()` (`LastAck`), `begin_graceful_close`, no `pump`, `reap(.., deadline)` -> 1, `pump(2)`: server not left half-open (`Closed`). `CLOSING` is covered by G1 (pure) because loopback cannot hold it between polls |
| G6 | `a_close_that_finishes_before_its_deadline_keeps_its_output_and_is_never_reset` | `established`; `client.send_slice` 100 bytes, `close()`, `begin_graceful_close(cap, 10_000)`; `pump(6)`; the server reads exactly the 100 bytes then `may_recv()` is false (FIN); `server.close()`, `pump(4)`: client `TimeWait`; `reap(.., 5_000)` -> 0; `advance(11_000)`, `advance(10)`: client `Closed` without tuple; `reap(.., 5_000)` -> 0 and the cap is freed (still inside its deadline); server `Closed` cleanly (no reset); `aborting().count() == 0` |
| G7 | `an_rst_that_smoltcp_already_owes_is_not_dropped_by_the_graceful_reaper` | `established`; `client.close()`; `client.abort()` (smoltcp-originated abort); `begin_graceful_close(cap, u64::MAX)`; `reap(.., 0)` -> 1 and the cap is in the abort ledger and not freed; `pump(2)`: server `Closed`; `reap_aborted` frees |
| G8 | `graceful_reap_is_bounded_per_turn_and_finishes_every_entry_once` | 20 `established` pairs, all closed and in the ledger with deadline 100 and no `pump` (all `FinWait1`); `reap(.., 100)` returns `GRACEFUL_REAP_PER_TURN` (8), then 8, then 4, then 0; after each call `graceful_closes().count()` is 12, 4, 0, 0 and `aborting().count()` 8, 16, 20, 20; `pump(2)`: every server `Closed`; two `reap_aborted` turns of up to `ABORT_REAP_PER_TURN` free all; `sockets_alive` and the table are back to baseline |
| G9 | `a_dead_owner_graceful_cap_is_flushed_then_aborted_at_its_deadline` (NET-OD x NET-GCD, the whole T3 control path) | the G2 state, owner `OWNER`; `reclaim(OWNER, u64::MAX)` == `(0, 0)` (skipped, D-NOD-4); `owner_death_line(OWNER, 0, 0)` is `None` (the control client's death logged nothing, as observed); cap graceful and `FinWait2`; `reap(.., deadline)` -> 1; `graceful_deadline_line(1)` is `Some("[net] graceful close deadline: aborted 1 TCP")`; peer reset after `pump`; ledger and sockets at baseline |
| G10 | `the_deadline_line_appears_only_when_something_was_aborted` | `graceful_deadline_line(0)` is `None`; `(1)` and `(3)` format as written |
| (retarget) | `a_graceful_close_in_flight_is_flushed_not_pre_empted_and_only_its_reaper_ends_it` | the two calls (`:318`, `:323`) gain the `now` argument `0`; the test's expectations stay (the first call is inside `u64::MAX`, the second finds `Closed`) and prove the in-time path is unchanged |

### 8.2 `socket_table/tests.rs` (1 new)

`graceful_to_abort_keeps_a_cap_in_at_most_one_ledger`: a connected cap with `begin_graceful_close(cap, 100)`; `graceful_to_abort(cap, 500)` -> `!is_graceful_closing`, `aborting()` == `[(cap, 500)]`; `begin_graceful_close(cap, 900)` is ignored (`graceful_closes().count() == 0`); a second `graceful_to_abort(cap, 700)` keeps deadline 500 (first wins); `abort_owner` on the same owner returns `(0, 0)` and skips it (`is_aborting`); `remove_internal(cap)` clears both ledgers and the budget returns (`can_insert`).

Totals: 11 new tests, 2 retargeted lines. Baseline at the CP2 of NET-OD: 65 passed (`net-od-cp2-dossier.md` section 1); **not re-measured at this HEAD**; no commit has touched `cells/services/net` since `d42e70d6a` (`git log` on the path), so the expected baseline is 65 and the expected result 76 [INFERENCE]. The baseline must be re-captured before the first edit.

Not testable on the host (stated, not hidden): the bare-metal `run()` loop (only its call line changes), the wake latency to the next poll, slirp's behaviour for the RST.

## 9. Verification and run budget

### 9.1 What the main agent runs at CP2 (host, no QEMU)

1. Before any edit: `git status --short` empty; the section-6 hashes re-taken equal; baseline `cargo test --offline --locked -p service-net --target x86_64-unknown-linux-gnu` and `cargo clippy --offline --locked -p service-net --all-targets` counts recorded (NET-OD recorded clippy bin 6 / bin test 50 warnings).
2. After the edits: the same two commands (expect +11 tests, no new clippy finding in an added line, three consecutive test runs equal); `cargo build --release --locked --offline -p service-net --target x86_64-unknown-none -Z build-std=core,alloc` (the lane's build form, `qemu-tier3-ssh-guest-x86.sh:255-266`; the debug bare-metal profile does not link even at baseline, NET-OD dossier section 4 last bullet); `python3 scripts/cellos-sign --check` rc 0 (F1/F5); the pinned list of section 6 `sha256sum -c` OK; `git status --short` shows exactly the four source/test paths plus the two docs.
3. Recorded in the CP2 dossier: before/after counts, the pre/post digests, and the statement that no QEMU observation was made.

### 9.2 QEMU: not required, not requested, and what it would cost

The defect is a deterministic reaper decision proven on real smoltcp sockets over loopback (G2, G5, G7, G8, G9); the RST on a real wire through slirp is the unchanged NET-OD mechanism that T3-D3 already observed for the cancel flow (RST precedes `cancel-done`, one RST, no FIN). **No T3 run is needed to close (a) or (b).** What a run would add is H1's falsifier of 3.3 (RST - FIN about 10 s). To take one:

- The lane **refuses** after the edit (section 6, pins). A lane CP1 ("T3-D4") would have to refresh the four pins (and the `rerun-cp1.md` 6.1/6.2 digests it quotes) and claim a token. All of `T3D3_DIAG_CONSUMED-1`, `T3D3_FULL_CONSUMED-1` are spent; `T3D3_CONFIRM_CONSUMED-1` is unspent but its rule reserves it for a host/lane defect (`rerun-cp1.md` L3), not for a product change. So a new token prefix is a lane edit.
- Budget if chosen: **one `--stage diag` boot** (observed 138.6 s elapsed, window cap 3600 s; `cp2-dossier-part5.md` section 5) with an additional pure observation (no criterion): control A `first_rst.t - first_fin.t` already derivable from the existing `pcap-summary.json` fields, plus the serial line of 5.6. A `full` boot is not needed (no marker depends on this).
- No judge edit is needed: the control RST is data-only today and stays so.

Recommendation: **do not run** (D-GCD-7 A). The T3 evidence stays as the historical record of the symptom; the host tests are the proof; the lane's staleness is then recorded rather than silently inherited.

## 10. Wording for CP2 (when, and only when, the host results exist)

"The Net service now implements the documented abort at the graceful-close deadline: a socket still in `FIN-WAIT-1`, `FIN-WAIT-2`, `CLOSING` or `LAST-ACK` ten seconds (nominal) after `TcpCloseGraceful` is aborted, one RST leaves on the next network turn and the slot is freed when it has left; a socket that is `Closed` or in `TIME-WAIT` is freed without any wire effect. This replaces the silent removal that left the peer to be reset by its own next segment (the 11-23 s late RST on the Tier-3 control flow, `evidence/t3-d3/diag/`, `evidence/t3-d2*/`). Host tests only; no QEMU observation of the new behaviour; the promoted/canonical image bytes keep the old reaper until a re-promotion." No acceptance row changes.

## 11. Law-1 analysis

`code-standards.md` scopes Law 1 to `libs/api/` and `libs/types/`. This slice changes **no** byte of either tree (tree digest in section 6; `libs/api/src/services/ipc.rs`, `libs/api/src/abi/syscall.rs` pinned individually) and no constant, payload or discriminant they define: `NetRequest::TcpCloseGraceful { cap_id }` and its `Ok`/`Err` reply are untouched; the doc comments that describe the deadline abort (`ipc.rs:808`, `clients/net.rs:165-167`) become **true** without an edit. Nothing in `libs/ostd` changes. No syscall, allowlist, manifest, policy, CapSet or launch-edge change; no kernel path. **No two-confirmation checkpoint is requested**; the owner is asked for an explicit "no ABI delta" acknowledgement (D-GCD-8). One behavioural point the owner should see because existing callers can observe it without a byte change: a graceful cap that outlives its deadline is now reset on the wire (peer sees "connection reset") rather than left half-open until the peer next speaks; unflushed output and unread data are discarded at that point (they were already unreachable once the deadline removed the socket).

## 12. Risks

- **R-1 Truncation at the deadline.** A slow but alive peer that has not acked the flush or closed within 10 s now gets an RST instead of a socket that vanishes. That is the documented bound (`clients/net.rs:165-167`), the data was already lost under the silent removal; the peer is told now. Option B (extend) is rejected for slot exposure (5.1).
- **R-2 RST bursts.** Up to 8 aborts per reaper turn and 32 drained per turn (`GRACEFUL_REAP_PER_TURN`, `ABORT_REAP_PER_TURN`), one frame each: bounded; the TX path under a burst is not measured (NET-OD 16 same item).
- **R-3 Slot accounting.** An aborting cap occupies its slot until its RST left or 1 s (`ABORT_DRAIN_TICKS`); `can_insert` counts it. At most one to two turns of delay for the slot; G8 proves return to baseline.
- **R-4 Legacy `TcpClose` race.** A legacy `TcpClose` on a cap that the reaper just aborted removes it before its RST left (5.4). Result: that one cap behaves as before the fix. The legacy path is documented silent (`clients/net.rs:150-151`) and is a separate decision (D-GCD-6).
- **R-5 Contract wording.** `ipc.rs:809` says the legacy close "remains an immediate abort" while the code and `clients/net.rs:150-151` say silent. Not touched here; recorded in D-GCD-6 so the follow-up decides one meaning.
- **R-6 httpd effect is small.** httpd retries with the legacy close after 1 s (`reactor.rs:18,567`), so this slice finishes the **Net half** of phase-03 step 5; httpd's reactor lifecycle stays L38's own slice.
- **R-7 Lane staleness.** The T3 lane refuses after landing (section 6, 9.2). Not a regression of any recorded result; the recorded evidence is unchanged.
- **R-8 Promoted bytes.** The promoted candidate and the canonical `kernel/src/embedded-x86_64/**` keep the old reaper until a re-promotion (same as NET-OD D-NOD-12; not requested).
- **R-9 smoltcp dependence.** Behaviour relied on: `abort()` emits one RST then clears the tuple; unknown-segment RST; `TIME-WAIT` 10 s. All read in 0.11.0 (pinned by `Cargo.lock`); G3 pins the unknown-segment rule, G2/G7 the abort rule. A smoltcp bump re-runs them.
- **R-10 Clock.** The 10 s is the HPET-based NET timebase; under TCG the relation to host wall time is [INFERENCE] (section 3.3). Irrelevant to correctness; relevant only if a later capture is compared with `10.0 s`.
- **R-11 Other architectures.** No `cfg` split; only the x86-64 host target is reasoned about [INFERENCE: aarch64/rv64 not built here].

## 13. Related findings left unchanged (each is its own decision)

1. Legacy `TcpClose` / `TcpStream::drop` silence (`handlers/tcp.rs:111-120`): the same silent-removal-then-late-RST class for an explicit hard close; ABI wording conflict (`ipc.rs:809` vs `clients/net.rs:150-151`). Changing it affects every Net client; D-GCD-6.
2. TLS handshake-in-flight sockets (`tls/dispatch.rs:173-174,190-191,298-357`; `tls_handler.rs:62,93,128-129`) drop silently: NET-OD D-NOD-6.
3. No TCP keepalive or user timeout anywhere (NET-OD 5.7 item 4).
4. httpd's reactor close lifecycle (`reactor.rs:564-575,626-628`).
5. A re-promotion of the Net cell into the canonical/matched candidate.

## 14. Lock handoff packet (form of `net-owner-death-cp1.md` section 11 / `k4b-cp1.md` section 11; DRAFT for the accountable owner, not submitted)

**14.1 Slice identity.** NET-GCD: Net service abort-at-deadline for the graceful-close ledger (state-aware reaper, ledger move into the NET-OD abort ledger, one log line), host tests, `DEV_POSTURE`. Serves the Net half of `httpd` phase-03 step 5 and the deferred item of NET-OD (D-NOD-7); no Cellbox row, not an Intel C2C outcome (the owner is asked to confirm the same bounded-slice basis as Phase 02, K2 CP2 and NET-OD). Controlling plan `.agents/261011-1200-cell-contract-authority/plan.md` (status complete; completion unlocks nothing, procedure step 6). Evidence reconciled: `net-owner-death-cp1.md`, `net-od-cp2-dossier.md`, `cp2-dossier-part4.md`, `cp2-dossier-part5.md`, the three pcap summaries of section 3.1, this document; no history erased.

**14.2 ADR-0024 contract delta (procedure step 2).**

| Principle | Effect of NET-GCD |
|---|---|
| Cell principal/incarnation distinct from Task | none; no owner or generation logic changes (`abort_owner` untouched) |
| Shared per-Cell capability space | no capability, handle or CapSet field; a cap stays Net-local and owned by one principal; a cap is in at most one ledger |
| Explicit delegation and attenuation | none; no delegation exists |
| Revocation generations | none |
| Kernel substrate vs ordinary policy Cells | no kernel file; TCP semantics stay in service-net |
| Durable logical state vs execution | none (volatile sockets) |
| Caller/test/doc migration, concurrency/remote | callers: `cellbox-ssh` and httpd unchanged (no ABI edit); tests: section 8; docs: `network-api.md`, `CHANGELOG.md`; one SocketSet owner, no new lock; no remote impact |
| No ambient credential / remote shortcut | none |

**14.3 Locked rows and overlapping plans (procedure step 3).** Read-only; `git status --short` empty at HEAD `952d84c8a`; the last commit touching `cells/services/net/**` is `d42e70d6a` (NET-OD), so no slice is in flight under the path [INFERENCE: the owner confirms against the live claims].

| Row / plan (`cell-contract-0024-locks.md`) | Blocked scope | NET-GCD relation | Needed from the owner |
|---|---|---|---|
| **L38** `261006-0934-httpd-single-cell-concurrency` (pending; "all unfinished reactor, async IPC, completion-capacity and connection-lifecycle slices") | connection-lifecycle | **Direct, same item.** Phase 03 step 5 ("abort explicitly on deadline") is this behaviour for the Net service; the same plan edits `socket_table.rs`/`service-runtime.rs` heavily (budgets, graceful drain). The httpd reactor side (legacy-close retry, `CLOSE_TIMEOUT`) stays L38's | file-bounded exception for the section-6 paths; narrow L38 step 5 to "Net-side deadline abort landed as NET-GCD", keep every whole-plan relation; the plan's phase 03 rebases onto this slice if released |
| L37 `261004-1957-x86-pc-lane`, L32 `260827-1004-hardware-independent-roadmap` | Net service authority adjacency (as recorded by NET-OD 11.3 and the S10-10 probe) | per-owner caps and attested admission unchanged | the Net file-owner named for NET-OD rules the adjacency compatible for this slice too |
| L8, L21, L23, L33, L35, L36, L9, L2 | kernel permission / owner lifetime / async IPC | **not touched**: no kernel path, no syscall, no owner-death change | note only |
| L11/L13/L18 (Tier-3 VMM), KMS plans, manifest v2, L25 | | no path of this slice lies in their scope | none |

**14.4 Retained gates (step 4).** Law 1: not triggered (section 11); explicit "no ABI delta" acknowledgement requested. Protected identity / non-rollback time/state: not applicable. Independent human decisions: D-GCD-1..10. No hardware, AWS or production gate is touched; `DEV_POSTURE` only. A green host test does not replace these gates.

**14.5 Scheduling change (step 5; text only).** In one change: record "NET-GCD" as a bounded adopted slice with ceiling "Net service reaper edit + table method + host tests + docs, `DEV_POSTURE`; no ABI, no kernel, no QEMU" in the controlling plan, `plan-portfolio.md` and `cell-contract-0024-locks.md` next to the NET-OD paragraph (`:125`), as a file-bounded exception for the section-6 paths with this packet's digest; narrow only L38 step 5; WIP stays one implementation slice; single integrating owner. The NET-OD exception is consumed (landed `d42e70d6a`) and is not extended.

**14.6 Execution boundary and rebase order once adopted.**

1. Baseline first (9.1 item 1): `git status` clean, start-of-slice overlap re-check (14.3), digest re-check of section 6, baseline counts.
2. Edits only in the section-6 paths; the pinned list re-verified; `handlers/tcp.rs`, `libs/**`, `kernel/**`, `Cargo.lock` untouched (STOP otherwise); no network, no key, no VM, no Docker.
3. **Rebase order:** NET-GCD lands as its own commit on top of `952d84c8a`; then, only if the owner wants a T3 observation, a lane CP1 that refreshes the four pins (9.2); httpd phase-03 slices rebase onto it if released; a re-promotion is separate.
4. **Rollback:** restore the section-6 pre-edit hashes (source reversal; no persistent state). Net-only: no kernel/Net version pairing exists, so no mismatch is possible.

**14.7 Decisions requested from the accountable owner.** (1) Adopt NET-GCD as a bounded slice with the section-6 file-bounded exception. (2) Name the Net file-owner and rule L37/L32 compatibility and L38 as in 14.3. (3) Acknowledge that no `libs/api`/`libs/types` byte changes (section 11). (4) Approve the one scheduling record (14.5). Nothing in this packet authorises a source edit.

## 15. Not checked

No compile, test, lane, QEMU, Docker or network action. Not measured: the current `service-net` test and clippy counts at HEAD (65 expected from the NET-OD dossier); that G2's zero-window setup behaves as described on 256-byte loopback buffers (the harness and `established` helper are read; the new scenario is not run); that G5(b) reaches `LastAck` without a poll as written; wake latency from the reaper's abort to the next poll (`sys_wait_completion(NET_RX, 10)`); the TX queue under a burst; slirp's reaction to an RST at the deadline (reasoned from the cancel flow, not observed for a FIN-WAIT-2 flow); the HPET versus wall-clock relation under TCG; the exact slirp persist schedule per run (only the T3-D2 cancel probes are recorded); what the control client's graceful close looked like in the Net cell (no instrumentation: a direct observation of the reap is impossible from existing evidence); `hypervisor-bridge` runtime behaviour; aarch64/riscv64 builds; the plans listed in NET-OD 11.3 as not examined; that the owner-side `readiness` consumers (httpd, `cellbox-ssh`) do not depend on the "unknown cap" error for a graceful cap after its deadline (read: they close and drop the cap first, section 2).

## 16. Decisions (OPEN until the user answers; recommendations in the last column)

| ID | Decision | Options | Recommendation |
|---|---|---|---|
| D-GCD-1 | Deadline semantics | **A** state-aware: free when nothing is owed, else abort + abort ledger (this text); **B** keep alive past the deadline while our FIN is unacked, then abort; **C** drain-and-discard the receive buffer so the peer can finish; **D** RST at `close()` when unread data exists; **E** leave silent and document | **A** (5.1: implements the documented contract with the NET-OD primitive; B/C/D/E reasons stated) |
| D-GCD-2 | Cause of the Tier-3 control RST | **A** record as the same defect (section 3), no separate item; **B** open a further investigation | **A** (one cause, one fix, three captures and a negative control; the falsifier is stated if anyone wants it) |
| D-GCD-3 | `TIME-WAIT` at the deadline | **A** free silently (clean end); **B** abort | **A** (nothing is owed; B would send a gratuitous RST to a peer that finished cleanly) |
| D-GCD-4 | A `Closed` socket that still owes an RST | **A** move it to the abort ledger; **B** treat `Closed` as done as today | **A** (cheap, makes the predicate exact; B can only drop a smoltcp-owed RST) |
| D-GCD-5 | Log line `[net] graceful close deadline: aborted N TCP` | **A** include; **B** no line | **A** (the only direct observation of the reap; appears only for stuck peers) |
| D-GCD-6 | Legacy `TcpClose` / `TcpStream::drop` (same silent-removal class; `ipc.rs:809` vs `clients/net.rs:150-151` conflict) | **A** separate follow-up CP1 deciding one contract; **B** include here (abort + ledger in `handlers/tcp.rs`: one more file, behaviour change for every Net client) | **A** |
| D-GCD-7 | T3 lane after landing | **A** leave it (pins stale, lane refuses; evidence stays historical); **B** a lane CP1 refreshing the four pins and taking one diag boot to observe RST - FIN about 10 s; **C** refresh pins in this slice without a run | **A** (a boot is not needed to close the defect, section 9.2; C is an unverified lane edit) |
| D-GCD-8 | Law-1 reading | **A** no `libs/api`/`libs/types` delta, owner acknowledges (section 11); **B** treat the doc-comment truth as an ABI item | **A** |
| D-GCD-9 | Test placement | **A** append to `service-runtime-owner-death-tests.rs` (371 -> about 600 lines; the standard's 200-300 LOC guideline is already exceeded by the file and by `service-runtime.rs` at 802); **B** new `service-runtime-graceful-close-tests.rs` plus `pub(super)` on the `Stack` harness | **A** (one harness, no visibility churn; B is a mechanical split if the owner prefers) |
| D-GCD-10 | Lock treatment | **A** bounded adopted slice with the section-6 exception, narrowing L38 step 5 (section 14); **B** wait and fold into the L38 plan | **A** (L38 is pending and gated on a Law-1 async prerequisite; this slice needs none) |
| D-GCD-11 | Promoted/canonical bytes | **A** unchanged, re-promotion separate; **B** include a promotion | **A** (same as D-NOD-12) |
