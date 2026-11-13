# Cellos Hot-Swap Guide

Live-replace a running service Cell with a new incarnation of the same service
without losing its state. Proven for `config` only (lane `config-hotswap-x86`);
no other production service hands over state, so they upgrade by restart.

---

## How It Works

The supervisor Cell orchestrates the swap (`cells/services/supervisor/src/hotswap.rs`);
the kernel only provides the primitives.

```
1. Supervisor pauses the service's registry entry and posts Snapshot
   [0xAC, 0xF0, swap_id LE] to the old Cell
2. Old Cell stashes its state in the kernel under hotswap_key(swap_id)
3. Supervisor freezes the old Cell; SpawnReplacement loads the new ELF and the
   kernel seals the stash to it
4. Supervisor posts Restore [0xAC, 0xF1, decimal swap id] to the new Cell; it
   restores, clears the slot and calls sys_hotswap_ready()
5. CommitHotswap drains old ingress into the new Cell and publishes its tid;
   the supervisor KillCells the old Cell (exit reason 0xAAAA_AAAA)
6. Init sees that reason and a live, different provider for the service id:
   it watches the new tid instead of restarting the service
```

Any failure before the commit kills the replacement and resumes the old Cell.
There is no symbol relinking: the new Cell is a fresh process.

### Who can read the stashed state (2026-10-31)

The kernel stash entry belongs to the Cell incarnation that wrote it. A guessed
key gives another Cell nothing: reads return 0, writes and clears are refused.
`SpawnReplacement` seals the old incarnation's entries to the replacement; from
then on only the replacement reads them, and clearing them consumes them. The
old instance can no longer write (fenced). The supervisor sees only whether state
exists (`sys_state_restore(key, &mut [])` returns the length) and may abort with
clear; it never receives the bytes. A Cell restarted any other way does not
inherit its predecessor's stash. When an incarnation retires, the kernel removes
the open entries it owns and the sealed entries addressed to it, in the same
retirement funnel that releases its other resources.

### Restore failure is RecoveryFailed (2026-11-01, Phase 09B)

A replacement that finds no valid state (missing, malformed or wrong schema)
exits as RecoveryFailed and never calls `sys_hotswap_ready()`; there is no
cold-start opt-in in the demos. The supervisor's ready wait then fails and it
rolls back to the old Cell. The stash is byte transport only: it does not carry
business commits, writer epochs or request deduplication.

### Crash recovery needs a state authority (Phase 09B)

The stash cannot survive a crash: sealing needs a live, frozen source. A
provider that must keep committed state across a worker crash keeps it in a
separate state-authority Cell (pattern: `cells/demos/recovery-demo`):

- the worker commits `(caller, method, request id, payload, result, revision)`
  at the authority before replying, under a writer epoch the authority issued;
- on worker death the authority fences that epoch, captures the committed
  watermark, and reads the kernel retirement verdict (`QueryRetirement`, 431,
  record `ViRetirementV1`): `Unproven` (untracked SHM export or quarantine
  overflow) means node recovery, no restart; `Contained` or `Quarantined`
  (counted, bounded) allows a replacement;
- the replacement attaches with a one-shot nonce staged in its argv, restores,
  validates schema and revision, and only then is published (Ready);
- a retried request with the same binding returns the recorded result; the same
  id with a different payload or method is refused (Conflict).

That state is RAM-only: it survives a worker crash, not an authority crash,
reboot or power loss. Durable retention requires the CellosFS state owner.

### Init restarts only after a retirement verdict (Phase 09C)

Every service init restarts (policy `always` or `on-failure`) is restarted only
after init has read the dead root's `QueryRetirement` verdict (bounded wait,
200 scheduler ticks):

