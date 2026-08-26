# P2.5 Design — Readiness protocol + reactor recv rules + frozen `AsCellHandle` ABI

> **Status:** **RATIFIED 2026-07-23** (user approved all 4 checklist items same day).
> The handle ABI (D7) and wire contract (spec 17 §10) are now FROZEN — P3/P4 consume, never redefine.
> Companion normative text: `docs/specs/17-ipc-wire-contract.md` §10 (draft amendment, same date).
> This doc records the **decisions + rationale + evidence**; the spec records the wire contract.

## Decision summary (what got settled)

| # | Question (from phase-25) | Decision |
|---|--------------------------|----------|
| D1 | `notify()` wakeup: (a) self-send / (b) `sys_wake_recv` / (c) busy-poll | **(a′) extend `ipc_try_send`'s `pending_msgs` fallback to same-cell `0x12 REACTOR_WAKE` messages** — no new syscall; resolves plan open question 3 |
| D2 | Readiness wire format | fixed 6-byte raw frame `[0x11, events, cap_id u32 LE]`, byte-0 `0x11` claimed in spec 17 §3 |
| D3 | M1b non-collision proof | net→client byte-0 is postcard `NetResponse` variant index (≤ 0x05 today, capped ≤ 0x0F normatively) ∪ `0x11`; attacker bytes start at offset ≥ 2 — disjoint by construction |
| D4 | M1c consumer rule | **exactly one recv-loop owner per tid**; reactor owns wildcard recv on its tid, forwards `0xAC`/`0x10` to an app queue; ostd boot-time assert |
| D5 | Edge vs level | edge-triggered + drain-until-`WouldBlock`; **registration emits an immediate current-state edge** (closes the register-after-data race, mio semantics) |
| D6 | Lost-edge on `try_send` drop | net cell keeps a per-(owner,cap) **coalescing pending-edge slot** and retries next loop iteration — naturally bounded, no unbounded queue |
| D7 | M6 handle ABI | `RawCellHandle = u32` = `class:8 | provider-local id:24`, monotonic allocation, **no reuse ever** (replaces generation counters); trait quad frozen below |
| D8 | Interest registration | `NetRequest::NotifyRegister{cap_id, interest}` (variant 17) + `NotifyDeregister{cap_id}` (18) — postcard **append-only**, discriminants 0–16 untouched |

---

## D1 — Wakeup primitive: same-cell pending_msgs fallback (no new syscall)

**Evidence chain (verified this session):**
- `ipc_try_send` delivers only if target is parked in `Recv` with matching mask; otherwise the
  bounded `pending_msgs` fallback fires **only for `caller == INPUT_CELL_TID`**; every other
  sender is dropped — `kernel/src/task.rs:1304-1375`. This is the M1a lost-wakeup window.
- BUT all three recv entry points **drain `pending_msgs` before parking**, mask-honoring:
  `Recv` (`syscall.rs:1016-1020`), `RecvTimeout` (`syscall.rs:1194-1226`), `TryRecv`
  (`syscall.rs:1266-1298`).

**Therefore:** a wake that is *queued* instead of dropped survives the not-yet-parked window —
if the reactor is parked, try_send delivers and unparks it; if it is not yet parked, the message
waits in `pending_msgs` and the drain at the reactor's next recv entry returns it. No flag, no
new syscall, and the wake is never lost. **SMP caveat (review finding):** the drain and the park
take `SCHEDULER.lock` separately (`syscall.rs:1199-1228` vs `1231-1245`), so a wake landing in
that gap is seen only at the *next* recv entry — delivery may defer up to one timeout period.
**Normative consequence: the reactor MUST park with `sys_recv_timeout` (bounded tick, e.g. the
smoltcp maintenance tick), NEVER bare `sys_recv`** — under a bare recv the deferral would be a
true lost wakeup. Worst-case notify latency = one reactor timeout tick, not "immediate".

