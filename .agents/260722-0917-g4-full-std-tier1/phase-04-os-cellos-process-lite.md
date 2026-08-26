# Phase 04 — std::os::cellos extension traits + process-lite

## Context Links
- Plan: [plan.md](plan.md) · Depends on: [phase-01](phase-01-compute-std.md), [phase-02](phase-02-os-std.md),
  [phase-02.5](phase-25-readiness-protocol.md) (**frozen `AsCellHandle` ABI — implement, do not redefine**)
- App tiers: `docs/specs/05-application.md` (no fork/exec) · IPC: `docs/specs/17-ipc-wire-contract.md`
- ostd `cap.rs`, `runtime.rs` (app_entry! cap flags), `grant.rs` (verified this session)

## Overview
- **Priority:** P3. **Status:** pending. **Now-able:** trait design now; code post-G3.
- Provide the **Cellos-native extension surface** that replaces the deliberately-absent `std::os::unix`,
  and a **process-lite** `Command` built on cell spawn (no fork), with IPC/Grant-relayed pipes.
- **Milestone M4:** `Command::new("/bin/foo").spawn()` launches a child cell + an IPC pipe; a POSIX-only
  crate (uses `std::os::unix`/`pre_exec`) **fails at compile** (firewall verified).

## Key Insights
- `std::os::unix` is **deliberately absent** (locked decision 4). POSIX-shaped crates fail to compile →
  routed to Tier 3 (Scope Doctrine firewall enforced by rustc). Do NOT add a unix shim.
- Analog to `std::os::wasi` / `std::os::windows`: a `std::os::cellos` module with extension traits keyed
  on Cellos concepts — **capabilities**, **cell handles**, **typed IPC**, **grants**.
- `Command` maps to a **posix_spawn-like** model on `sys_spawn_from_elf(238)` / `sys_spawn_from_path(12)`
  — NOT fork. Capabilities replace uid/gid/`pre_exec`: `SpawnExt::with_caps(CapSet)`.
- Pipes have no kernel primitive; emulate via **IPC message relay** (small) or a **Grant ring** (large),
  reusing ostd `grant.rs` + the spec 17 framing.
- **[M6, corrected]** The `AsCellHandle` trait + handle namespace/lifetime/reuse rule is **frozen in
  P2.5**, not defined here. P4 **implements the `std::os::cellos::io` module against that frozen ABI and
  adds only extension methods** — it must NOT redefine the namespace or generation rule (P3's mio fork
  already keys on it). Adding a new rule here would force a mio-fork rework.

## Requirements
- **Functional:**
  - `std::os::cellos::io::{AsCellHandle, FromCellHandle, IntoCellHandle}` (fd analog) — **implementing the
    P2.5-frozen ABI**, not a new definition. `FromCellHandle` must respect the owner-scoping (C5): a
    forged/non-owned handle yields an unusable stream (the net cell rejects ops), never another cell's socket.
  - `std::os::cellos::process::CommandExt::with_caps(CapSet)` (capability-based spawn; replaces uid/pre_exec).
  - `std::os::cellos::net::CellStreamExt` — a unix-socket analogue over typed IPC (`CellStream` per spec 17).
  - `std::process::{Command, Child, Stdio}` spawn a child **cell** (ELF by path), with `stdin/stdout/stderr`
    piped over IPC/Grant relay; `Child::wait` via `NotifyOnExit(204)`.
- **Non-functional:** `#![forbid(unsafe_code)]` holds for cells; extension traits are additive to std;
  compile-time firewall: no `std::os::unix` symbols exist for `target_os="cellos"`.

## Architecture / data flow
```
Command::new("/bin/foo").with_caps(caps).stdout(Stdio::piped()).spawn()
   ──▶ set spawn-args stash ──▶ sys_spawn_from_path(12) / sys_spawn_from_elf(238)  (cap ∩ spawner)
   ──▶ create IPC/Grant pipe endpoints; hand child its stdio handles via spawn args
Child::wait() ──▶ sys_notify_on_exit(204) ──▶ recv exit notification ──▶ ExitStatus
pipe write    ──▶ small: sys_send framed (spec 17) | large: Grant ring (grant.rs)
CellStream    ──▶ typed IPC connect/send/recv (unix-socket analogue, byte0-registered protocol)
```
- Spawn capability rule (verified): spawned cap = requested ∩ spawner (loader.rs; init=Root exempt).
  `with_caps` cannot exceed the parent's CapSet — kernel-enforced (spec 16 §3.1).

