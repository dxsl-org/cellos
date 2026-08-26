# ViCell App SDK v1 — Plan

**Status**: 📋 Planned  
**Created**: 2026-06-15  
**Stage**: G1 tail / G2 prep  
**Law 1**: No syscall additions. No `libs/api/` changes. All work in `libs/ostd/`.

## Goal

Eliminate the boilerplate every cell must write today:
1. 8-line retry loop for service discovery
2. `recv / decode / dispatch / encode / send` service loop
3. Manual `sys_exit` + context setup

Deliver three composable helpers: `ServiceRef`, `MessageHandler` + `run_service`, and `AppContext` + `run_app`.

## Phases

| # | Phase | Status | Priority | Parallel |
|---|-------|--------|----------|----------|
| 01 | [ostd foundation](phase-01-ostd-foundation.md) — hashbrown + embedded-io | 📋 Planned | P1 | standalone |
| 02 | [ServiceRef](phase-02-service-ref.md) — typed service discovery handle | 📋 Planned | P1 | parallel w/ 03 |
| 03 | [MessageDispatch](phase-03-message-dispatch.md) — typed recv loop | 📋 Planned | P1 | parallel w/ 02 |
| 04 | [AppContext](phase-04-app-context.md) — unified entry + event loop | 📋 Planned | P2 | after 02+03 |
| 05 | [Cell migration + integration test](phase-05-migration.md) — sdk-demo + CI test | 📋 Planned | P2 | after 04 |

## Dependency Graph

```
Phase 01 (ostd foundation)
         │
    ┌────┴────┐
    ▼         ▼
Phase 02   Phase 03     ← run in parallel
(ServiceRef) (dispatch)
    │         │
    └────┬────┘
         ▼
     Phase 04
   (AppContext)
         │
         ▼
     Phase 05
  (migration + test)
```

Phase 01 is fast (~1 day) and unblocks 02+03 which can run in parallel. Total estimated wall-clock: ~4 days.

## Key Design Decisions

- **No new crate**: everything lives in `libs/ostd/` as new modules. Cells already depend on ostd; no Cargo.toml churn in cells.
- **`&mut self` on `ServiceRef::resolve`**: cache is a plain `Option<usize>`, no `UnsafeCell` needed since AppContext is always `&mut`.
- **`MessageHandler` trait works for all existing services**: `Request`/`Response` are enums (already the case for VfsRequest, ConfigRequest, etc.).
- **No forced migration**: existing cells that are simpler without the SDK stay as-is. Phase 05 adds one new `sdk-demo` cell.
- **Zero Law 1**: `sys_lookup_service=206` and all required syscalls already exist. No new kernel surface.

## Files Created/Modified

| File | Change |
|------|--------|
| `libs/ostd/Cargo.toml` | add hashbrown, embedded-io |
| `libs/ostd/src/collections.rs` | new — HashMap re-export |
| `libs/ostd/src/io.rs` | add embedded-io impls on File + Stdin |
| `libs/ostd/src/prelude.rs` | add HashMap + embedded_io traits |
| `libs/ostd/src/service.rs` | new — ServiceRef<const ID: u16> |
| `libs/ostd/src/dispatch.rs` | new — MessageHandler + run_service |
| `libs/ostd/src/app.rs` | new — AppContext + run_app + AppEvent |
| `libs/ostd/src/lib.rs` | add pub mod for new modules |
| `cells/apps/sdk-demo/` | new cell exercising the full SDK |
| `tests/integration/tests/boot.rs` | add sdk_app_context test |

## Success Criteria (overall)

- `cargo check` passes with zero errors
- A cell can call `ostd::app::run_app(|ctx| { ctx.vfs.call(...) })` with no manual boilerplate
- A service can implement `MessageHandler` and call `run_service(&mut handler, Some(500))` replacing its hand-rolled loop
- `sdk-demo` cell boots, resolves VFS, reads a file, exits cleanly (verified in CI test)
- No regressions in existing integration tests
