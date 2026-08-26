# Phase 03 — Async backends: polling (smol) then mio (tokio)

## Context Links
- Plan: [plan.md](plan.md) · Depends on: [phase-01](phase-01-compute-std.md), [phase-02](phase-02-os-std.md),
  **[phase-02.5](phase-25-readiness-protocol.md) (frozen contract: protocol + reactor recv rules +
  `AsCellHandle` ABI — GATE)**, **[phase-02.6](phase-26-net-readiness-engine.md) (readiness edges actually
  emitted — GATE; a spec alone is not enough)**
- Kernel Boundary Law: `docs/specs/15-kernel-boundary.md` (no in-kernel epoll)

## Overview
- **Priority:** P2. **Status:** pending. **Now-able:** consume the P2.5-frozen `AsCellHandle` ABI; code
  post-G3, gated on **both** P2.5 (contract/handle freeze) **and P2.6** (readiness edges actually emitted).
- Write pure-Rust OS backends at the **bottom of the async stack** so the ecosystem runs unmodified:
  **`polling` first** (unlocks `async-io`/`smol`, smaller, validates the protocol), **then `mio`**
  (unlocks tokio: features `rt`/`rt-multi-thread`/`net`/`time`/`sync`/`io-util`/`macros`; `process`+`signal` OFF).
- **Milestone M3:** a smol TCP echo, then **tokio (current-thread) + axum hello-world** in a Tier 1 cell.

## Key Insights (from research, verified)
- **Rank 1 = fork `polling`, add `src/cellos.rs`.** Smaller surface (7 methods, no `dyn`), and
  `async-io`→`smol`→`async-net` then work with **zero further OS-specific code**. Template = `poll.rs`
  (770 LOC, the generic poll(2) backend), **not** a wasi backend — **`polling` has no wasi backend**
  (`smol-rs/polling#102` open). Estimate **~400-600 LOC** (Cellos's binary mask removes the pollfd-array
  bookkeeping that inflates poll.rs).
- **`Poller` methods to implement:** `new()`, `add`/`add_with_mode` (unsafe), `modify`/`modify_with_mode`,
  `delete`, `wait(&mut Events, timeout)`, `wait_deadline`, `notify()`, `supports_level`/`supports_edge`.
- **Rank 2 = fork `mio`, add `src/sys/cellos/`.** Two-layer contract: `event::Source`
  (`register`/`reregister`/`deregister`, implemented by TcpStream/Listener) + `sys::Selector`
  (`new`/`select`/`register`/`reregister`/`deregister`) + a `Waker`. Template = the generic poll(2) Unix
  `Selector` (~650 LOC) used by Hermit/WASI. Estimate **~700-1000 LOC**. mio's `wasip1` backend is
  crippled (no waker, register-vs-poll lock conflict) — **do not copy its shape**; the wildcard-recv +
  software-demux design avoids that trap.
- **tokio feature → mio map:** only `net`/`process`/`signal` pull mio; `rt`/`rt-multi-thread`/`time`/`sync`/
  `io-util`/`macros`/`fs` are mio-free. Cellos ports the `net` `Selector` only; keep `process`+`signal` OFF.
- **Readiness, not completion** (P2.5): `wait`/`select` return token + readiness flags, never data.
  Discard the IPC payload at the backend; the socket's own `recv()` re-fetches. AFD/Windows precedent.