**Kernel change (P0-adjacent, ~15-20 LOC in `ipc_try_send`):** extend the fallback condition
from `caller_id == input_tid` to also accept:

```
caller.cell_id == target.cell_id            // same cell (covers sibling thread AND self)
  && msg_len <= 8 && msg[0] == 0x12         // ONLY the REACTOR_WAKE opcode
  && !target.pending_msgs.iter().any(same-cell 0x12 already queued)   // coalesce: wakes are idempotent
```

- **Narrowness is deliberate:** arbitrary same-cell IPC must NOT silently gain queueing
  semantics (that would change spec-17 §6 for every existing protocol). Only the wake opcode.
- **Coalescing** bounds the queue impact to ≤ 1 slot regardless of notify() call rate.
- **Boundary Law:** this extends an existing kernel IPC *delivery mechanism* (`pending_msgs`,
  same primitive as the input path); zero policy. Legal under the whitelist (IPC dispatch).
- Works today (single-thread runtime: caller == target task) and after P0 threads (sibling
  thread tid ≠ reactor tid, same `cell_id` — field exists: `kernel/src/task/tcb.rs:152`).

**Rejected:** (b) `sys_wake_recv` — new syscall + allowlist churn for what D1 gets from 15
lines; (c) timeout busy-poll — adds up to one tick (10 ms) latency to every cross-thread wake.

## D2/D3 — Wire format + collision proof

See spec 17 §10 draft for the normative text. Rationale for **raw fixed-frame, not postcard**:
demux must be O(1) on byte-0 without attempting a postcard decode on every message (a decode
that *fails* on a non-NetResponse message can't be distinguished from a corrupt reply —
fail-loud rule §7 needs an unambiguous classifier first). Fixed 6 bytes also satisfies §4's
explicit-boundary rule without a length field.

Collision proof (M1b), messages arriving from `net_tid` at the reactor:
1. postcard `NetResponse` replies — byte-0 = varint variant index. Today 6 variants (0x00-0x05,
   `libs/api/src/services/ipc.rs:182-189`). **Normative cap added: `NetResponse` MUST stay ≤ 16
   variants** so byte-0 ≤ 0x0F < 0x11 forever.
2. `0x11 NET_READY` — this protocol.
3. Nothing else: raw TLS ops `0x30-0x32` are client→net *requests* (§3 registry, direction
   column); normative: the net cell never emits them toward a client; reactor drops them loud.

Attacker-controlled remote bytes live only inside `NetResponse::Data(&[u8])` — postcard lays
out `[variant=0x01][len varint][payload…]`, so payload starts at offset ≥ 2 and can never be
byte-0. The demux classifier reads `(sender_tid, byte0)` only → unforgeable by payload bytes.

**Overlap caveat (review finding):** the D8 request variants 17/18 themselves postcard-encode
to byte-0 `0x11`/`0x12` — numerically identical to the two claimed frames. Safe **by direction
only** (requests are client→net; the frames are net→client / same-cell→reactor), same treatment
as the `0x04 WIRE_ASCII` registry row. Documented in spec 17 §3 + §10.2 invariant 4, with two
guard rules: the net tid can never be an interest owner, and `NetRequest` growth past variant 18
must re-check the §3 registry.

## D4 — One recv consumer per tid (M1c)

The recv endpoint is **per-task** (`TaskState::Recv` is task state; delivery targets a tid).
Post-P0-threads the correct granularity is therefore per-tid, not per-cell:

- **Rule:** at most one code path may execute `sys_recv`/`sys_recv_timeout`/`sys_try_recv` on a
  given tid. The reactor thread owns wildcard recv on its own tid.
- Non-reactor frames arriving at the reactor tid (`0xAC` APP_MSG, `0x10` input events) are
  **forwarded to an in-process app queue** — never dropped (ostd-compat path).
