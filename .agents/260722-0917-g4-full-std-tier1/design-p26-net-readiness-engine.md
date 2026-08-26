# P2.6 Design — Net-cell readiness engine (implements the P2.5 contract)

> **Status:** design **RATIFIED 2026-07-23**; code post-G3. Implements spec 17 §10 (ratified)
> per [design-p25](design-p25-readiness-protocol-handle-abi.md). Depends on P2's C5 owner field.
> Target: `cells/services/net/src/` — smoltcp **0.11** (Cargo.toml:40), MAX_SOCKETS = 18.

## 1. Data structures (new module `readiness.rs`, beside `socket_table.rs`)

```rust
/// One row per NotifyRegister. Keyed by cap_id (BTreeMap<u32, Interest>).
struct Interest {
    owner_tid: usize,     // tid that sent NotifyRegister (== C5 owner; enforced)
    mask: u8,             // READABLE|WRITABLE (ERROR/HUP always-on)
    last: LevelState,     // last-observed levels — the diff baseline
    pending: u8,          // coalesced undelivered edge bits (0 = none) — the D6 retry slot
}

#[derive(Default, Clone, Copy, PartialEq)]
struct LevelState { readable: bool, writable: bool, error: bool, hup: bool }
```

- `pending` doubles as the bounded retry queue: ≤ 1 slot per interest, ≤ 18 interests total —
  no allocation on the hot path, no unbounded growth (spec 17 §10.4).
- **`SocketTable` change (D7 prerequisite, review finding):** `insert`/`insert_with_state` gain
  the frozen handle ceiling — allocate `next_cap` monotonically but return `Err` once it would
  exceed `0x00FF_FFFF` (24-bit id space; the `cap as u32` truncation at `handlers.rs:154`
  otherwise corrupts the class byte / eventually collides with live caps).
- Lives beside `SocketTable`, NOT inside it — the table maps cap→handle/state (protocol
  correctness); readiness is an orthogonal concern with its own lifecycle (register/deregister
  vs create/close). `remove(cap)` in the table triggers `readiness.drop_cap(cap)`.

## 2. Level predicates (smoltcp 0.11 mapping)

Computed per interested cap after every `iface.poll()`:

| Level | TCP (`tcp::Socket`) | UDP (`udp::Socket`) |
|---|---|---|
| `readable` | `sock.can_recv()` **or** (`!sock.may_recv()` && was Established — remote FIN: recv now returns EOF, which counts as readable) | `sock.can_recv()` |
| `writable` | `sock.can_send()` (implies Established w/ tx space) | `sock.can_send()` |
| `error` | state collapsed to `Closed` without local close (RST/abort) — detect via `SocketTable` state ∈ {Connecting, Connected} && smoltcp `state() == Closed` | n/a (false) |
| `hup` | `!sock.may_send() && !sock.may_recv()` or state `Closed`/`TimeWait` after peer close | n/a (false) |

- **Connect completion (M2 feed):** `Connecting → can_send()` flips `writable` false→true —
  the WRITABLE edge IS the connect-completion signal (mirrors epoll). No new wire needed.
- **EOF vs error (M2):** HUP/ERROR bits let the client distinguish peer-close from abort at
  the readiness layer even before P2's io-trichotomy fixes the `0xFF` reply conflation
  (`handlers.rs:135-405` today returns `Err(0xFF)` for everything).
- UDP caps: guarded by `table.is_udp(cap)` (socket_table.rs:124-131) before the TCP downcast
  — `sockets.get::<tcp::Socket>()` on a UDP handle panics.

## 3. Main-loop integration (`main.rs:152-181`)

```
loop iteration (existing structure, additions marked +):
    iface.poll(now, &mut device, &mut sockets)          // unchanged site, main.rs:154
 +  readiness.sweep(&sockets, &table):                  // §2 predicates
 +      for each (cap, interest): now = levels(cap)
 +          rising = (!interest.last.bit && now.bit) per bit
 +          interest.pending |= (rising & (interest.mask | ERROR | HUP))
 +          interest.last = now
 +  readiness.fan_out():                                // D6 delivery
 +      for each interest with pending != 0:
 +          if sys_try_send(owner_tid, [0x11, pending, cap_le]) == Ok { pending = 0 }
 +          // else keep pending — retried next iteration (never lost, never blocking)
    sys_try_recv(0, &buf) → handle_request(..)          // unchanged request path
 +      NotifyRegister  → insert Interest{owner=sender}; seed last = levels(cap);
 +                        pending = currently-true bits & (mask | ERROR | HUP)
 +                        // registration edge, §10.3 — ERROR/HUP are unmaskable, so they MUST
 +                        // be in the seed too: a socket already RST-dead at registration
 +                        // (readable=false) would otherwise never rise → silent hang (review)
 +      NotifyDeregister / TcpClose → drop Interest + pending
    idle: sys_wait_for_event(NET_RX, timeout)
 +      timeout = if readiness.any_pending() { 1 tick } else { 10 ticks }   // bounded retry latency
```

