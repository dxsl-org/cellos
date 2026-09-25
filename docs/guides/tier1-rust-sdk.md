# Tier 1 Rust + SDK Service Clients — Apps with Services

> VFS, network, and IPC clients built into AppContext. For most user apps.
> The SDK is not split into numbered tiers; this guide covers SDK service-client
> modules for trusted Tier 1 native Cells and future Tier 2 native Cells.

---

## AppContext: The Entry Point

Instead of raw syscalls, use the context:

```rust
#![no_std]
#![no_main]

extern crate alloc;

use ostd::app::{AppContext, AppEvent};
use ostd::io::println;

ostd::app_entry!(handler = app_main);

fn app_main(ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // ctx.vfs(), ctx.net(), ctx.send() all available here
            match ctx.vfs().stat("/") {
                Ok((size, is_dir)) => {
                    println(&alloc::format!("Root: size={} is_dir={}", size, is_dir));
                }
                Err(_) => println("VFS unavailable"),
            }
        }
        AppEvent::Message { sender_tid, data } => {
            ctx.send_msg(sender_tid, b"pong").ok();
        }
        _ => {}
    }
}
```

---

## VfsClient API

Lazy-initialized on first `ctx.vfs()` call. Implements the common filesystem operations:

```rust
// Read entire file
ctx.vfs().read_file("/path/to/file")?
    // → Result<Vec<u8>, ViError>

// Write entire file (creates or truncates)
ctx.vfs().write_file("/path/to/file", b"content")?
    // → Result<(), ViError>

// Stat: (size, is_dir)
let (size, is_dir) = ctx.vfs().stat("/path")?
    // → Result<(usize, bool), ViError>

// List directory
ctx.vfs().list_dir("/path")?
    // → Result<Vec<String>, ViError>

// Delete (unlink)
ctx.vfs().unlink("/path")?
    // → Result<(), ViError>

// Mkdir
ctx.vfs().mkdir("/path")?
    // → Result<(), ViError>
```

All operations are **synchronous**; the VFS service handles buffering and caching.

---

## NetClient API

Network stack exposes a **TcpStream** implementing `embedded_io::Read` + `Write`:

```rust
use embedded_io::{Read, Write};

let mut stream = ctx.net().tcp_connect(&[10, 0, 2, 2], 8080)?;
    // → Result<TcpStream, ViError>

// Write (implements embedded_io::Write)
stream.write_all(b"GET / HTTP/1.1\r\n")?;

// Read (implements embedded_io::Read)
let mut buf = [0u8; 256];
let n = stream.read(&mut buf)?;
    // → Result<usize, ViError>

let response = &buf[..n];
```

The `TcpStream` is dropped automatically (socket close on Drop).

---

## IPC & Message Sending

Send App SDK–wrapped messages:

```rust
// Send typed message to another Cell (by TID)
let remote_tid = 5usize;
ctx.send_msg(remote_tid, b"hello")?;
    // Wraps [0xAC, 0x00, b"hello"] for AppContext on the other end

// Send raw bytes (legacy)
ctx.send(remote_tid, &my_bytes)?;

// Receive (handled by app_entry! loop — you get AppEvent::Message)
```

---

## Service Discovery

Look up well-known services by ID:

```rust
use api::service;

let vfs_tid = ctx.lookup_service(service::VFS)?
    .ok_or(ViError::IO)?;
    // → Option<usize>

let net_tid = ctx.lookup_service(service::NET)?
    .ok_or(ViError::IO)?;
```

Service IDs are defined in `libs/api/src/service.rs`. Modern code should use `ctx.vfs()` and `ctx.net()` instead of manual lookup.

---

## Manifest & Syscalls

Declare what you need:

```rust
api::declare_manifest!(
    block_io = false,   // false — use VFS instead
    network = true,     // true if you use ctx.net()
    spawn = false,      // leave false unless you're init/shell
    gpio = false
);

api::declare_syscalls![Send, Recv, Log, Exit, LookupService];
```

---

## Input Events (Optional)

For UI apps:

```rust
// At startup, request input focus
ctx.request_input_focus();

// In your event loop, handle:
AppEvent::Input(input_event) => {
    // input_event is an api::input::InputEvent
    // (keyboard key / mouse move / button)
    match input_event {
        api::input::InputEvent::Key { key, pressed } => { /* ... */ }
        api::input::InputEvent::Motion { x, y } => { /* ... */ }
        api::input::InputEvent::Button { button, pressed } => { /* ... */ }
        _ => {}
    }
}
```

Alternatively, use **[Tier 1 + ViUI](viui-guide.md)** for a higher-level UI framework.

---

## Timeout & Heartbeat

Run the loop with a deadline:

```rust
// Timeout (fires AppEvent::Timeout every N ticks; 1 tick ≈ 10 ms)
ctx.run_with_timeout(1000, |ctx, event| {
    match event {
        AppEvent::Timeout => {
            // Do periodic work here
        }
        _ => {}
    }
});

// Or: arm the watchdog heartbeat
ctx.arm_heartbeat(1000);  // kernel kills us if we don't call any syscall in 1000 ticks
```

---

## Canonical Example

See [cells/demos/sdk-demo/src/main.rs](../../cells/demos/sdk-demo/src/main.rs) — 64 lines. It demonstrates VFS stat, message echo, and graceful shutdown.

---

## When to Use Tier 1 + SDK Service Clients

