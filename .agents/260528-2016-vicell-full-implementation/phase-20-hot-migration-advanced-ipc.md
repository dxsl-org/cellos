# Phase 20 — Hot Migration & Advanced IPC

**Effort:** 180h | **Priority:** P3 | **Status:** pending | **Blockers:** Phase 06, Phase 13

## Overview

Two related capabilities that distinguish ViCell from traditional Unixes:

1. **Hot Cell migration**: live-replace a running Cell's code (e.g., shell v1.0 → shell v1.1) without losing in-flight state. Required by `libs/api/src/hotswap.rs` which already defines the `ViStateTransfer` trait.
2. **Advanced IPC**: timeouts on `Recv`, lease auto-revocation, grant chains with bounded delegation depth, scatter/gather bulk messages.

After this phase, ViCell supports zero-downtime upgrades and richer IPC semantics that scale to production-grade Cells.

## Context Links

- `libs/api/src/hotswap.rs` — existing `ViStateTransfer` trait scaffold
- `libs/api/src/async_io.rs` — IPC types
- Phase 06 (external ELF loading) — needed to load new versions from disk
- Phase 13 (VFS) — old/new ELFs both live on disk

## Key Insights

- Hot-swap protocol: (1) freeze old Cell (kernel queues incoming msgs, doesn't deliver), (2) call `serialize()` → owned bytes, (3) load new ELF, (4) call `deserialize(bytes)` in new instance, (5) atomic registry switch — new Cell receives queued msgs.
- State schema must be versioned. If new Cell's `deserialize` cannot interpret old bytes, abort migration and keep old Cell. **Migration test**: serialize → deserialize → equal — must hold in CI.
- `Recv` timeout: add `timeout_ticks` field to the syscall variant. Kernel arms a timer; on expiry, wakes task with `ViError::Timeout`. Easy to get wrong if waker races with timer fire — use compare-and-swap on the waker slot.
- Lease auto-revoke: a `Lease` cap has `expires_at: u64` in absolute kernel ticks. Every cap lookup checks expiry; expired ones return `ViError::Expired`. Lazy approach, no timer needed.
- Grant chains: `GrantDepth(n)` — a cap with depth n can be re-granted, decrementing to n-1; depth 0 cannot be re-granted. Bound depth to 4 by default.
- Scatter/gather: `SendGather(&[(&[u8])])` builds a single message from multiple non-contiguous buffers; `RecvScatter(&mut [&mut [u8]])` writes into multiple buffers. Avoids one big copy.

## Requirements

**Functional**
- `ViStateTransfer` impl on shell + config + vfs cells
- `HotSwap(cell_id, new_elf_path)` syscall does freeze→serialize→load→deserialize→resume
- During swap, no incoming messages are lost (queued + delivered to new Cell)
- `Recv { timeout_ticks }` returns `Timeout` on expiry
- Leases auto-revoke at `expires_at`
- Grant chains: delegation depth enforced
- SendGather/RecvScatter built and benchmarked vs single-buffer baseline

**Non-functional**
- Hot-swap end-to-end latency < 500ms for a small (< 100 KB) state Cell
- Timeout precision ± 10ms in QEMU
- Bulk message throughput ≥ 20% better than single-buffer for large payloads

## Architecture

```
HotSwap protocol:
   kernel.cell_registry → mark cell "frozen"
       └─ queued msgs kept in pending queue
   kernel.task.spawn_syscall_in_cell(cell_id, Serialize) → owned bytes
   kernel.cell_registry.load_new_elf(cell_id, new_path) → replaces code pages
   kernel.task.spawn_syscall_in_cell(cell_id, Deserialize(bytes)) → state restored
   kernel.cell_registry.mark "live" → queued msgs flushed

Recv timeout:
   sys_recv(mask, buf, timeout_ticks):
       state = TaskState::WaitingRecv { …, deadline: now + timeout_ticks }
       arm timer ← deadline
       block → either:
            - msg arrives → consume, cancel timer, return Ready
            - timer fires → mark Ready with ViError::Timeout

Lease expiry:
   on cap lookup:
       if entry.lease.is_some() && entry.lease.expires_at <= now: return Err(Expired)
       else proceed
```

## Related Code Files

**Modify:**
- `libs/api/src/hotswap.rs` — finalize `ViStateTransfer` trait + helpers
- `libs/api/src/syscall.rs` — add `HotSwap`, `Recv { timeout }`, `SendGather`, `RecvScatter`, lease-aware ops
- `libs/api/src/async_io.rs` — extend `IpcMessage` with optional gather/scatter
- `kernel/src/task/syscall.rs` — dispatch new variants
- `kernel/src/cell/registry.rs` — `freeze/unfreeze`, `load_new_elf`, lease expiry check
- `kernel/src/task/tcb.rs` — add `Recv { deadline }` field
- `kernel/src/task/scheduler.rs` — wake-on-timer for Recv timeout
- `cells/apps/shell/src/main.rs` — impl `ViStateTransfer` for shell state
- `cells/services/config/src/main.rs` — impl `ViStateTransfer` for config KV
- `cells/services/vfs/src/main.rs` — impl `ViStateTransfer` for VFS state (just open handle table; mounts re-discoverable)

**Create:**
- `kernel/src/task/recv_timer.rs` — deadline wheel for Recv timeouts
- `kernel/src/cell/hotswap.rs` — orchestrate the 5-step swap
- `cells/sys-tools/src/bin/hotswap.rs` — CLI wrapper invoking the syscall (admin tool)
- `tests/integration/hotswap_shell.rs` — start shell v1, hotswap to shell v2 mid-command, assert no message loss
- `tests/integration/recv_timeout.rs` — wait 100ms then expect Timeout
- `tests/integration/lease_revoke.rs` — open file with 50ms lease, sleep 100ms, expect Expired
- `tests/integration/scatter_gather.rs` — send 3-segment msg, recv into 3 buffers
- `docs/hotswap-guide.md` — how Cell authors implement `ViStateTransfer`

## Implementation Steps

### Phase 20.1 — Recv timeout (24h)

1. Extend syscall variant: `Recv { source_mask, buf, timeout_ticks: u64 }`
2. Add deadline wheel `kernel/src/task/recv_timer.rs`:
   - Sorted heap of `(deadline, TaskId)`
   - On timer IRQ: pop entries ≤ now, wake those tasks with `ViError::Timeout`
3. In `sys_recv`: register waker AND register in deadline wheel; on msg arrival cancel deadline; on timeout cancel waker
4. Integration test `recv_timeout.rs`: spawn task that Recv with 100ms timeout, no sender; assert returns Timeout in 100±10ms

### Phase 20.2 — Lease auto-revoke (24h)

5. Extend cap entry: `lease: Option<Lease { expires_at: u64 }>`
6. Cap lookup checks expiry; returns `ViError::Expired` if past
7. API: `Cap::with_lease(self, duration_ticks) -> Self`
8. Integration test `lease_revoke.rs`: open file with 50ms lease; spin until 100ms past; expect Expired

### Phase 20.3 — Grant chains (24h)

9. Add `GrantDepth(u8)` field to cap entry (default 4)
10. On `Grant(cap, target_cell)`: if depth == 0 → error; else clone cap to target with depth-1
11. Test: 5-deep grant chain — last one fails

### Phase 20.4 — Scatter/gather (32h)

12. Define `IoVec { ptr, len }` array argument in syscall variants
13. Kernel reads each segment into the destination message via Linkable contiguous copy
14. Benchmark vs single-buffer: assert ≥ 20% throughput improvement at 64KB messages

### Phase 20.5 — Hot migration (76h)

15. Finalize `ViStateTransfer`:
   ```rust
   pub trait ViStateTransfer: Sized {
       const SCHEMA_VERSION: u32;
       fn serialize(&self) -> Result<Box<[u8]>, ViError>;
       fn deserialize(bytes: &[u8]) -> Result<Self, ViError>;  // verifies version
   }
   ```
16. Cell registers its `Box<dyn ViStateTransfer>` instance with kernel at startup
17. `HotSwap` syscall orchestrator `kernel/src/cell/hotswap.rs`:
    - `freeze(cell_id)`: pause new msg delivery, queue incoming
    - `serialize_in_cell(cell_id)`: invoke registered serializer (cell-side call via internal IPC)
    - `replace_code(cell_id, new_elf_bytes)`: unmap old code, load new
    - `deserialize_in_cell(cell_id, bytes)`: invoke new instance's deserialize
    - `unfreeze(cell_id)`: flush queue, resume normal delivery
18. Implement `ViStateTransfer` for:
    - **Config Cell**: serialize KV map
    - **Shell Cell**: serialize history + current job table + cwd
    - **VFS Cell**: serialize open handle table (mounts re-mount on deserialize)
19. CLI tool `cells/sys-tools/src/bin/hotswap.rs`:
   ```
   hotswap <cell-name> <new-elf-path>
   ```
20. Integration test `hotswap_shell.rs`:
   - Start shell v1
   - Have shell run `sleep 5 &`
   - hotswap shell to v2 mid-sleep
   - Assert: job still tracked, history preserved, shell prompt returns
21. Document `docs/hotswap-guide.md`: how to implement `ViStateTransfer`, schema-version migration rules, what cannot survive a hotswap (active syscall in flight)

## Todo List

- [ ] Implement Recv timeout (syscall variant + deadline wheel)
- [ ] Implement lease auto-revoke
- [ ] Implement grant-chain depth bound
- [ ] Implement SendGather/RecvScatter
- [ ] Benchmark scatter/gather (≥ 20% improvement at 64KB)
- [ ] Finalize `ViStateTransfer` trait + register API
- [ ] Implement `HotSwap` orchestrator (freeze/serialize/replace/deserialize/unfreeze)
- [ ] Implement `ViStateTransfer` for Config, Shell, VFS cells
- [ ] CLI: `cells/sys-tools/src/bin/hotswap.rs`
- [ ] Integration tests: timeout, lease, scatter/gather, hotswap-shell
- [ ] Document `docs/hotswap-guide.md`
- [ ] CI green

## Success Criteria

- All 4 sub-features pass integration tests
- Hot-swap shell with no message loss; history + jobs preserved
- Timeout precision ± 10ms
- Scatter/gather ≥ 20% throughput improvement
- Lease expiry observable; expired caps cleanly rejected
- Grant depth bound enforced

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Hot-swap leaks frames if deserialize fails mid-way | Med | High | Two-phase: parse new ELF + run deserialize in a *temporary* twin first; only commit if both succeed |
| State serialization explodes in size (Shell history with 10K entries) | Med | Med | Cap per-cell serialized state at 4 MB; truncate with warning |
| Recv timeout races: msg + timer arrive same tick | High | Low | Cancel order matters; document; test specifically |
| Cell with native pointers in state cannot serialize | Cert | High | Document: `ViStateTransfer` must NOT include raw pointers; Cells re-resolve handles on deserialize |
| Grant-chain depth 4 too restrictive for legit chains | Low | Low | Make depth configurable per cap type; default 4 |

## Security Considerations

- Hot-swap is privileged (requires `CELL_ADMIN` cap) — non-admin cells cannot replace others
- Old Cell code pages must be zeroed before reuse (info leak otherwise)
- Serialized state crosses the cell boundary via kernel — kernel validates schema version before deserialize
- Lease bypass: a cell holding a lease cap and re-granting it without lease is forbidden (kernel enforces lease propagation)

## Rollback

Sub-features (timeout, lease, grant chain, scatter/gather) each ship as independent commits and can be reverted individually. Hot-swap is one bigger PR; revert removes the orchestrator + the `ViStateTransfer` impls (cells still function without them — just can't be hot-swapped).

## Next Steps

Hot-swap unlocks zero-downtime upgrades for v1.x. Phase 22 benchmarks swap latency. Patterns from this phase feed potential future "live migration across hosts" (post-v1.0).
