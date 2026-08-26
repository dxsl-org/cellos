# Phase 02 — OS std: std::fs over VFS, std::net over net cell

## Context Links
- Plan: [plan.md](plan.md) · Depends on: [phase-01](phase-01-compute-std.md)
- IPC wire contract: `docs/specs/17-ipc-wire-contract.md` (masked recv, 480B cap, fail-loud)
- VFS: `docs/specs/09-vfs.md` · ostd `fs.rs`, `ipc.rs`, `grant.rs` (verified this session)

## Overview
- **Priority:** P2. **Status:** pending. **Now-able:** design now; code post-G3.
- Promote fs + net from `Unsupported` to real implementations backed by the VFS cell and net cell.
- **Red-team gates folded in:** owner-scope the net SocketTable (**C5** — must land before P3/P4 freeze the
  handle model); give the net wire protocol a real `(WouldBlock, EOF, Error, ConnRefused)` trichotomy +
  connect-completion (**M2**); resolve or explicitly restrict DNS (**M3**); canonicalize VFS paths (**M7**).
- **Milestone M2:** `std::fs` round-trips a file via VFS; `std::net::TcpStream` does an HTTP GET via net
  cell **to a numeric IP** (hostname GET gated on the M3 DNS decision).

## Key Insights (from research, verified)
- Hermit `sys/fs/hermit.rs` is the **largest PAL file (615 LOC)** and cannot be ported line-by-line: it
  assumes a local POSIX syscall table. Cellos rewrites it against the **VFS IPC/grant protocol**.
- ostd already has the primitives to model on: cap-based `open_cap(13)/read_cap(14)/write_cap(229)/
  close_cap(15)/seek_cap(228)/stat_cap(230)/truncate_cap(231)/sync_cap(232)` and the read-via-grant path
  (`fs.rs:read_full_via_grant`, `read_all` chooses <4KB=ReadCap vs ≥4KB=grant).
- **No `VfsRequest::Rename` exists** (verified). `fs::rename` = **copy + delete**, documented as
  non-atomic; a real rename needs a new VFS op → `libs/api` (Law 1). Default: copy+delete, **no ABI change**.
- IPC chunking: small writes cap at **400B** per `VfsRequest::Append` (`fs.rs:CHUNK_CONTENT=400`); large
  reads/writes use the grant path. Replies fit **≤480B** after postcard envelope (spec 17 §5).
- Net = smoltcp + typed IPC to the net cell (not BSD sockets). `sys/net/cellos.rs` is **bespoke** — no
  reuse of the shared `socket.rs`. Existing socket ops live in the net service; legacy raw TLS ops on
  byte-0 `0x30-0x32` (spec 17 §3) and typed `NetRequest`.
- No stdin today (`console.rs` = `sys_log` only). std stdout/stderr → `sys_log` (from P1); stdin stays
  `Unsupported` unless an IPC console-input path is added (defer).
- **[C5 — cross-cell socket hijack, verified.]** `SocketTable` (`cells/services/net/src/socket_table.rs:
  19-30`) has **no owner field**; `TcpSend/Recv/Close{cap_id}` (`handlers.rs:157-208`) never check the
  sender owns `cap_id`; cap_ids are a **dense guessable counter (1..18)**. Cell B `TcpRecv{cap_id:5}`
  reads cell A's inbound bytes; TcpSend injects; TcpClose tears down. `FromCellHandle` (P4) makes forgery
  ergonomic. **Fix: SocketTable records owning sender tid/cell_id; every cap_id op returns Err when
  sender≠owner. The net cell (not the std wrapper) is the authorization point.** Gate before P3/P4.
- **[M2 — std::io trichotomy is inexpressible today, verified.]** `TcpRecv` returns `R::Data(&[])` for
  would-block but `R::Err(0xFF)` for **both EOF and generic failure** — same 0xFF as make_tcp/connect
  errors (`handlers.rs:135,149,188-196`). `TcpConnect` returns `R::CapId` **before the handshake
  completes** (`handlers.rs:153-154`; lazy `try_promote`). So `std Read` can't tell `Ok(0)` EOF from
  error → `read_to_end`/hyper/reqwest hang or treat error as clean EOF; `connect` returns a not-yet-
  connected stream with no refused/timeout signal. **Fix: distinct discriminants for
  WouldBlock/Eof/ConnRefused/ConnReset + a connect-completion signal** (blocking poll of SocketState
  with timeout, or fold into P2.5 readiness).
- **[M3 — DNS is a hard stub, verified.]** `NetRequest::Resolve => R::Err(0xFF)` "DNS resolver not yet
  implemented" (`handlers.rs:404-405`). Every **hostname** connect (what hyper/reqwest use) fails at
  `ToSocketAddrs`; only numeric IP works. The plan's "Resolve exists" reuse claim is **wrong**.
- **[M7 — VFS gate is prefix-only, no canonicalization, verified.]** `access.rs:80-100` prefix-matches
  without resolving `./..` and ignores CellId; `open("/data/../bin/evil")` matches the `/data/` rule — if
  the FAT/mount layer collapses `..`, the write lands in `/bin` (the spawn security boundary). Separately
  `allow_read_all:true` for all prefixes (`access.rs:34-67`) = **global read** (any cell reads any path).