## Related Code Files
- **Create (std fork):** `library/std/src/os/cellos/{mod.rs,io.rs,process.rs,net.rs}`; wire
  `sys/process/cellos.rs` (real `Command`/`Child`/`Stdio` — replaces the P1 `Unsupported` fall-through)
  + cfg arm in `sys/process/mod.rs`; `sys/pipe/cellos.rs` (IPC/Grant relay) + cfg arm.
- **Reference:** `libs/ostd/src/cap.rs` (CapHandle), `runtime.rs` (cap flags), `grant.rs`, `service.rs`.
- **Possible (Law 1):** if `CellStream` needs a new well-known service id or `libs/api` type → **2× confirm**.
- **Create cells:** `cells/apps/std-process-smoke/` (parent spawns child + pipe), a POSIX-only crate
  compile-fail fixture.

## Implementation Steps
1. **(Now)** Design the `std::os::cellos` trait surface (io/process/net) + `CellStream` protocol sketch —
   consuming the P2.5-frozen `AsCellHandle` ABI (io traits are implemented, not redefined).
2. Implement `AsCellHandle`/`FromCellHandle`/`IntoCellHandle` against the **frozen** namespace; enforce
   C5 owner-scoping so `FromCellHandle` can't forge access to another cell's socket.
3. Implement `sys/process/cellos.rs`: `Command` → spawn-from-path/elf; `Child` + `wait` via NotifyOnExit;
   `Stdio::piped()` → IPC/Grant pipe endpoints.
4. Implement `sys/pipe/cellos.rs`: small=IPC framed, large=Grant ring; `Read`/`Write`.
5. Implement `CommandExt::with_caps` (cap ∩ spawner enforced by kernel).
6. Implement `CellStream` (unix-socket analogue) over typed IPC.
7. QEMU: parent spawns child cell, pipes a message through stdout, `wait()`s for exit; assert output.
8. Add compile-fail fixture: a crate using `std::os::unix::process::CommandExt::pre_exec` fails to build.

## Todo List
- [ ] std::os::cellos::io (AsCellHandle family) — implement P2.5-frozen ABI, C5 owner-scoped; no redefine
- [ ] std::os::cellos::process::CommandExt::with_caps
- [ ] std::os::cellos::net::CellStream (+ protocol, byte0 registered)
- [ ] sys/process/cellos.rs (Command/Child/Stdio) + wait via NotifyOnExit
- [ ] sys/pipe/cellos.rs (IPC small / Grant large)
- [ ] std-process-smoke cell → `STD-PROCESS: PASS`
- [ ] POSIX-only crate compile-fail fixture → firewall verified

## Success Criteria
- QEMU x86_64: parent cell `Command::new("/bin/child").stdout(piped()).spawn()`, reads child's piped
  output, `wait()` returns the child's exit code. Serial oracle: `STD-PROCESS: PASS`.
- A fixture crate importing `std::os::unix` fails `cargo build` for `target_os="cellos"` (expected;
  proves the doctrine firewall). Serial/CI oracle: build error asserted, not a runtime panic.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Crates expect `std::os::unix` transitively (e.g. `nix`, `libc` users) → hard compile break | M×M | This is **intended** (firewall); document the Tier-3 escape hatch; provide `std::os::cellos` equivalents for the common needs (spawn, handle) |
| `Command` spawn model diverges from `Command` semantics (env, cwd, args) enough to surprise callers | M×M | Map args via spawn-args stash; env via P1 in-mem map; cwd = Unsupported (documented); keep surprises typed |
| Pipe relay backpressure/deadlock (child blocks writing, parent not reading) | M×H | Grant ring with bounded capacity + non-blocking try_send + drain discipline (spec 17 §6); integration test the deadlock case |
| `with_caps` requesting caps the parent lacks silently drops them | M×M | Kernel enforces cap ∩ spawner; surface the effective CapSet; test a request-exceeds-parent case |
| No MMIO/cap release until child death (verified footgun) | L×M | Document; children are short-lived or explicitly killed (sys_kill_cell) |

## Security Considerations
- `with_caps` is capability-based least-privilege — strictly better than uid/`pre_exec`. Kernel caps the
  child at parent ∩ requested; a cell cannot escalate a child (spec 16 §3.1).
- IPC pipes carry no ambient authority; a child gets only the handles explicitly passed.
- The compile-time absence of `std::os::unix` is a **security-relevant firewall**: it prevents fork/exec/
  signal patterns that have no safe SAS semantics from entering Tier 1.

## Next Steps
- With P1-P4, the app platform is functionally complete for panic=abort. P5 adds unwinding + upstreaming.