- **current-thread tokio, but NOT multi-thread-free (red-team M5, corrected).** The single-threaded
  runtime still needs a **blocking OS-thread pool** for `tokio::fs` and `spawn_blocking` — so **M3 depends
  on the P0 thread path being production-solid** (this corrects the original "current-thread decouples
  from multi-thread maturity" claim). Worse: a blocking `std::fs::read` inside an async task **freezes the
  single reactor thread** in `recv_timeout` → whole-cell stall, dropped connections. Bound the blocking
  pool by cell quota; add a test that a blocking call inside an async task is detected/documented (no
  silent stall). `rt-multi-thread` remains a further step gated on P0 multi-thread hardening.
- **[M6] Consume the frozen `AsCellHandle` ABI (P2.5), do NOT hardcode it.** The mio fork keys on the
  handle accessor defined + frozen in P2.5; P3 must not invent its own namespace/generation rule (P4 only
  adds ext methods on the same frozen ABI).
- **Do NOT add an in-kernel epoll** (the Redox path) — ecosystem-chasing into the kernel, rejected by
  Boundary Law + Scope Doctrine. Userspace fork is cheaper and keeps syscall surface untouched.

## Requirements
- **Functional:** `polling::Poller` over Cellos IPC readiness → `smol` TCP echo works. `mio::Poll`+`Selector`
  → tokio(net, current-thread) + axum serves a hello-world response. Both crates otherwise **unmodified**.
- **Non-functional:** backends built with `#![forbid(unsafe_code)]` where possible via ostd/cellos-abi
  wrappers (the `add`/`register` unsafe is the crate's own fd-safety contract, documented). Edge-triggered
  readiness with drain-until-`WouldBlock`.

## Architecture / data flow
```
smol:  async_net::TcpListener → async_io::Async<std::net::TcpListener> → polling::Poller (cellos.rs)
tokio: axum → hyper → tokio(net, current-thread) → mio::Poll → mio::sys::cellos::Selector
Poller/Selector.wait/select ─▶ reactor: wildcard sys_recv_timeout + sys_try_recv drain
                            ─▶ demux by (sender=net tid, byte0=NET_READY) → token readiness (P2.5)
Poller/Selector.register    ─▶ NetRequest::Register{handle, interest} → net cell
.notify()                   ─▶ P2.5 wakeup primitive (self-send or sys_wake_recv)
std::net socket handle (P2) ─▶ the "RawFd"-equivalent the backends key on (AsCellHandle)
```

## Related Code Files
- **Create (polling fork):** `src/cellos.rs` (~400-600) + `cfg_if` arm in `src/lib.rs`; expose the
  socket-handle → `Events` mapping.
- **Create (mio fork):** `src/sys/cellos/{mod.rs,selector.rs,waker.rs,tcp.rs,udp.rs}` (~700-1000) + cfg
  arms in `src/sys/mod.rs`; `event::Source` glue via the P2 socket handle.
- **Consume (frozen in P2.5, M6):** the `AsCellHandle` accessor on `TcpStream` etc. — P3 keys on it but
  does not define it; P4 formalizes the `std::os::cellos` ext methods on the same frozen ABI.
- **Create cells:** `cells/apps/smol-echo/`, `cells/apps/tokio-axum-hello/`.

## Implementation Steps
1. Consume the P2.5-frozen `AsCellHandle` ABI + Events shape (do not redefine); confirm P2.6 emits edges.
2. Fork `polling`; implement `cellos.rs` `Poller` over the P2.5 reactor; wire `notify()` (M1a wakeup).
3. `smol-echo` cell: `smol::block_on` a TCP echo through the net cell (P2.6 readiness); QEMU verify.
4. Fork `mio`; implement `Selector` + `Waker` + `event::Source` for TcpStream/Listener/UdpSocket.
5. `tokio-axum-hello` cell: current-thread runtime, `net` feature, `process`/`signal` OFF; QEMU verify a GET.
6. **(M5)** Bound the blocking pool by cell quota; add a test that a blocking `std::fs` call inside an
   async task is detected/documented (no silent reactor stall).
7. (Stretch) enable `rt-multi-thread` once P0 multi-thread TLS + `available_parallelism` are solid.

## Todo List
- [ ] consume P2.5-frozen `AsCellHandle` ABI (M6 — do not redefine)
- [ ] confirm P2.6 emits readiness edges before wiring the backend
- [ ] polling fork: `cellos.rs` Poller (add/modify/delete/wait/wait_deadline/notify)
- [ ] smol-echo cell → `SMOL-ECHO: PASS`
- [ ] mio fork: Selector + Waker + event::Source
- [ ] tokio(net, current-thread) + axum hello → `AXUM-HELLO: PASS`
- [ ] (M5) blocking pool bounded by cell quota; blocking-in-async detected (no silent stall)
- [ ] confirm process/signal features stay OFF (compile-fail if on)
- [ ] (stretch) rt-multi-thread on N threads

## Success Criteria
- QEMU x86_64: `smol-echo` accepts a TCP connection and echoes bytes; `tokio-axum-hello` serves an HTTP
  200 "hello" to a request driven through the net cell. Serial oracles: `SMOL-ECHO: PASS`, `AXUM-HELLO: PASS`.
- `polling`/`mio`/`smol`/`tokio`/`axum` are pulled from crates.io **unmodified** (only the forked
  `polling`/`mio` supply the backend, via `[patch]`).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| **[NEW — top risk] socket handle ≠ RawFd**: `polling`/`mio`/`event::Source` are fd-centric; Cellos sockets are IPC handles | H×H | Synthesize a `u32` handle namespace (P2) + `AsCellHandle`; keep the backend's "fd" a Cellos handle; this handle-model work is the real bulk of P3, not the readiness loop |
| `notify()` unavailable if P2.5 picked busy-poll | M×M | Gate on P2.5 ratified wakeup; if busy-poll, cap latency + document; prefer self-send/`sys_wake_recv` |
| mio wasip1-style register-vs-poll lock conflict | M×M | Wildcard-recv + software demux avoids it (no per-source subscription mutex); document the divergence from wasip1 |
| tokio `net` transitively needs `libc`/`socket2` types | M×M | Provide the minimal `socket2`-shaped types via std::net/os::cellos, or patch tokio's `net` glue; scope-check early |
| **[M5] current-thread tokio still needs the P0 thread path** (fs/spawn_blocking pool) | H×H | M3 depends on P0 thread path being solid (corrects the "decoupled" claim); bound pool by cell quota |
| **[M5] blocking call in async task freezes the single reactor** → cell stall, dropped conns | M×H | Route blocking to the pool; test/detect a blocking std::fs in an async task; document; no silent stall |
| **[M6] backend hardcodes a handle rule P4 later changes** | M×H | Consume the P2.5-frozen `AsCellHandle` ABI; P4 adds only ext methods; frozen gate blocks P3 start |
| rt-multi-thread hangs without robust P0 multi-thread TLS | M×H | Ship M3 on current-thread; multi-thread is stretch, gated on P0 hardening |
| Edge-triggered miss → axum connection hangs | M×H | Enforce drain-until-WouldBlock (P2.5); integration test with slow/partial reads |

## Security Considerations
- Backends run inside the cell (LBI); readiness carries no capability. Network authority is still the
  cell's net cap (kernel CapSet). A compromised net cell could send spurious readiness — bounded to
  DoS-lite by fail-loud drop of unexpected senders (P2.5).

## Next Steps
- Completes the async ecosystem story. P4 formalizes `std::os::cellos` (incl. `AsCellHandle`). P5 adds
  unwinding (some async crates assume `catch_unwind` at task boundaries — panic=abort limits until P5).