- `Contained`, or `Quarantined` for counted pins/VFS leases only → restart;
- `Quarantined` with deferred DMA teardown → no restart for the rest of the boot
  (the device's PCI function stays retained; the device is unavailable);
- `Unproven`, no record, a refusal or still `Pending` at the bound → no restart,
  `node recovery required`.

`restart = "never"` services are not queried. The crash budget (5 restarts per
1000 ticks) is charged before the verdict; a refused restart keeps that charge.
A restarted block driver regains only the route of the device it owns
(`docs/vfs-api.md`, Mount Points).

---

## Implementing a swappable service

A service on `ostd::service::serve` implements the two `Handler` hooks; `serve`
accepts the Snapshot/Restore envelopes only from the live `SUPERVISOR` provider,
never replies to them, stashes what `on_snapshot` returns, and after a `true`
from `on_restore` clears the slot and calls `sys_hotswap_ready()`. The state type
implements `ostd::hotswap::ViStateTransfer`, whose versioned header
`encode_transfer`/`decode_transfer` write and check (config: `cells/services/config/src/main.rs`):

```rust
use ostd::hotswap::{decode_transfer, encode_transfer, ViStateTransfer};

impl ViStateTransfer for Store {
    const SCHEMA_VERSION: u32 = 1; // bump on incompatible changes
    fn serialize(&self) -> Result<Box<[u8]>, ViError> { /* … */ }
    fn deserialize(version: u32, bytes: &[u8]) -> Result<Self, ViError> { /* … */ }
}

impl ostd::service::Handler for Service {
    fn on_request(&mut self, request: ostd::service::Request<'_>) { /* … */ }
    fn on_snapshot(&mut self) -> Option<Vec<u8>> {
        encode_transfer(&self.store).ok()
    }
    fn on_restore(&mut self, state: &[u8]) -> bool {
        decode_transfer::<Store>(state).map(|store| self.store = store).is_ok()
    }
}
```

The service's syscall allowlist needs `StateStash`, `StateRestore`,
`StateStashClear` and `HotSwapReady`, and the supervisor's target map
(`service_id_for_name`) must name it. An `AppContext` Cell handles
`AppEvent::Snapshot`/`AppEvent::Restore` itself (`cells/demos/hotswap-demo-v1`/`-v2`).

---

## Schema Versioning Rules

- **Bump `SCHEMA_VERSION`** whenever the binary layout changes (field added,
  removed, or reordered).
- `deserialize` receives the serializer's version and must handle every
  version it accepts. An unrecognised one returns `Err(ViError::InvalidInput)`:
  the replacement never signals Ready and the supervisor keeps the old Cell.
- Adding new optional fields at the **end** of the payload is safe with a
  check of the payload length.  Removing or reordering fields requires a version bump.

---

## What Can and Cannot Survive a Hot-Swap

| State | Survives? | Notes |
|-------|-----------|-------|
| In-memory data (counters, KV map, history) | ✅ Yes | Via `ViStateTransfer` |
| IPC messages in flight | ✅ Yes | Kernel queues them during freeze |
| Open file descriptors (POSIX) | ❌ No | POSIX shim re-opens on new Cell startup |
| Capability handles (`ViFileHandle`) | ⚠️ Partial | VFS cell must re-issue caps to new owner |
| Pending async futures | ❌ No | Async executor state is not serialisable in v1.0 |
| Hardware IRQ state | ❌ No | Driver Cells cannot currently be hot-swapped |

---

## Triggering a Hot-Swap

The `hotswap` admin tool (`cells/tools/sys-tools/src/bin/hotswap.rs`) asks the
Supervisor; only that signed program may request a swap, and only `config` and
`hotswap-demo` are targets. The replacement path needs a boot or reviewed
catalog row, so config is swapped onto its own `/bin/config`:

```
Cellos > hotswap config /bin/config
hotswap: success: supervisor swapped service 'config' to /bin/config
```

The legacy `HotSwap` syscall (id 400) path is retired/reserved and decodes `Unknown`; `HotSwapReady` and `Snapshot` keep the shared bit-32 compatibility bucket.

---

## Implemented Cells

| Cell | State Serialised | Schema Version | Status |
|------|-----------------|----------------|--------|
| Config | KV map | v1 | ✅ `config-hotswap-x86` |
| hotswap-demo-v1 → v2 | Counter | v1 | Demo, manual `hotswap-smoke` lane |

---

## Lease Auto-Revocation

A cap with `expires_at` set is silently revoked on the next `verify()` call
after the deadline passes.  The caller receives `ViError::PermissionDenied`.

```rust
// Example: allocate a cap that expires after 1 second (10M ticks at 10 MHz)
let now = crate::task::system_ticks() as u64;
let cap = table.alloc_with_lease(owner, resource, perms, now + 10_000_000);
```

---

## Files

| File | Purpose |
|------|---------|
| `libs/ostd/src/hotswap.rs` | `ViStateTransfer`, stash helpers, transfer header |
| `libs/ostd/src/service/serve.rs` | Snapshot/Restore routing to `Handler::on_snapshot`/`on_restore` |
| `cells/services/supervisor/src/hotswap.rs` | Swap orchestration and rollback |
| `kernel/src/cell/cap_registry.rs` | Capability allocation and lease expiry enforcement |
| `kernel/src/cell/hotswap.rs` | Live cutover helpers, `HotSwapReady`, and bit-32 compatibility bookkeeping |
| `cells/services/config/src/main.rs` | Config KV state transfer |
| `cells/tools/init/src/supervisor.rs` | Init adopting the swapped-in provider |
| `cells/tools/sys-tools/src/bin/hotswap.rs` | Supervisor IPC hotswap client |
