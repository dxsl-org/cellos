# Phase 27 Protocol Hardening

**Status**: 📋 Planned  
**Created**: 2026-06-07  
**Depends on**: Phase 27 Typed IPC + Syscall Filter (complete ✅)  
**Stage**: G1

---

## Context

Phase 27 added typed IPC enums (`VfsRequest`/`NetRequest`/`InputRequest`/`ConfigRequest`)
and wired the syscall allowlist filter into the kernel.  However:

- **VFS** is the only service that actually uses typed postcard IPC — Net, Input, and Config
  still use raw byte opcodes with manual byte parsing.
- The syscall allowlist check emits `log::warn!` on denial but writes no audit event to the
  ring buffer — denials are invisible to post-mortem tooling.
- `SyscallSet` and the `declare_syscalls!` macro have no unit tests.

This plan closes those gaps in three focused phases.

---

## Phase Map

| Phase | Title | Status | Est. | Law 1 touch? |
|-------|-------|--------|------|--------------|
| [01](phase-01-net-typed-ipc.md) | Net Typed IPC Migration | 📋 Planned | 2 days | ⚠️ YES — `libs/api/src/ipc.rs` adds 4 variants |
| [02](phase-02-input-config-typed-ipc.md) | Input + Config Typed IPC Migration | 📋 Planned | 1 day | No |
| [03](phase-03-syscall-audit-tests.md) | Syscall Audit Event + Test Coverage | 📋 Planned | 0.5 day | No |

Phases 02 and 03 are independent of each other and can run in parallel after Phase 01.

---

## Key Dependencies

```
Phase 27 (done)
    └─ Phase 01: Net typed IPC  (extends NetRequest in libs/api)
    └─ Phase 02: Input+Config   (no libs/api changes)
    └─ Phase 03: Audit + tests  (no libs/api changes)
```

Phase 02 and 03 are independent of Phase 01.

---

## Risk Summary

- **Law 1** — Phase 01 adds `UdpBind`, `GetLocalIp`, `MulticastJoin`, `MulticastLeave`
  to `NetRequest` in `libs/api/src/ipc.rs`. Requires **2× user confirmation** before
  implementation (per CLAUDE.md Law 1).
- **Net consumer blast radius** — curl, nc, httpd, ping, mqtt, wget all use
  `poll_driver::cell_opcodes`. All must migrate in Phase 01.
- **Kernel RX path stays raw** — the kernel-to-net Ethernet frame path (opcode 0x00) is NOT
  migrated to typed IPC; only the consumer cell socket API is.

---

## Success Criteria

- `cargo check` clean on all affected crates.
- Net, Input, and Config services use `api::ipc::decode` / `api::ipc::encode`.
- `poll_driver::cell_opcodes` and raw byte parsing removed from net cell.
- `AuditEvent::SyscallDenied` fires on every denied syscall.
- `SyscallSet::permits()` logic tested by unit tests.