- **Ordering: sweep → fan_out → serve requests.** A request served in this iteration that
  changes state (e.g. TcpSend fills the tx buffer) is observed by the *next* iteration's sweep
  — acceptable: edges are level-transition-driven, and the client that just made the request
  doesn't need an edge to know its own action.
- **Registration edge seeding:** `pending` starts as the currently-true bits (not `last = false`)
  — this is exactly the §10.3 register-after-data race fix; `last` still seeds to the true
  levels so the next sweep doesn't double-fire.
- **Latency paths (all inbound-driven transitions wake the loop via NET_RX):** data arrival,
  SYN-ACK (connect→WRITABLE), FIN/RST (HUP/ERROR), ACKs freeing tx space (WRITABLE) — all
  originate from an RX frame → `sys_wait_for_event(NET_RX, ..)` returns → poll → sweep →
  fan_out in the same iteration. Timer-driven transitions (retransmit exhaustion → error) ride
  the ≤ 100 ms idle tick. Undelivered-edge retry rides the 1-tick (10 ms) degraded timeout.

## 4. Owner scoping (C5 interlock)

- `NotifyRegister` records `owner_tid = sender` and — once P2's C5 owner field exists on
  `SocketTable` — must verify `sender == table.owner(cap)` before accepting; mismatch →
  `NetResponse::Err(NOT_OWNER)` (fail-loud, spec 17 §7).
- Every fan-out asserts the destination == the recorded owner; there is no code path that
  sends an edge derived from cap X to any tid other than `interest[X].owner_tid`, so no cell
  can observe another cell's socket activity (C5 regression guard is structural, plus oracle).
- If P2.6 is ever scheduled before C5 lands (not planned), the sender-recorded owner is the
  interim gate — noted so the dependency is explicit, not silent.

## 5. What this phase does NOT do

- No wire-format changes beyond the two appended `NetRequest` variants (P2.5 D8 — Law 1
  2× confirm happens there). The `SocketTable` ceiling (§1) is a table-internal change, not wire.
- No reply-path rework (`Err(0xFF)` trichotomy, DNS, premature-connect) — that is P2 (M2/M3).
  **Known hazard carried until then (review finding):** ALL NetResponse replies — including the
  new NotifyRegister/Deregister acks — go through `send_typed` → blocking `sys_send`
  (`handlers.rs:46-51`), so a client that requests and then doesn't recv parks the whole net
  cell in `Sending{client}` (spec 17 §6/§8.1 class). Interim rule: a reactor MUST consume the
  register/deregister ack synchronously (masked recv or its wildcard loop) before doing other
  blocking work; the proper fix (reply path → try_send + client recv_timeout) lands with P2.
- No kernel changes — the `0x12` same-cell fallback is P0-adjacent work owned by P2.5 D1;
  the net cell only ever try_sends `0x11` frames cross-cell (drop-safe + retry slot).

## 6. Test plan / oracle (QEMU x86_64, extends the net smoke)

`NET-READINESS: PASS` requires all of:
1. Register READABLE on an established socket, peer sends bytes → exactly one `0x11` edge with
   READABLE; client drains; more bytes → new edge (drain re-arms).
2. Two bursts arriving between client parks → ONE coalesced edge (no spam).
3. `TcpConnect` + register WRITABLE while `Connecting` → WRITABLE edge fires on SYN-ACK
   (connect completion); peer close → HUP edge; RST → ERROR edge.
4. Registration edge: peer sends bytes FIRST, then client registers → immediate READABLE edge.
5. Owner isolation: second cell attempts `NotifyRegister`/receives-edge on foreign cap →
   `Err(NOT_OWNER)`, zero edges observed (C5 oracle).
6. Retry: client stays un-parked through one edge, parks later → edge arrives on the retry
   (deferred, not lost).
7. Regression: request/reply latency of the existing smoke unchanged (fan-out ≤ 18 try_sends
   per iteration is O(1)-ish; measure before/after).

## 7. LOC estimate vs plan

`readiness.rs` (~250-350) + handler arms (~80) + main-loop hooks (~40) + smoke-cell test
driver (~300-400) ≈ **700-900 LOC** — inside the plan's 800-1200 envelope (the spike
`reactor-spike` from P2.5 covers the client side).
