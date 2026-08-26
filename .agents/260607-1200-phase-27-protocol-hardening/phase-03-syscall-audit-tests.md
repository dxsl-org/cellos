# Phase 03: Syscall Audit Event + Test Coverage

**Status**: 📋 Planned  
**Priority**: P2 (independent of Phase 01)  
**Effort**: ~0.5 day  
**Stage**: G1

---

## Overview

Two loose ends from Phase 27:

1. **Audit gap** — when the kernel denies a syscall via the allowlist filter, it emits
   `log::warn!` but nothing goes to the `audit` ring buffer.  Post-mortem tools that drain
   `/data/kernel.log` will miss denials entirely.  Adding `AuditEvent::SyscallDenied` closes
   this gap.

2. **Test gap** — `SyscallSet::permits()` logic and the `declare_syscalls!` bit-packing are
   untested.  The existing `syscall_tests.rs` only tests ID-to-variant mapping.  Add
   unit tests for allowlist logic.

---

## Requirements

### Audit event
1. Add `AuditEvent::SyscallDenied = 14` to `kernel/src/audit.rs`.
2. Payload: `encode_u32x2(caller_tid as u32, vi_syscall_raw_opcode as u32)` (matches the
   existing `encode_u32x2` helper pattern used by `CellSpawnDenied`).
3. In `kernel/src/task/syscall.rs`, replace the bare `log::warn!` in the allowlist check
   with `log::warn!` + `crate::audit::log_event(AuditEvent::SyscallDenied, …)`.

### Unit tests (`libs/api/src/syscall_tests.rs`)
4. `syscall_set_empty_permits_nothing` — `SyscallSet::EMPTY.permits(ViSyscall::Send) == false`.
5. `syscall_set_all_permits_everything` — `SyscallSet::ALL.permits(ViSyscall::Send) == true`.
6. `syscall_set_with_adds_bit` — `SyscallSet::EMPTY.with(ViSyscall::Send).permits(ViSyscall::Send)`.
7. `syscall_set_does_not_permit_unset` — after `.with(Send)`, `Recv` is not permitted.
8. `syscall_set_always_permitted_syscalls` — `SyscallSet::EMPTY.permits(ViSyscall::Exit) == true`
   (always-permitted syscalls bypass the filter regardless of the bitset).
9. `declare_syscalls_bits_are_stable` — verify a known `declare_syscalls![Send, Recv, Log]`
   produces the expected u64 value (bit 0 | bit 1 | bit 10 = `0x403`).

### Allowlist coverage: remaining cells
10. Add `api::declare_syscalls![…]` to the following cells that don't have one yet:
    - `cells/services/input` (from Phase 02, ensure it's done here if Phase 02 not yet complete)
    - `cells/services/config` (same)
    - `cells/apps/bench` — `Send, Recv, TryRecv, Log, Heartbeat, GetTime, SetTimer`
    - `cells/apps/hello` — `Log`
    - `cells/drivers/wasm` — `Send, Recv, Log, Heartbeat`

---

## Related Code Files

**Modify:**
- `kernel/src/audit.rs` — add `SyscallDenied = 14`
- `kernel/src/task/syscall.rs` — add audit call in allowlist check
- `libs/api/src/syscall_tests.rs` — add 6 unit tests
- `cells/apps/bench/src/main.rs` (or equivalent entry point)
- `cells/apps/hello/src/main.rs`
- `cells/drivers/wasm/src/lib.rs`

---

## Implementation Steps

1. In `kernel/src/audit.rs`, add `SyscallDenied = 14,` to `AuditEvent`.
2. In `kernel/src/task/syscall.rs`, in the allowlist check block, add:
   ```rust
   crate::audit::log_event(
       crate::audit::AuditEvent::SyscallDenied,
       &crate::audit::encode_u32x2(caller_id as u32, vi.allowlist_bit().unwrap_or(0xff) as u32),
   );
   ```
   Keep the existing `log::warn!` alongside the audit call.
3. In `libs/api/src/syscall_tests.rs`, add 6 tests in a `mod allowlist` block.
4. Add `declare_syscalls!` to bench, hello, wasm cells.
5. `cargo check` kernel + `cargo test --package api`.

---

## Todo List

- [ ] Add `AuditEvent::SyscallDenied = 14` to audit.rs
- [ ] Wire audit call into allowlist denial path in handle_syscall()
- [ ] Add `SyscallSet` unit tests in syscall_tests.rs
- [ ] Add `declare_syscalls!` to bench, hello, wasm cells
- [ ] `cargo check` kernel; `cargo test --package api`

---

## Success Criteria

- [ ] `AuditEvent::SyscallDenied` exists in audit.rs and fires on every denied syscall.
- [ ] Unit tests pass: `cargo test --package api`.
- [ ] bench, hello, wasm cells have `declare_syscalls!`.
- [ ] `cargo check` kernel clean.

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `SyscallDenied = 14` collides with a future variant | Low | Document that 14 is reserved; only add sequential IDs |
| Audit ring fills faster under heavy denial load | Very Low | Ring drops silently on full; no correctness issue |
