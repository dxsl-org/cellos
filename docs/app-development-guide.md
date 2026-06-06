# ViCell App Development Guide

> How to write, build, run, and test a **Cell application** for ViCell.
> For setup/build of the whole OS see [getting-started.md](getting-started.md);
> for the raw syscall ABI see [api-reference.md](api-reference.md).

**Version**: v0.2.1-dev | **Last updated**: 2026-06-06

---

## 1. What is a Cell app?

A ViCell application is a **Cell**, not a Unix process. Cells share one address
space (SAS) and are isolated by Rust's type system (LBI), not by an MMU. In
practice that changes four things for app authors — the relevant [Coding Laws](code-standards.md):

| Law | What it means for your app |
|-----|----------------------------|
| **4 — No unsafe in Cells** | App crates are `unsafe`-free. Only the `#[no_mangle]` on `main` forces an exception (see §2). |
| **2 — Owned buffers for async** | Pass `Box<[u8]>`/`Vec<u8>` across `async` boundaries, never `&mut [u8]`. |
| **8 — Implement Drop** | There is no process teardown — release resources (handles, caps) in `Drop`. |
| **Manifest** | Privileged capabilities (disk, network, spawn) are declared, not assumed (see §4). |

---

## 2. Hello, Cell — the minimal app

The smallest possible app ([cells/apps/hello/src/main.rs](../cells/apps/hello/src/main.rs)):

```rust
#![no_std]
#![no_main]

extern crate ostd;

#[no_mangle]
pub fn main() {
    ostd::io::println("Hello from a separate ELF!");
    ostd::syscall::sys_exit(0);
}
```

- `#![no_std]` / `#![no_main]` — Cells are bare-metal; there is no std and no
  Rust runtime `main`.
- `#[no_mangle] pub fn main()` — the ELF loader looks up the symbol `main`.
  Because `#[no_mangle]` trips the `unsafe_attr` lint, a binary crate **cannot**
  use `#![forbid(unsafe_code)]` at the crate root. Keep all real logic in
  submodules and forbid unsafe there, or simply write no `unsafe` (the compiler
  still rejects it elsewhere).
- `sys_exit(0)` — terminate the Cell. If `main` returns without it, the loader
  exits the Cell for you, but calling it explicitly is clearer.

---

## 3. Cargo.toml

```toml
[package]
name = "app-hello"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "hello"          # the binary/ELF name → /bin/hello
path = "src/main.rs"

[dependencies]
types = { path = "../../../libs/types" }   # VAddr, PAddr, ViError, CellId
api   = { path = "../../../libs/api" }      # IPC enums, manifest macro, syscall ids
ostd  = { path = "../../../libs/ostd" }     # std-lib for Cells (io, syscall, alloc)

# Optional: feature-gate test-only behaviour (see vfs/Cargo.toml for an example)
# [features]
# test-hooks = []
```

To use `Box`/`String`/`Vec`, add `extern crate alloc;` to `main.rs` — `ostd`
provides the global allocator.

### The ostd prelude

`use ostd::prelude::*;` brings in the essentials:

```rust
print, println          // serial console output
Mutex                   // interrupt-safe spinlock
Result, ViError, ViResult
Box, String, ToString, Vec
```

Other useful modules: `ostd::syscall` (all syscalls), `ostd::io`, `ostd::task`
(`yield_now`), `ostd::fast_ipc`.

---

## 4. Capabilities & the manifest

Privileged syscalls (block I/O, network, spawning Cells) are gated by
**capability tokens** the kernel grants at spawn time, driven by an ELF manifest.
Declare what your app needs:

```rust
// Embeds an 8-byte record into the __ViCell_manifest ELF section.
api::declare_manifest!(block_io = false, network = true, spawn = false);
```

- `network = true` → the kernel grants `NetworkCap`; `NetTx`/`NetRx` (and the net
  service) work. Without it those calls are rejected.
