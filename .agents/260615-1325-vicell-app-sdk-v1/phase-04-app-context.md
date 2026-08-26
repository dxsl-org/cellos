# Phase 04 — AppContext: Unified Entry + Event Loop

**Status**: 📋 Planned  
**Priority**: P2  
**Estimate**: ~1 day  
**Depends on**: Phase 02 (ServiceRef) + Phase 03 (dispatch)

## Context Links

- Codebase: [libs/ostd/src/lib.rs](../../libs/ostd/src/lib.rs) · [cells/apps/hello/src/main.rs](../../cells/apps/hello/src/main.rs) · [cells/apps/robot-demo/src/main.rs](../../cells/apps/robot-demo/src/main.rs)
- Building on: `service.rs` (Phase 02) + `dispatch.rs` (Phase 03)

## Overview

Even with `ServiceRef` and `run_service`, a one-shot app still needs to:
1. Construct service handles
2. Do work
3. Call `sys_exit(0)`

`AppContext` bundles all standard service handles. `run_app` provides the one-shot entry pattern. `run_event_loop` provides the event-driven pattern for apps that receive messages (not just send).

```rust
// One-shot app (reads a file, prints it, exits)
#[no_mangle]
pub fn main() {
    ostd::app::run_app(|ctx| {
        let resp: VfsResponse = ctx.vfs.call(&VfsRequest::Read { cap: 0 }).unwrap();
        ostd::io::println("done");
    });
}

// Event-driven app (receives messages until Shutdown)
#[no_mangle]
pub fn main() {
    ostd::app::run_event_loop(|ctx, event| match event {
        AppEvent::Message { sender, data } => { /* handle */ true }
        AppEvent::Shutdown => false,
    });
}
```

## Key Insights

- `AppContext` is stack-allocated in `run_app` / `run_event_loop` — no heap overhead at startup
- All service handles start uncached (`None`) and lazy-resolve on first `call()`
- `run_event_loop` needs a `Shutdown` signal. In ViCell there's no `SIGTERM` equivalent, but init sends a message to supervised cells before forced kill. For v1, `Shutdown` is delivered when the cell receives a message with `sender == 1` (init TID) and the message decodes as a shutdown opcode (0xFF). This is a convention, not a kernel mechanism.
- `run_app` calls `sys_exit(0)` after the closure — prevents cells that forget `sys_exit` from hanging

## Requirements

- `AppContext` struct with `pub vfs: VfsRef`, `pub net: NetRef`, `pub input: InputRef`, `pub config: ConfigRef`, `pub compositor: CompositorRef`
- `AppContext::new() -> Self` (all ServiceRef handles start uncached)
- `run_app<F: FnOnce(&mut AppContext)>(f: F)` — calls f, then sys_exit(0)
- `AppEvent<'a>` enum: `Message { sender: usize, data: &'a [u8] }`, `Shutdown`
- `run_event_loop<F: FnMut(&mut AppContext, AppEvent<'_>) -> bool>(f: F) -> !`
  - Calls `f` with `AppEvent::Shutdown` when shutdown signal received; if `f` returns `false`, calls `sys_exit(0)`
  - Calls `f` with `AppEvent::Message` for all other senders

## Architecture

```
libs/ostd/src/app.rs  (new)

use crate::service::{VfsRef, NetRef, InputRef, ConfigRef, CompositorRef};
use crate::syscall::{sys_recv, sys_exit, sys_yield, SyscallResult};
use api::ipc::IPC_BUF_SIZE;

// Opcode convention for graceful shutdown messages (from init)
const SHUTDOWN_OPCODE: u8 = 0xFF;

pub struct AppContext {
    pub vfs:        VfsRef,
    pub net:        NetRef,
    pub input:      InputRef,
    pub config:     ConfigRef,
    pub compositor: CompositorRef,
}

impl AppContext {
    pub fn new() -> Self { /* all ServiceRef::new() */ }
}

pub enum AppEvent<'a> {
    Message  { sender: usize, data: &'a [u8] },
    Shutdown,
}

pub fn run_app<F: FnOnce(&mut AppContext)>(f: F) {
    let mut ctx = AppContext::new();
    f(&mut ctx);
    sys_exit(0);
}

pub fn run_event_loop<F>(mut handler: F) -> !
where F: FnMut(&mut AppContext, AppEvent<'_>) -> bool
{
    let mut ctx = AppContext::new();
    let mut buf = [0u8; IPC_BUF_SIZE];
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(sender) if sender > 0 => {
                let event = if buf[0] == SHUTDOWN_OPCODE {
                    AppEvent::Shutdown
                } else {
                    AppEvent::Message { sender: sender as usize, data: &buf }
                };
                if !handler(&mut ctx, event) {
                    sys_exit(0);
                }
                buf = [0u8; IPC_BUF_SIZE];
            }
            _ => sys_yield(),
        }
    }
}
```

## Related Code Files

- **New**: `libs/ostd/src/app.rs`
- **Modify**: `libs/ostd/src/lib.rs` — add `pub mod app`
- **Modify**: `libs/ostd/src/prelude.rs` — optionally re-export `AppContext`, `AppEvent`, `run_app`, `run_event_loop`

## Implementation Steps

1. Create `libs/ostd/src/app.rs`
2. Implement `AppContext` struct using `VfsRef`/`NetRef`/etc. from Phase 02
3. Implement `run_app<F: FnOnce>` helper
4. Define `AppEvent<'a>` enum
5. Implement `run_event_loop<F: FnMut>` 
6. Add `pub mod app;` to `libs/ostd/src/lib.rs`
7. `cargo check`

## Todo List

- [ ] Create libs/ostd/src/app.rs
- [ ] Implement AppContext { vfs, net, input, config, compositor }
- [ ] Implement AppContext::new()
- [ ] Define AppEvent<'a> (Message + Shutdown)
- [ ] Implement run_app<F: FnOnce>
- [ ] Implement run_event_loop<F: FnMut> → !
- [ ] pub mod app in lib.rs
- [ ] cargo check clean

## Success Criteria

```rust
// One-shot pattern compiles and runs:
ostd::app::run_app(|ctx| {
    let _: VfsResponse = ctx.vfs.call(&VfsRequest::Stat { path: "/etc/cfg" }).unwrap();
});

// Event loop pattern compiles:
ostd::app::run_event_loop(|_ctx, event| {
    matches!(event, AppEvent::Message { .. })  // true = keep going
});
```

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `SHUTDOWN_OPCODE = 0xFF` conflicts with a real postcard discriminant | Postcard uses varint for enum discriminants; 0xFF would only appear for enum variant 127 in a ≥128-variant enum. Existing request types have far fewer variants. Document the convention; can be made more robust later with a distinct message type. |
| `run_app` calling `sys_exit(0)` after `f` — if `f` itself calls `sys_exit`, double-exit occurs | `sys_exit` is `-> !` — if f calls it, run_app's call to sys_exit is unreachable. No problem. |
| AppContext size on stack (5 × ServiceRef = 5 × sizeof(Option<usize>) = 40 bytes) | Negligible. |

## Security Considerations

`run_event_loop` processes messages from any sender — callers must validate sender TID in their `AppEvent::Message` handler. This is no worse than the existing sys_recv loop; the SDK makes it explicit by providing sender in the event.