## Requirements
- **Functional:** `File::{open,create,read,write,seek,metadata,set_len,sync_all}`, `read_dir`, `remove_file`,
  `rename` (copy+delete), `create_dir`; `TcpStream::{connect,read,write}`, `TcpListener::{bind,accept}`,
  `UdpSocket`, `ToSocketAddrs`/DNS via net cell. Blocking semantics (async is P3).
- **Non-functional:** every request/reply is **masked to the service tid** (spec 17 §2); driver replies
  via `try_send`+client `recv_timeout` (§6); no silent-empty / silent-drop (§7); chunks leave envelope headroom.
- **[C5]** Net SocketTable is **owner-scoped**: every `cap_id`-bearing op returns `Err` when
  `sender ≠ owner`; the net cell is the sole authorization point.
- **[M2]** Net responses distinguish `WouldBlock` / `Eof` / `ConnRefused` / `ConnReset` / generic `Err`,
  and expose a **connect-completion** signal; `std::net` maps them to correct `io::Result`/`ErrorKind`.
- **[M3]** Either a DNS client (UDP/53 + cache) in net-cell scope, **or** `ToSocketAddrs(hostname)` =
  `Unsupported` with M2/M3 restricted to numeric-IP endpoints (explicit, documented — chosen at step 4).
- **[M7]** VFS paths are **canonicalized** (resolve `./..`, reject escapes) **before** the prefix check.

## Architecture / data flow
```
File::open(path)  ──▶ sys/fs/cellos.rs ──▶ open_cap(13) → cap_id ; VFS tid cached
File::read(buf)   ──▶ read_all: <4KB ReadCap(14) | ≥4KB GrantAlloc(208)+share+BlkReadAsync(212)
File::write(buf)  ──▶ write_all: small → Append (≤400B chunks) | large → grant path (229)
fs::rename(a,b)   ──▶ copy(a→b) + remove(a)   [NON-ATOMIC — documented]
TcpStream::connect──▶ sys/net/cellos.rs ──▶ NetRequest::Connect → net cell → socket handle
   read/write     ──▶ NetRequest::{Recv,Send} (masked recv to net tid, recv_timeout)
ToSocketAddrs     ──▶ NetRequest::Resolve (DNS via net cell)
```
- Socket handle = a Cellos `u32` index returned by the net cell (NOT a POSIX fd). This handle is the
  seed of the P3 async "fd-like" abstraction — design its shape here (see P2.5/P3).

## Related Code Files
- **Create (std fork):** `sys/fs/cellos.rs` (~500-700), `sys/net/cellos.rs` (~500-700); add cfg arms to
  `sys/fs/mod.rs` + `sys/net/connection/mod.rs`.
- **Reference (no change):** `libs/ostd/src/fs.rs`, `ipc.rs`, `grant.rs`; net cell `NetRequest` protocol.
- **Modify (Law 1, only if atomic rename chosen):** `libs/api` `VfsRequest::Rename` → **2× confirm**.
- **Modify (C5):** `cells/services/net/src/socket_table.rs:19-30` (owner tid/cell_id field);
  `cells/services/net/src/handlers.rs:157-208` (owner check on every cap_id op).
- **Modify (M2):** `cells/services/net/src/handlers.rs:135,149,153-154,188-196` (distinct WouldBlock/Eof/
  ConnRefused/ConnReset discriminants + connect-completion); `sys/net/cellos.rs` maps them.
- **Modify (M3, if resolver):** add a DNS client to `cells/services/net/` (UDP/53 + cache) + a Resolve arm.
- **Modify (M7):** `cells/services/vfs/src/access.rs:80-100` (canonicalize before prefix check).
- **Create:** extend `cells/apps/std-smoke/` (or `std-io-smoke`) exercising fs + a TcpStream GET.

## Implementation Steps
1. Map `std::fs::File` → cap-based ostd ops; implement `FileAttr`/`Metadata` from `stat_cap`.
2. Implement `read_dir`/`DirEntry` over the legacy FD readdir path (or a VFS list op).
3. Implement `fs::rename` as copy+delete; document non-atomicity in the fn + spec.
4. **(C5)** Add owner tid/cell_id to SocketTable; enforce sender==owner on every cap_id op.
5. **(M2)** Add distinct net response discriminants + connect-completion; map them in `sys/net/cellos.rs`.
6. **(M3)** Decide: DNS client in net-cell (UDP/53+cache) OR `ToSocketAddrs(hostname)`=Unsupported +
   numeric-IP-only M2/M3. Implement the chosen path; document the restriction if deferred.
7. **(M7)** Canonicalize VFS paths before the prefix check; **[verify]** whether the FAT/mount layer
   collapses `..` (confirms whether the escape is live).
8. Implement `sys/net/cellos.rs`: TcpStream/Listener/UdpSocket over `NetRequest`; masked recv.
9. Blocking read/write use `recv_timeout` + retry (driver replies are `try_send`, §6).
10. QEMU: write+read a file; TcpStream GET to a numeric IP → assert bytes; assert a non-owner cap_id op fails.

