# Phase 03 — MessageDispatch: Typed Service Loop

**Status**: 📋 Planned  
**Priority**: P1  
**Estimate**: ~1 day  
**Parallel**: runs alongside Phase 02 (different file — no conflict)  
**Depends on**: Phase 01 (Cargo.toml foundation)

## Context Links

- Codebase: [cells/services/config/src/main.rs](../../cells/services/config/src/main.rs) · [cells/services/vfs/src/main.rs](../../cells/services/vfs/src/main.rs) · [cells/services/input/src/main.rs](../../cells/services/input/src/main.rs)
- Pattern: all three services implement the same `loop { sys_recv; decode; handle; encode; sys_send }` structure

## Overview

Every service today hand-rolls the same recv/dispatch loop:

```rust
// Current — repeated in config, vfs, input, etc.
let mut buf = [0u8; IPC_BUF_SIZE];
let mut resp_buf = [0u8; IPC_BUF_SIZE];
loop {
    match ostd::syscall::sys_recv(0, &mut buf) {
        SyscallResult::Ok(sender) if sender > 0 => {
            let req = api::ipc::decode::<MyRequest>(&buf).unwrap();
            let resp = handle(req, sender);
            let enc = api::ipc::encode(&resp, &mut resp_buf).unwrap();
            ostd::syscall::sys_send(sender, enc);
            buf = [0u8; IPC_BUF_SIZE];
        }
        _ => ostd::task::yield_now(),
    }
}
```

`MessageHandler` trait + `run_service()` replaces this with:

```rust
// After SDK
struct MyHandler { /* state */ }
impl MessageHandler for MyHandler {
    type Request  = MyRequest;
    type Response = MyResponse;
    fn handle(&mut self, req: MyRequest, sender: usize) -> MyResponse { ... }
}
run_service(&mut MyHandler { ... }, Some(500));
```

## Key Insights

- All existing service `Request`/`Response` types are enums (VfsRequest, ConfigRequest, etc.) — the trait works as-is without changing their types
- Services that dispatch on sender TID (e.g., input: sender=0 is kernel, sender>0 is cell) can encode this in their `Request` enum or check `sender_tid` in `handle()` — no special handling needed
- `sys_heartbeat` must be called inside the loop (not just once) — `heartbeat_ticks: Option<u64>` param lets services opt in without changing their logic
- Malformed messages (decode fails) are silently dropped — this matches current behavior and avoids panic on garbage from a misbehaving cell
- `run_service` returns `!` because services never stop unless killed

## Requirements

- `trait MessageHandler` with `type Request`, `type Response`, `fn handle(&mut self, req, sender_tid) -> Response`
- `fn run_service<H: MessageHandler>(handler: &mut H, heartbeat_ticks: Option<u64>) -> !`
- Silent drop on decode failure (log via `sys_log` in debug mode)
- Heartbeat called once per loop iteration when `heartbeat_ticks.is_some()`
- `buf` zeroed after each message (matches current behavior — prevents stale bytes leaking into next decode)

## Architecture

```
libs/ostd/src/dispatch.rs  (new)

use serde::{Deserialize, Serialize};
use api::ipc::{IPC_BUF_SIZE, encode, decode};
use crate::syscall::{sys_recv, sys_send, sys_heartbeat, SyscallResult};
use crate::task::yield_now;

pub trait MessageHandler {
    type Request:  for<'de> Deserialize<'de>;
    type Response: Serialize;

    /// Called for each valid decoded message. Return the response to send back.
    fn handle(&mut self, req: Self::Request, sender_tid: usize) -> Self::Response;
}

pub fn run_service<H: MessageHandler>(handler: &mut H, heartbeat_ticks: Option<u64>) -> ! {
    let mut buf      = [0u8; IPC_BUF_SIZE];
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    loop {
        if let Some(ticks) = heartbeat_ticks {
            sys_heartbeat(ticks);
        }
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(sender) if sender > 0 => {
                if let Ok(req) = decode::<H::Request>(&buf) {
                    let resp = handler.handle(req, sender as usize);
                    if let Ok(enc) = encode(&resp, &mut resp_buf) {
                        sys_send(sender as usize, enc);
                    }
                }
                buf = [0u8; IPC_BUF_SIZE];
            }
            _ => yield_now(),
        }
    }
}
```

## Related Code Files

- **New**: `libs/ostd/src/dispatch.rs`
- **Modify**: `libs/ostd/src/lib.rs` — add `pub mod dispatch`

## Implementation Steps

1. Create `libs/ostd/src/dispatch.rs`
2. Define `MessageHandler` trait
3. Implement `run_service<H: MessageHandler>(handler, heartbeat_ticks) -> !`
4. Add `pub mod dispatch;` to `libs/ostd/src/lib.rs`
5. `cargo check`

## Todo List

- [ ] Create libs/ostd/src/dispatch.rs
- [ ] Define MessageHandler trait (Request + Response assoc types + handle fn)
- [ ] Implement run_service with heartbeat + recv loop + silent-drop on decode error
- [ ] pub mod dispatch in lib.rs
- [ ] cargo check clean

## Success Criteria

```rust
// Must compile:
impl MessageHandler for ConfigHandler {
    type Request = ConfigRequest;
    type Response = ConfigResponse;
    fn handle(&mut self, req: ConfigRequest, _sender: usize) -> ConfigResponse { ... }
}
run_service(&mut ConfigHandler::new(), Some(500)); // never returns
```

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Services with sender=0 kernel events (input service) | input service can keep its custom loop or check sender in handle(); trait does NOT break it |
| Net service uses `sys_try_recv` + `sys_wait_for_event` (not blocking recv) | Net service keeps its custom loop — trait is opt-in, not mandatory migration |
| `H::Response: Serialize` bound — what if handler wants fire-and-forget (no reply)? | Add `Option<H::Response>` variant later if needed; v1 always replies |

## Security Considerations

Silent decode drop prevents a malformed IPC from panicking a service. The sender must already have send permission (IPC is capability-checked by the kernel). No new attack surface.