- `AppContext::run` / `run_with_lifecycle` (`libs/ostd/src/app.rs:162,187`) on the same tid as
  a reactor is **forbidden**: ostd gains a per-task `RECV_OWNER` claim (AtomicU8: Unclaimed /
  App / Reactor); second claimant panics at startup with a diagnostic (fail-loud, cheap).
- Blocking std ops on *other* threads keep spec-17 §2 masked recv — safe, because delivery is
  tid-targeted: the net cell replies to the requesting tid, and readiness edges go only to the
  interest owner's tid (D8). No cross-talk between a blocking sibling and the reactor.

## D5/D6 — Edge semantics + delivery reliability

- **Edge-triggered.** One `NET_READY` per false→true transition of each interest bit; the
  client MUST drain until `WouldBlock` (mio contract). Level-true-but-undrained ⇒ no new edge.
- **Registration edge:** `NotifyRegister` (and re-register) immediately emits an edge for every
  interest bit that is *currently* level-true. Without this, register-after-data hangs forever
  (the classic EPOLLET race). Matches mio's register semantics.
- **Delivery:** net cell fan-out uses `try_send` (§6 — never block on a client). A failed
  try_send does NOT lose the edge: the edge stays in the per-(owner,cap) pending slot and is
  retried each main-loop iteration until delivered or deregistered. Bounded by construction:
  ≤ 1 coalesced slot per interest, ≤ `MAX_SOCKETS` (18) interests. While any pending edge
  exists, the net loop shortens its `sys_wait_for_event` timeout to 1 tick (10 ms) so retry
  latency is bounded (idle timeout today is 10 ticks — `cells/services/net/src/main.rs:178`).

## D7 — Frozen `AsCellHandle` ABI (M6) — **P3 and P4 consume, never redefine**

```
RawCellHandle = u32          0x00000000 = INVALID
  bits 31..24  HandleClass   0x01 NET_SOCKET · 0x02 VFS_FILE · 0x03 CELL_CHILD ·
                             0x04 PIPE (reserved) · 0x00 invalid · 0xFF pseudo (reserved)
  bits 23..0   provider-local id, allocated monotonically from 1, NEVER reused
```

- **No-reuse replaces generation counters.** The net table already allocates monotonically and
  never recycles (`cells/services/net/src/socket_table.rs:29,51-53` — `next_cap += 1`,
  `remove()` doesn't touch `next_cap`). Frozen rule: every provider allocates its 24-bit local
  id monotonically and returns `Err` on exhaustion (16.7M handles per provider lifetime) —
  never wraps, never reuses. Stale-edge handling therefore reduces to: **reactor drops any
  `NET_READY` whose handle is not in its live interest map** (log-and-drop, §7).
  **Gap vs today's code (review finding):** the existing table has NO exhaustion ceiling —
  `next_cap: u64` grows unbounded and is truncated `cap as u32` at `handlers.rs:154`; past
  `0x00FF_FFFF` the id would silently corrupt the class byte, and past `2^32` the truncation
  collides with live low caps, voiding the no-reuse premise. **The ≤ `0x00FF_FFFF` ceiling +
  `Err` in `SocketTable::insert/insert_with_state` is therefore a REQUIRED P2.6 change**
  (added to design-p26 §1/§5), not an already-satisfied property.
- **Wire compatibility:** `NetRequest`/`NET_READY` carry the **provider-local id** (existing
  `cap_id: u32` fields unchanged; providers cap allocation at `0x00FF_FFFF`). Class-tagging is
  a std/reactor-layer concern; services stay ignorant of the std namespace.
- **Trait quad (final shape; lives in `std::os::cellos::io` in P4; P3's polling/mio forks key
  on the raw u32):**

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct BorrowedCellHandle<'h> { raw: u32, _marker: PhantomData<&'h OwnedCellHandle> }

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct OwnedCellHandle { raw: u32 }   // Drop → provider Close (routed by HandleClass);
                                          // for NET_SOCKET, Close implies NotifyDeregister.