✅ Reading/writing files (VFS)
✅ Network apps (TCP, HTTP)
✅ Talking to other Cells (IPC)
✅ Most user applications

❌ Complex UIs → use Tier 1 + ViUI
❌ Protected relay keys → use the purpose-specific KMS client; Silo is not a public API
❌ C/C++ interop → use Tier 1 `ffi-posix` profile

---

## Supervisor Trees (actors)

An app can declare its own supervision tree instead of relying on `/bin/init`
([ADR-0021](../decisions/0021-actor-supervisor-library-in-userspace.md)). The library is userspace-only:
no new syscall, opcode, or message byte.

```rust
use ostd::actor::{self, Actor, ActorCtx, Backoff, ChildSpec, Policy, Strategy, Tree};
use ostd::app::AppEvent;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
enum Msg { Ping { seq: u32 } }
#[derive(Serialize, Deserialize, Debug)]
enum Reply { Pong { seq: u32 } }

/// An ordinary child: answer typed calls.
struct Worker;
impl Actor for Worker {
    type Msg = Msg;
    fn on_message(&mut self, ctx: &mut ActorCtx<'_>, from: usize, msg: Msg) {
        match msg {
            Msg::Ping { seq } => { let _ = ctx.reply(from, &Reply::Pong { seq }); }
        }
    }
}

/// A supervisor: one `one_for_one` tree over three children.
struct Boss { tree: Tree }
impl Actor for Boss {
    type Msg = Msg;                                   // what this actor receives
    fn on_start(&mut self, ctx: &mut ActorCtx<'_>) { self.tree.start_all(ctx); }
    fn on_message(&mut self, _ctx: &mut ActorCtx<'_>, _from: usize, _msg: Msg) {}
    fn on_event(&mut self, ctx: &mut ActorCtx<'_>, ev: AppEvent) {
        // A watched child's death arrives as a raw message: tid + 8-byte reason.
        if let AppEvent::RawMessage { sender_tid, data } = ev {
            if let Some(reason) = actor::exit_reason(&data) {
                self.tree.handle_exit(ctx, sender_tid, reason);
            }
        }
    }
    fn on_tick(&mut self, ctx: &mut ActorCtx<'_>) {
        let now = ctx.now_ticks();
        self.tree.handle_tick(ctx, now);               // fires backoff timers
    }
}

// Declare the tree; `run` owns the mailbox loop and never returns.
let tree = Tree::new(Strategy::OneForOne, [
    ChildSpec::new("w0", "/bin/my-worker").with_policy(Policy::Transient)
        .with_backoff(Backoff { base_ticks: 50, cap_ticks: 200 }),
    ChildSpec::new("w1", "/bin/my-worker").with_policy(Policy::Transient),
]);
actor::run(Boss { tree });
```

| Concept | Values / default |
|---|---|
| `Policy` | `Permanent` (always), `Transient` (only abnormal exit — non-zero reason), `Temporary` (never) |
| `ChildSpec::intensity` / `window_ticks` | ≤5 restarts per 1 000 ticks (~10 s); the sixth abnormal exit inside the window **gives up on that child only** and logs it |
| `Backoff` | `base_ticks << (consecutive failures - 1)`, capped by `cap_ticks`; `Backoff::NONE` respawns immediately |
| `Strategy` | `OneForOne`, `OneForAll`, `RestForOne` (expansion is in child declaration order) |
| Deadlines | `on_tick` every `actor::ACTOR_TICK_TICKS` (5 ticks ≈ 50 ms) |

Rules that come from the IPC contract (Spec 17) and are enforced by the library:

- actor messages ride the existing `0xAC 0x00` envelope, so `Shutdown`, `CapRevoked`, and hot-swap
  events still arrive on the same mailbox (§3);
- `ActorCtx::call` recvs **masked to the peer tid** (§2) and a reply is a **blocking** send, so the
  peer must be waiting — do not answer a caller that may have timed out with a bare `sys_send`;
- an undecodable message is logged with its sender and length, never dropped silently (§7);
- there is no fire-and-forget send and no per-actor mailbox: the kernel mailbox stays bounded and
  backpressured (§6).

**Placement matters for a supervisor.** A supervisor holds `SpawnCap` (declare
`spawn = true` in its manifest), and the kernel refuses a non-empty child ceiling on the
caller-supplied-bytes (`SpawnFromElf`) route — so an authority-bearing cell must be staged in
**VIFS1**, not only in the disk cell-store (`gen_disk.ps1` does this for the B0 witness). Children
are usually capability-free and are reached through VFS from the ordinary cell-store, which is what
`sys_spawn_from_path` does automatically.

Working example: `cells/tests/backend` (supervisor + worker) driven by
`scripts/qemu-actor-supervisor.sh`.

---

## Common Errors

**VFS not registered?** —  The service may not be running. Check kernel boot output and catch `Err(_)` gracefully.

**Network port refused?** — Network service or target unreachable. Use `Result::ok()` to ignore.

**Message send fails?** — Remote Cell dead or not receiving. Use `Result::ok()` to drop silently.

---

## Next Steps

- Building a UI? → [Tier 1 + ViUI](viui-guide.md)
- Need the development custody boundary? → [KMS-mediated Silo `DEV_REFERENCE`](tier1-silo.md)
- Have existing C code? → [Tier 1 `ffi-posix` profile](tier1b-c-zig.md)
- See [api-reference.md](../api-reference.md) for syscall details.