- `block_io = true` → `BlockIoCap` (raw disk). Normal apps use the VFS service
  instead and leave this `false`.
- `spawn = true` → `SpawnCap` (spawn/hot-swap other Cells). Reserved for
  init/shell-class apps.

Privileged caps are only honoured for binaries under `/bin/` (see [Phase 30](project-roadmap.md)).
Most apps declare everything `false` and talk to services over IPC.

---

## 5. Talking to services (IPC)

Apps don't touch hardware — they message **service Cells** over IPC. Each service
listens on a well-known endpoint (task id).

> ⚠️ **Endpoint ids are currently fixed by spawn order**, not discovery:
> `init=1, hello=2, vfs=3, … net=6, compositor=7, shell=8`. This is fragile and
> will be replaced by a name-service; hard-code with care and centralise the
> constant.

### VFS — typed IPC (postcard)

The filesystem service uses typed request/response enums in
[libs/api/src/ipc.rs](../libs/api/src/ipc.rs). Encode → send → recv → decode:

```rust
const VFS: usize = 3;

fn vfs(req: &api::ipc::VfsRequest) -> api::ipc::VfsResponse<'static> {
    let mut tx = [0u8; 512];
    let n = api::ipc::encode(req, &mut tx).map(|s| s.len()).unwrap_or(0);
    ostd::syscall::sys_send(VFS, &tx[..n]);
    // Leak the rx buffer so VfsResponse::Data can borrow it for 'static.
    let rx: &'static mut [u8; 512] =
        alloc::boxed::Box::leak(alloc::boxed::Box::new([0u8; 512]));
    match ostd::syscall::sys_recv(0, rx) {
        ostd::syscall::SyscallResult::Ok(_) =>
            api::ipc::decode::<api::ipc::VfsResponse>(rx)
                .unwrap_or(api::ipc::VfsResponse::Err(0xFE)),
        _ => api::ipc::VfsResponse::Err(0xFD),
    }
}
```

Common requests: `Write{path,content}`, `Append{path,content}`, `Stat(path)`,
`ReadAsync{path}` → `PendingHandle(h)`, `Poll{handle}` → `Data(&[u8])`, `Mkdir`,
`Unlink`, `ListDir`. Full protocol: [vfs-api.md](vfs-api.md). A complete client
lives in [cells/apps/vfs-test/src/main.rs](../cells/apps/vfs-test/src/main.rs).

### Net — raw opcodes

The network service uses a raw frame `[opcode:1][cap:8 LE][payload]` to
`NET_ENDPOINT = 6`:

```rust
const NET: usize = 6;
const SOCKET_TCP: u8 = 0x10;

// Create a TCP socket → reply is an 8-byte CapId.
ostd::syscall::sys_send(NET, &[SOCKET_TCP, 0,0,0,0,0,0,0,0]);
let mut cap = [0u8; 8];
let _ = ostd::syscall::sys_recv(0, &mut cap);
let cap_id = u64::from_le_bytes(cap);
```

Opcodes: `SOCKET_TCP 0x10`, `SOCKET_UDP 0x11`, `CONNECT 0x12`, `SEND 0x13`,
`RECV 0x14`, `CLOSE 0x15`, `BIND 0x16`, `LISTEN 0x17`, `ACCEPT 0x18`,
`SENDTO 0x21`, `RECVFROM 0x22`, `JOIN_MULTICAST 0x23`. Full protocol:
[network-api.md](network-api.md). Real clients: `cells/apps/net-tools/src/bin/{nc,curl,mqtt}.rs`.

---

## 6. Running your app

Two ways a Cell binary reaches the running system:

**A. Disk-loaded (most apps).** The binary lives under `/bin/` on the VirtIO
disk and is launched with `SpawnFromPath`. Changing it only rebuilds the cell —
**no kernel rebuild**. This is how `net`, `compositor`, and the utilities load.

