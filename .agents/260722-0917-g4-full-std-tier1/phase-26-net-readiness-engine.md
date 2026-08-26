# Phase 02.6 — Net-cell readiness engine (implements the P2.5 contract)

## Context Links
- Plan: [plan.md](plan.md) · Implements: [phase-25](phase-25-readiness-protocol.md) (ratified protocol)
- Depends on: [phase-02](phase-02-os-std.md) (owner-scoped SocketTable — C5) + P2.5 (wire format)
- Gates: [phase-03](phase-03-async-backends.md) — P3 depends on P2.6, not just on the spec
- Net cell: `cells/services/net/` (handlers.rs, main.rs, socket_table.rs)

## Overview
- **Priority:** P2 (blocks P3). **Status:** **design-draft 2026-07-23** — implementable design in
  [design-p26](design-p26-net-readiness-engine.md) (data structures, smoltcp-0.11 level predicates,
  loop integration, C5 interlock, 7-point oracle, ~700-900 LOC). Code post-G3.
- **[C3 — the missing engine]** Build the readiness-emission machinery inside the net cell. The protocol
  spec (P2.5) is necessary but **not sufficient**: today the net cell is a pure request→reply server that
  emits **no** readiness edge, so `Poller::wait`/`mio::poll`/tokio would block forever on a NET_READY that
  never arrives. This phase makes the edges actually fire.

## Key Insights (verified, red-team C3)
- **No interest/register variant exists.** The `NetRequest` match (`cells/services/net/src/handlers.rs:
  131-414`) has no Register/interest arm and never sends an unsolicited message.
- **The poll loop only replies.** `cells/services/net/src/main.rs:152-181` services `sys_try_recv`
  requests and replies; it does not fan out readiness to clients.
- Therefore P3's entire async stack cannot function until the net cell (a) tracks per-client interest,
  (b) detects per-socket edges across `smoltcp` state after each `iface.poll()`, and (c) `try_send`s
  readiness edges to the owning client under spec-17 §6 discipline. **This is unbudgeted in the original
  plan and gets its own LOC + oracle here.**

## Requirements
- **Functional:**
  - Per-client **interest table**: `Register{cap_id, interest: READABLE|WRITABLE}` / `Deregister` /
    `Reregister`, keyed by owning tid/cell_id (reuses the owner field added in P2/C5).
  - **Edge detection**: after each `iface.poll()`, diff each socket's readable/writable/closed state vs
    the last-observed state; on a level transition, enqueue a readiness edge (edge-triggered, coalesced).
  - **Fan-out**: `try_send(owner_tid, NET_READY{handle, events})` per the P2.5 wire format; drop-on-
    not-ready is safe (client re-derives via drain); never block the net cell on a client (spec-17 §6).
  - **Connect completion + close** surfaced as WRITABLE/HUP edges (feeds M2 connect-completion signal).
- **Non-functional:** readiness fan-out must not starve request/reply servicing; bounded per-client edge
  queue; no readiness to a non-owner (C5); one edge per level transition (no busy spam).

## Architecture / data flow
```
client: NetRequest::Register{cap_id, READABLE} ─▶ interest_table[cap_id] = {owner_tid, READABLE}
net-cell main loop each iteration:
   iface.poll(now) ─▶ for each socket with interest:
        state_now = {readable, writable, closed}
        if state_now != last[socket]: enqueue edge(handle, delta_events)
   drain edge queue ─▶ try_send(owner_tid, NET_READY{handle, events})   [§6: drop if not parked]
   service pending NetRequest (try_recv) ─▶ reply                        [unchanged request path]
```
- Edge detection hooks the existing `iface.poll()` site; interest + last-state live beside SocketTable.

## Related Code Files
- **Modify:** `cells/services/net/src/handlers.rs:131-414` (add Register/Deregister/Reregister arms);
  `cells/services/net/src/main.rs:152-181` (edge-detect after poll + readiness fan-out in the loop);
  `cells/services/net/src/socket_table.rs:19-30` (last-observed-state + interest fields alongside the
  C5 owner field).
- **Reference:** P2.5 wire format (byte-0 `NET_READY`, envelope), spec 17 §6 (try-send discipline).
- **Create:** extend a net smoke cell or `cells/apps/reactor-spike` to assert edges arrive.

## Implementation Steps
1. Add interest table + Register/Deregister/Reregister request arms (owner-scoped, C5).
2. Track last-observed {readable,writable,closed} per interested socket.
3. After `iface.poll()`, diff state → coalesced edges; fan out via `try_send` (§6, drop-safe).
4. Surface connect-completion + close as WRITABLE/HUP edges (M2).
5. QEMU: a client registers interest, drives traffic, receives correct interleaved edges; assert no edge
   to a non-owner; assert request/reply latency does not regress.

## Todo List
- [ ] Register/Deregister/Reregister arms (owner-scoped)
- [ ] per-socket last-state tracking
- [ ] edge detection after iface.poll() + coalescing
- [ ] try_send readiness fan-out (spec-17 §6, drop-safe)
- [ ] connect-completion + HUP edges (M2)
- [ ] QEMU: `NET-READINESS: PASS` (edges arrive, owner-only, no request regression)

## Success Criteria
- QEMU x86_64: a test client registers READABLE interest on a socket, a peer sends bytes, the client
  receives a `NET_READY{handle, READABLE}` edge and drains successfully; WRITABLE fires on connect
  completion; HUP fires on peer close. No edge reaches a non-owning cell. Oracle: `NET-READINESS: PASS`.
- P3's `smol-echo` can be driven end-to-end against this engine (integration checkpoint).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Readiness fan-out starves request servicing | M×M | Bounded edge queue drained per loop iteration; fairness cap; measure request latency in the oracle |
| Edge coalescing drops a needed transition → client hang | M×H | Level-diff after every poll; re-arm on state change; drain-until-WouldBlock on client side (P2.5) |
| Readiness sent to a non-owner (C5 regression) | M×H | Interest keyed by owner tid/cell_id; assert owner on every fan-out; covered by oracle |
| `try_send` drop loses a wakeup permanently | M×H | Client re-derives readiness by draining on its next wait; net cell re-emits on next transition; test the drop race |
| smoltcp state model doesn't expose a clean writable edge | M×M | Derive writable from send-buffer space; validate against smoltcp 0.11 socket API |

## Security Considerations
- Readiness carries no capability; owner-scoping (C5) prevents a cell from learning another cell's socket
  activity. Unexpected sender/interest for a non-owned cap_id → fail-loud Err (spec 17 §7).
- Bounded edge queues prevent a client that never drains from exhausting net-cell memory.

## Next Steps
- With P2 (owner-scoped table + io trichotomy + DNS) and P2.5 (contract + handle freeze), P2.6 makes
  readiness real → unblocks P3.