## Todo List
- [ ] sys/fs/cellos.rs (File/OpenOptions/Metadata/ReadDir)
- [ ] fs::rename = copy+delete (documented non-atomic)
- [ ] (C5) SocketTable owner-scoped; sender≠owner → Err (gate before P3/P4)
- [ ] (M2) net WouldBlock/Eof/ConnRefused/ConnReset discriminants + connect-completion
- [ ] (M3) DNS resolver OR documented numeric-IP-only restriction
- [ ] (M7) VFS path canonicalization before prefix check + [verify] FAT `..` collapse
- [ ] sys/net/cellos.rs (TcpStream/Listener/UdpSocket)
- [ ] masked-recv + recv_timeout discipline (spec 17 §2/§6)
- [ ] std-io-smoke cell: file round-trip + numeric-IP TCP GET
- [ ] QEMU: `STD-FS: PASS` / `STD-NET: PASS` / non-owner cap_id op rejected

## Success Criteria
- QEMU x86_64: cell writes a file, reads it back byte-identical via `std::fs`; opens a `TcpStream` to a
  **numeric IP**, sends an HTTP GET through the net cell, reads a response and correctly distinguishes
  EOF from error (M2). Serial oracle: `STD-FS: PASS`, `STD-NET: PASS`.
- **(C5)** A second cell issuing a `TcpRecv/Send/Close` on a cap_id it does not own gets `Err`, not data.
- **(M7)** `open("/data/../bin/x")` is rejected by canonicalization (or `[verify]` proves FAT doesn't collapse `..`).
- Spec 17 compliance checklist satisfied for every new IPC path (masked recv, length/postcard, fail-loud).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Wildcard-recv poisoning (spec 17 §8.2 recurrence) — std net/fs recv eats a queued event | M×H | Mask every request/reply to the service tid; reuse `ostd::ipc::service_call`; assert sender tid |
| `fs::rename` non-atomic surprises crates expecting atomic replace | M×M | Document loudly; provide `std::os::cellos` atomic-rename only if `VfsRequest::Rename` lands (P4/Law-1) |
| 400B chunk / 480B reply caps cause partial writes if not looped | M×M | Loop chunks; large path via grant; unit test a >4KB write and a >480B read |
| Blocking net read starves on dropped driver reply | M×M | `recv_timeout` + bounded retry; treat drop as timeout (§6), never block on maybe-gone peer |
| Socket-handle model chosen here constrains P3 async backend | M×H | Co-design the handle namespace with P2.5/P3 (fd-like `u32`, `AsCellHandle`); don't finalize unilaterally |
| **[C5] Cross-cell socket hijack** (guessable cap_id, no owner check) | H×H | Owner tid/cell_id on SocketTable; sender==owner on every op; net cell is authorization point; land before P3/P4 |
| **[M2] std Read can't tell EOF from error** → hyper/reqwest hang or false clean-EOF | H×H | Distinct WouldBlock/Eof/ConnRefused/ConnReset discriminants + connect-completion; map to ErrorKind |
| **[M3] hostname connect fails** (Resolve is a stub) | H×M | DNS client in net-cell, OR document numeric-IP-only + `ToSocketAddrs(hostname)=Unsupported`; hyper/reqwest hostname use gated on this |
| **[M7] Path-traversal into /bin** (prefix-only gate, no canonicalization) | M×H | Canonicalize before prefix check; `[verify]` FAT `..` behavior; reject escapes |
| **[M7 — DEFER, accepted risk] `allow_read_all:true` = global read** (any cell reads any path) | M×M | **Pre-existing posture, NOT introduced by G4; user has not authorized reversing it.** Documented as accepted risk; a future per-CellId read ACL would close it. Plan's "manifest-scoped VFS caps" wording is reconciled: **read authority is currently global**; only write authority is prefix-gated |

## Security Considerations
- fs/net widen the cell's authority — the cell manifest must carry the matching caps (VFS access, network
  cap). std cannot grant authority the manifest lacks (CapSet enforced in kernel, spec 16 §3.1).
- **Reconcile "manifest-scoped VFS caps" with reality (M7):** today VFS **write** authority is prefix-
  gated but **read** authority is **global** (`allow_read_all:true`, `access.rs:34-67`). G4 does not
  change this (DEFER — not authorized); a std cell can read any path. Document loudly; a per-CellId read
  ACL is the future closer. Do NOT claim per-cell read isolation the code doesn't provide.
- **(C5)** Socket ownership is a confidentiality + integrity control — without it any cell reads/injects/
  closes another cell's TCP stream. The net cell authorizes; the std handle is an opaque per-cell token
  (see P2.5 `AsCellHandle` freeze), never an ambient capability.
- Path handling: VFS is `/bin` RO in places; `File::create` under RO paths must surface a typed error, not
  silent success. Canonicalize before the gate check (M7).

## Next Steps
- Feeds **P2.6** (net readiness engine builds on the owner-scoped SocketTable + M2 discriminants), P3
  (async net), and P4 (os::cellos CellStream). P2.5 protocol spec runs in parallel (no code dep).