**B. Embedded in the kernel.** Only `init`, `shell`, `vfs`, `config`, `lua` are
baked into the kernel image via `include_bytes!`. To change one you must rebuild
and re-embed:

```powershell
./scripts/update-embedded.ps1        # builds release cells → kernel/src/embedded/
cargo build --release -p vicell-kernel
```

Build any cell for the target:

```bash
cargo build --target riscv64gc-unknown-none-elf [--release] -p app-hello
```

Then from the shell: `spawn /bin/hello` (or just `hello` if it is on the path).
See [run.ps1](../run.ps1) for the QEMU invocation.

---

## 7. Testing your app

- **In-ViCell test cell** — spawn-and-assert, prints `[PASS]/[FAIL]`, exits 0/1.
  Pattern: [cells/apps/vfs-test/src/main.rs](../cells/apps/vfs-test/src/main.rs).
- **Integration harness** — boots QEMU, drives the shell over serial, asserts on
  output. Pattern: [tests/integration/tests/boot.rs](../tests/integration/tests/boot.rs).
- Always verify functionally: build → boot → run. A cell that compiles is not a
  cell that works (see [10-testing.md](specs/10-testing.md)).

---

## 8. Gotchas & limits

- **IPC buffer is 512 bytes.** A single `Write`/`SENDTO` payload is capped near
  ~480 bytes after the envelope. Chunk larger transfers.
- **`sys_recv` returns the *sender id*, not a byte count.** Recover the payload
  length by scanning for the last non-zero byte, then apply per-message minimum
  floors (see the net service `main.rs` for the canonical pattern). A payload
  ending in `0x00` is the known edge case.
- **Endpoint ids are spawn-order constants** (§5) — fragile until a name service
  lands. Centralise them.
- **`#[no_mangle]` blocks `#![forbid(unsafe_code)]`** at the crate root (§2).
- **Owned buffers across `async`** (Law 2); **`Drop` for every handle/cap** (Law 8).
- **No blocking forever without yielding.** Use `ostd::task::yield_now()` /
  `sys_try_recv` in poll loops so other Cells make progress.

---

## 9. Worked example A — `greet-file` (VFS round-trip)

A minimal app: write a greeting to RamFS, read it back, print it.

`cells/apps/greet-file/Cargo.toml`:
```toml
[package]
name = "app-greet-file"
version = "0.1.0"
edition = "2021"
[[bin]]
name = "greet-file"
path = "src/main.rs"
[dependencies]
types = { path = "../../../libs/types" }
api   = { path = "../../../libs/api" }
ostd  = { path = "../../../libs/ostd" }
```

`src/main.rs`:
```rust
#![no_std]
#![no_main]
extern crate alloc;
extern crate ostd;

use ostd::io::println;
use ostd::syscall::{sys_send, sys_recv, sys_exit, SyscallResult};

const VFS: usize = 3;

// No privileged caps: VFS access goes through IPC, not BlockIoCap.
api::declare_manifest!(block_io = false, network = false, spawn = false);

fn vfs(req: &api::ipc::VfsRequest) -> api::ipc::VfsResponse<'static> {
    let mut tx = [0u8; 512];
    let n = api::ipc::encode(req, &mut tx).map(|s| s.len()).unwrap_or(0);
    sys_send(VFS, &tx[..n]);
    let rx: &'static mut [u8; 512] =
        alloc::boxed::Box::leak(alloc::boxed::Box::new([0u8; 512]));
    match sys_recv(0, rx) {
        SyscallResult::Ok(_) =>
            api::ipc::decode::<api::ipc::VfsResponse>(rx)
                .unwrap_or(api::ipc::VfsResponse::Err(0xFE)),
        _ => api::ipc::VfsResponse::Err(0xFD),
    }
}

#[no_mangle]
pub fn main() {
    // 1. Write a greeting to RamFS (/tmp is volatile, needs no disk).
    match vfs(&api::ipc::VfsRequest::Write {
        path: "/tmp/greeting.txt",
        content: b"Hello, ViCell!",
    }) {
        api::ipc::VfsResponse::Ok => println("[greet] wrote /tmp/greeting.txt"),
        _ => { println("[greet] write failed"); sys_exit(1); }
    }

    // 2. Read it back with the async two-step (ReadAsync → Poll).
    let handle = match vfs(&api::ipc::VfsRequest::ReadAsync { path: "/tmp/greeting.txt" }) {
        api::ipc::VfsResponse::PendingHandle(h) => h,
        _ => { println("[greet] read request failed"); sys_exit(1); }
    };
    match vfs(&api::ipc::VfsRequest::Poll { handle }) {
        api::ipc::VfsResponse::Data(bytes) => {
            // bytes is a safe copy in the reply buffer — no unsafe deref.
            if let Ok(s) = core::str::from_utf8(bytes) {
                println("[greet] read back:");
                println(s);
            }
        }
        _ => println("[greet] poll returned no data"),
    }
    sys_exit(0);
}
```