pub trait AsCellHandle          { fn as_cell_handle(&self) -> BorrowedCellHandle<'_>; }
pub trait FromCellHandle: Sized { fn from_cell_handle(owned: OwnedCellHandle) -> Self; }
pub trait IntoCellHandle        { fn into_cell_handle(self) -> OwnedCellHandle; }

impl OwnedCellHandle {
    /// # Safety — IO-safety contract (mirrors FromRawFd): caller asserts sole ownership;
    /// raw must be a live provider-issued handle. Callable only inside the std fork and
    /// the polling/mio forks — Tier 1 cells forbid unsafe, which IS the firewall.
    pub unsafe fn from_raw_cell_handle(raw: u32) -> Self;
    pub fn as_raw_cell_handle(&self) -> u32;
}
```

- `mio::Token` mapping: `Token(raw as usize)` — collision-free by monotonic no-reuse.
- P4 adds only extension methods/impls (e.g. `AsCellHandle for TcpStream`); it may not alter
  the layout, class table, allocation rule, or trait signatures.

## D8 — Interest registration (wire)

Append to `NetRequest` (`libs/api/src/services/ipc.rs:111-178`) — **append-only**, existing
postcard discriminants 0–16 stay stable (**Law 1: 2× user confirm at implementation time**):

```rust
/// 17 — replace-semantics interest registration; owner = sender tid;
///      emits an immediate edge for currently-true bits (D5).
NotifyRegister { cap_id: u32, interest: u8 },
/// 18 — remove interest + drop any pending edge for this cap.
NotifyDeregister { cap_id: u32 },
```

Reply: `NetResponse::Ok` | `Err(NO_SUCH_CAP)` | `Err(NOT_OWNER)` (owner check = C5 field from
P2; until C5 lands, the registering sender is recorded as owner and enforced on every op).
`TcpClose`/`UdpClose` imply deregister. Interest bits = `READABLE 0x01 · WRITABLE 0x02` (ERROR
and HUP are always-on, unmaskable — matches epoll/mio).

## D9 — Reactor demux contract (wildcard `recv_timeout(0)` classifier)

| Order | Match on `(sender, byte0)` | Route |
|-------|---------------------------|-------|
| 1 | sender == net_tid, byte0 == `0x11` | decode 6-byte frame → fire waker for `Token(handle)`; unknown handle → log+drop |
| 2 | sender == net_tid, byte0 ≤ `0x0F` | postcard `NetResponse` → deliver to the in-flight request slot (reactor-issued requests) |
| 3 | same-cell sender, byte0 == `0x12` | wake: return from `wait()` (spurious-safe) |
| 4 | byte0 == `0xAC` or `0x10` | forward to app queue (D4) |
| 5 | anything else | **fail-loud log + drop** (§7); never decode-guess |

Request/reply exchanges issued *by the reactor thread itself* ride row 2 (its wildcard recv
subsumes the masked recv — the §2 mask rule is satisfied by the sender check + single in-flight
slot per request). Blocking siblings keep classic §2 masked recv untouched.

## Consequences for the reactor-spike (unchanged scope, now concrete)

`cells/apps/reactor-spike` validates: (1) D1 wake — notify() before parking, then park: must
return immediately (drain path); (2) 3 handles × interleaved edges demux correctly; (3)
registration edge fires for a pre-readable socket; (4) wake latency ≤ 1 tick. Oracle
`REACTOR-SPIKE: PASS`.

## Ratification checklist (user) — **all approved 2026-07-23**

- [x] D1 kernel fallback extension (same-cell `0x12`, coalesced) — accepted as P0-adjacent work item
- [x] Byte-0 claims `0x11`/`0x12` + `NetResponse` ≤ 16-variant cap — spec 17 §3/§10 ratified
- [x] D7 handle layout (`class:8 | id:24`, no-reuse-ever) — FROZEN for P3/P4
- [x] D8 `NetRequest` append (Law 1 — confirm #1 of 2× given; confirm #2 due at implementation)
