# CP1 — Session `PrepareChild` route 2: launch from VFS-supplied ELF bytes

Status: APPROVED by the user 2026-11-01 and implemented. Wire deviation: `reserved` stays 0, and route 2 appends `grant_id:u64, elf_len:u64` (grant ids may exceed 32 bits). A path with no launch edge exits 126 (`Denied`), not 127.
Direction chosen by the user 2026-11-01 ("Route launch qua VFS (CP1 mới)").

## Problem

The shell's foreground launch is `PrepareChild` with route=1, a kernel path read
(`kernel/src/task/cell_session.rs:1687` refuses any other route). The path
resolves only through `EarlyLoader::read_file`, i.e. VIFS1 plus the P2 cell
table (`kernel/src/loader/governed_spawn.rs:253`). On QEMU the kernel has no
block device, so P2 is unreadable.

Every program that lives only on disk therefore fails with
`foreground preparation failed: Io`: `tetris-c`, `tetris-lua`, `doom`, `lua`,
`ls`, `cat`, `ps`, `kill`, and so on. The old `sys_spawn_from_path` fallback (VFS
read into a grant, then `SpawnFromElf`) was lost when the shell moved to the
session.

## Proposal — wire/ABI delta (Law‑1)

`PrepareChild` body: `[ref operation][u32 route][u32 path_len][u32 context_len][u32 reserved=0][path]`.

| route | Meaning | Who reads the bytes |
|---|---|---|
| 1 (existing) | kernel path route, `LaunchRoute::Path` | kernel loader (VIFS1/P2) |
| **2 (new)** | caller-supplied ELF, `LaunchRoute::Elf` | the caller passes a read-only **grant** of the ELF. The `reserved` field becomes `grant_id`, followed by a `u64 elf_len` |

**Kernel, route 2:**
1. Authorize the exact edge `(caller, LaunchRoute::Elf, path)` with
   `launch_profile::authorize`. It already refuses a capability-bearing ceiling
   on the ELF route (`launch_profile/mod.rs:108-113`): "caller-owned bytes must
   not borrow authority". Network tools (`NET_CLIENT`), `/bin/shutdown`
   (Path-only), `hypervisor`, `hypha` and similar therefore stay route 1, and
   route 2 only ever yields `LEGACY`/empty-ceiling children.
2. Copy the grant bytes into a kernel buffer, bounded by the existing ELF size
   limit, then run the **same** `prepare_gated` as route 1: signature and
   manifest gate, tier and domain classification, measurement digest over the
   exact bytes. A domain child then goes through `admit_and_bind_domain`
   (ADR-0019, P1).
3. `SessionPublication` records the route. `current()` re-authorizes with that
   same route, not hard-coded `Path`.

**`libs/terminal`:** `launch_foreground` tries route 1 first. Only on
`Status::NotFound` does it read the file through VFS
(`ostd::fs::read_full_via_grant`) and retry once with route 2. Every other
status (`Denied`, `Io` and so on) is final, so route 2 can never be used to get
around a refusal. The grant is freed after `PrepareChild` returns.

**Companion fix (required by the fallback rule):** `cell_session.rs:450` `vi()`
gains `ViError::NotFound => Status::NotFound`. Today a missing program is
reported as `Io`, so the shell's 127 branch (`executor.rs:3732`) never fires.

## Security notes

- The bytes come from the caller. As with `SpawnFromElf` today, the signature
  check (dev or release key per posture) is what binds them to an artifact, and
  the measurement log records the digest of the exact bytes launched.
- The path is advisory except for edge lookup. The ELF route edge table already
  exists and is narrower than the Path one, so no new authority is created.
- Unsigned bytes are admitted only as Tier‑2 domains in dev posture, through
  P1's admission. Release posture denies them.

## Files

- `kernel/src/task/cell_session.rs`: route parse, `vi()` mapping.
- `kernel/src/loader/governed_spawn.rs`: `prepare_session_elf(bytes, path)`
  next to `prepare_session_path`.
- `kernel/src/task/launch.rs`: route stored in `SessionPublication`, `current()`.
- `kernel/src/task/syscall.rs`: `session_launch_request` gains a route
  parameter.
- `libs/terminal/src/{stream,session}.rs`: route‑2 encoder, fallback.
- `libs/api` cell_session ABI docs and constants, if route values are named
  there.
- Kernel and terminal host tests; `CHANGELOG.md`.

## Evidence plan

- **Host tests:**
  - route parse: route 2 needs `grant_id`/`elf_len`, unknown route → `Malformed`;
  - the fallback triggers only on `NotFound`;
  - `vi(NotFound)` → `NotFound`.
- **RV64 runtime:**
  - from the shell: `tetris-c` (Tier‑2 domain, serial `[domain] admitted`),
    `ls /`, `cat`, `ps`;
  - a capability-bearing disk-only program is still refused on route 2;
  - a nonexistent program returns 127;
  - full RV64 boot suite against 51/65.
- **Regression:** `tier2-fault-isolation`, `posix_shim_*`, `atomic-publication`,
  the x86 candidate lanes.

## Risk

Medium:
- a new wire route;
- larger transient kernel memory, one ELF copy per launch, bounded by the
  existing limit;
- the fallback rule must stay exactly `NotFound`.

Rollback: revert; route 1 is unchanged.