Build + run: `cargo build --target riscv64gc-unknown-none-elf -p app-greet-file`,
place under `/bin`, then `spawn /bin/greet-file`.

---

## 10. Worked example B — `mqtt-logger` (net + VFS)

An embedded-style app: subscribe to an MQTT topic and append each payload to a
log file. This is the *software half* of a robot telemetry node.

`Cargo.toml` is the same as §9 but declares the network capability:
```toml
# main.rs:
api::declare_manifest!(block_io = false, network = true, spawn = false);
```

Structure (full MQTT 3.1.1 framing is non-trivial — reuse it from
[cells/apps/net-tools/src/bin/mqtt.rs](../cells/apps/net-tools/src/bin/mqtt.rs)
rather than re-implementing):

```rust
const NET: usize = 6;
const VFS: usize = 3;

#[no_mangle]
pub fn main() {
    // 1. TCP-connect to the broker via the net service (SOCKET_TCP → CONNECT),
    //    then send MQTT CONNECT + SUBSCRIBE. See mqtt.rs for the exact packets.
    let cap_id = net_tcp_connect(broker_addr, 1883); // helper mirroring mqtt.rs

    // 2. Receive loop: each RECV (0x14) returns published payload bytes.
    loop {
        let payload = net_recv(cap_id);          // [] when nothing is ready
        if payload.is_empty() {
            ostd::task::yield_now();             // cooperate, don't spin hot
            continue;
        }
        // 3. Append the payload to /data/mqtt.log via the VFS service.
        //    Append auto-creates the file on first use; persists to FAT16.
        let _ = vfs(&api::ipc::VfsRequest::Append {
            path: "/data/mqtt.log",
            content: payload,                    // ≤ ~480 B per message (§8)
        });
    }
}
```

Key points this example teaches:
- **Capability**: `network = true` in the manifest is required, or `RECV` is rejected.
- **Two services, one app**: net (raw opcodes) + VFS (typed IPC) compose freely.
- **Backpressure**: `yield_now()` on empty receive keeps the scheduler fair.
- **Persistence**: `/data/` is FAT16 on the VirtIO disk; `/tmp/` is volatile RamFS.

For a fully working MQTT client, study `mqtt.rs`; for the persistence side, study
`vfs-test`. Together they cover everything `mqtt-logger` needs.

---

## See also

- [getting-started.md](getting-started.md) — OS setup, build, first contribution
- [api-reference.md](api-reference.md) — syscall ABI + trait definitions
- [vfs-api.md](vfs-api.md) · [network-api.md](network-api.md) · [input-api.md](input-api.md) · [display-api.md](display-api.md)
- [scripting-guide.md](scripting-guide.md) — if a Lua/Python script fits better than a native Cell
- [code-standards.md](code-standards.md) — the 8 Coding Laws in full
