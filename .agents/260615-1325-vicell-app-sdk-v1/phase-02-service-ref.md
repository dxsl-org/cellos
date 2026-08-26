# Phase 02 — ServiceRef: Typed Service Discovery Handle

**Status**: 📋 Planned  
**Priority**: P1  
**Estimate**: ~1 day  
**Parallel**: runs alongside Phase 03 (different file — no conflict)  
**Depends on**: Phase 01 (for prelude, but core impl has no embedded-io deps — can start immediately)

## Context Links

- Codebase: [libs/ostd/src/syscall.rs](../../libs/ostd/src/syscall.rs) · [libs/api/src/syscall.rs](../../libs/api/src/syscall.rs) · [cells/apps/shell/src/config_client.rs](../../cells/apps/shell/src/config_client.rs)
- Pattern: `ConfigClient::endpoint()` retry loop is the reference

## Overview

Every cell that talks to a service reimplements this 8-line retry loop:

```rust
// Current boilerplate — repeated in every cell
fn vfs_tid() -> usize {
    loop {
        if let Some(tid) = ostd::syscall::sys_lookup_service(service::VFS) {
            return tid;
        }
        ostd::task::yield_now();
    }
}
```

`ServiceRef<const ID: u16>` replaces this with a typed, caching handle:

```rust
// After SDK
ctx.vfs.call(&VfsRequest::Read { ... })?
```

## Key Insights

- `sys_lookup_service(206)` is already open to all cells (allowlist bit 37)
- Cache is `Option<usize>` — valid for single-task cells (standard pattern). `&mut self` enforces exclusive access, no `UnsafeCell` needed
- On send failure (service restarted), callers call `invalidate()` to force re-resolve on next `call()`
- `call<Req, Resp>()` encapsulates encode → send → recv → decode, eliminating the four-step dance in every client
- Stack-allocates two `[u8; IPC_BUF_SIZE]` buffers (4 KiB × 2 = 8 KiB on stack) — acceptable per RISC-V/ARM64 default stack sizes (≥ 64 KiB per cell)

## Requirements

- `ServiceRef<const ID: u16>` struct with `resolve(&mut self) -> Option<usize>` + `invalidate(&mut self)`
- `call<Req: Serialize, Resp: DeserializeOwned>(&mut self, req: &Req) -> ViResult<Resp>` — one-shot request-reply
- On send error, auto-invalidate cache and return `Err(ViError::IO)` (caller can retry)
- No `unsafe` code
- Module `ostd::service` with constants `VFS`, `NET`, `INPUT`, `CONFIG`, `COMPOSITOR` of the right type

## Architecture

```
libs/ostd/src/service.rs  (new)

pub struct ServiceRef<const ID: u16> {
    cached_tid: Option<usize>,
}

impl<const ID: u16> ServiceRef<ID> {
    pub const fn new() -> Self
    pub fn resolve(&mut self) -> Option<usize>   // retries 8×, caches on success
    pub fn invalidate(&mut self)                  // clears cache (call after send failure)
    pub fn call<Req, Resp>(&mut self, req: &Req) -> ViResult<Resp>
      where Req: serde::Serialize,
            Resp: for<'de> serde::Deserialize<'de>
}

// Convenience type aliases
pub type VfsRef     = ServiceRef<{api::syscall::service::VFS}>;
pub type NetRef     = ServiceRef<{api::syscall::service::NET}>;
pub type InputRef   = ServiceRef<{api::syscall::service::INPUT}>;
pub type ConfigRef  = ServiceRef<{api::syscall::service::CONFIG}>;
pub type CompositorRef = ServiceRef<{api::syscall::service::COMPOSITOR}>;
```

`call()` implementation:
```
1. resolve() — returns Err(NotFound) if all retries fail
2. encode req into stack buf via api::ipc::encode
3. sys_send(tid, encoded) → on SyscallError, invalidate + return Err(IO)
4. sys_recv(0, resp_buf) → on failure, return Err(IO)
5. api::ipc::decode::<Resp>(&resp_buf)
```

## Related Code Files

- **New**: `libs/ostd/src/service.rs`
- **Modify**: `libs/ostd/src/lib.rs` — add `pub mod service`
- **Modify**: `libs/ostd/src/prelude.rs` — re-export `ServiceRef` (optional — may be too specialised for prelude)

## Implementation Steps

1. Create `libs/ostd/src/service.rs`
2. Implement `ServiceRef<const ID: u16>` struct and methods
3. Add type aliases `VfsRef`, `NetRef`, `InputRef`, `ConfigRef`, `CompositorRef`
4. Add `pub mod service;` to `libs/ostd/src/lib.rs`
5. `cargo check`

## Todo List

- [ ] Create libs/ostd/src/service.rs
- [ ] Implement ServiceRef::new() const
- [ ] Implement ServiceRef::resolve() with 8× retry + yield
- [ ] Implement ServiceRef::invalidate()
- [ ] Implement ServiceRef::call<Req, Resp>() — encode/send/recv/decode
- [ ] Add type aliases (VfsRef, NetRef, etc.)
- [ ] pub mod service in lib.rs
- [ ] cargo check clean

## Success Criteria

```rust
// Must compile and work end-to-end:
let mut vfs: ServiceRef<{service::VFS}> = ServiceRef::new();
let resp: VfsResponse = vfs.call(&VfsRequest::Stat { path: "/etc/config" })?;
```

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `serde` traits not in scope in ostd | ostd re-exports `api::*` which re-exports serde; add explicit `use serde::{Serialize, Deserialize}` if needed |
| Const generic `{api::syscall::service::VFS}` requires Rust const-expr in position | Stable Rust supports const in const generics since 1.65; verify rustc version in workspace |
| Stack overflow from 8 KiB stack buffers in deeply nested calls | Document limit; cells have ≥64 KiB stack; no concern for normal usage |

## Security Considerations

None — uses existing syscall 206 which is already open to all cells. No new trust boundaries.
