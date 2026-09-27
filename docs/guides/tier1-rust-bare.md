# Tier 1 `rust-no-std` Profile — Minimal Cell Apps

> Direct syscall access, no SDK abstractions. For apps that are pure compute or need fine-grained control.

---

## Entry Point: `ostd::app_entry!`

> Gate `no_std`/`no_main` on the target (`#![cfg_attr(target_os = "none", …)]`). `ostd`'s entry
> macros already gate their `#[no_mangle]` the same way, so a hosted test build keeps `std` and
> gets libtest's entry; leaving them ungated hands the C runtime the cell's own `main` and the
> harness hangs before it lists a test. Guest output is unchanged.

Instead of `#[no_mangle] fn main()`, use the zero-boilerplate macro:

```rust
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use ostd::app::{AppContext, AppEvent};
use ostd::io::println;

ostd::app_entry!(handler = my_handler);

fn my_handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => println("Hello from Cellos!"),
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
            ostd::syscall::sys_exit(0);
        }
        _ => {}
    }
}
```

The macro generates:
- Manifest declaration (empty/default capabilities)
- Syscall allowlist (Init, Shutdown, Log, Exit only)
- `#[no_mangle] pub fn main()` wrapper
- `unsafe` isolation (you're safe)

---

## AppEvent Variants

| Event | When | Handler Duty |
|-------|------|--------------|
| `Init` | Before first `sys_recv` (if using `app_entry!` + `run_with_lifecycle`) | Startup: config, logging setup, resource init. |
| `Message { sender_tid, data }` | Typed IPC message arrived (envelope starts `0xAC`) | Echo, relay, or dispatch. |
| `RawMessage { sender_tid, data }` | Raw `sys_send` (legacy/non-SDK senders) | Ignore or handle specially. |
| `Input(InputEvent)` | Keyboard/mouse input (only if `request_input_focus()` called) | UI apps only. |
| `Timeout` | Receive deadline elapsed (only `run_with_timeout()`) | Periodic tasks, watchdog. |
| `Shutdown` | Kernel graceful shutdown (no reason) | Exit cleanly. |
| `ShutdownWith { reason }` | Kernel shutdown with reason (Watchdog / ParentDied / Requested) | Restart vs. abort logic. |

Always add `_ => {}` wildcard to future-proof your code.

---

## Manifest & Syscall Allowlist

Declare capabilities and permitted syscalls:

```rust
api::declare_manifest!(
    block_io = false,   // raw disk I/O
    network = false,    // network access
    spawn = false,      // spawn other Cells
    gpio = false,       // GPIO peripherals
    uart = false,       // UART serial
    hypervisor = false  // create VMs (Tier 3 linux-guest, G2+)
);

api::declare_syscalls![Send, Recv, Log, Exit, GetTime];
```

**Capabilities** are kernel grants (honored only for `/bin/*` binaries). **Syscalls** are the thin whitelist the kernel enforces. Omit what you don't use.

The Rust macro emits the exact 16-byte Manifest-v2 ABI. Its ABI-stable `tier`
byte is a PKU **protection class**, not this guide's application execution tier;
new code should use `protection_class` terminology. Legacy Zig Cells continue
to emit exact 8-byte v1 records, which the Rust loader upcasts without changing
their behavior.

Inspect a built Cell without modifying it:

```bash
python3 tools/check_elf.py path/to/cell.elf
```

Read `Execution tier`, `Runtime profile`, `Protection class`, `Capabilities`,
and `Evidence` as separate claims. The inspector does not prove a signature or
runtime measurement. At load time, only a structurally valid ELF with no
manifest selects explicit legacy path policy; a malformed or duplicate
manifest is denied before task creation.

---

## Syscall Allowlist

Common syscalls:
- `Init` — initialization (implicit in app_entry!)
- `Send`, `Recv` — IPC
- `Log` — `println!`
- `Exit` — `sys_exit()`
- `GetTime` — `sys_get_time()`
- `Heartbeat` — `sys_heartbeat()` (watchdog)
- `LookupService` — `sys_lookup_service()`
- `GetRandom` — entropy

See [api-reference.md](../api-reference.md) for the full list.

---

## Minimal Example

```rust
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use ostd::io::println;

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Log, Exit];

ostd::app_entry!(handler = main_handler);

fn main_handler(_ctx: &mut AppContext, event: ostd::app::AppEvent) {
    match event {
        ostd::app::AppEvent::Init => {
            println("Cell started");
        }
        _ => {}
    }
}
```

That's it. No IPC, no service clients, no async — just init and exit.

---

## When to Use Tier 1 `rust-no-std`

✅ Numeric computation, data processing
✅ Pure-Rust no external I/O
✅ Extreme performance requirements
✅ Learning syscalls directly

❌ Reading files → use Tier 1 + SDK service clients (VFS client)
❌ Talking to other Cells → use Tier 1 + SDK service clients (IPC wrappers)
❌ Building UIs → use Tier 1 + ViUI

---

## Canonical Example

See [cells/demos/hello-cell/src/main.rs](../../cells/demos/hello-cell/src/main.rs) — 18 lines total.

---

## Host-testing a cell (why CI runs your `#[test]`s)

A cell's tests run in the host `x86_64-unknown-linux-gnu` harness, not in the guest. The
five things that make that work — the same recipe applied five times in 2026-09:

1. **Gate `no_std` / `no_main` / entry / heap by target.** `#![cfg_attr(target_os = "none", …)]`
   for the first two, and note every `ostd` macro that emits `#[no_mangle]` (`cell_main!`,
   `run_app!`, `app_entry!`, `service_entry!`) already gates it. Ungated, the C runtime calls
   the cell's entry in the test binary and the harness runs your app and hangs before it lists
   a test.
2. **A `staticlib` dependency needs `std` on the host** — gate its `no_std` by target too.
3. **`ostd::heap` exports exist only on the target** — a test that reaches them needs the same gate.
4. **Target-only features gate their own handler** (`ai_sdk::ostd_transport` is the example):
   gate the handler and the route branch, not the whole module.
5. **Then fix what the tests find.** Re-enabling a suite surfaces drift — missing imports and
   changed signatures in test modules — and that drift is the part worth the exercise.

Wire the suite into the host-unit job in `.github/workflows/ci.yml` when it is green; the boot
suite is a separate, **allowlisted** job (a new test stays excluded until it passes 2/2
consecutive full runs — see the comment on `boot-suite`).

---

## Next Steps

- Need services (VFS, network)? → [Tier 1 + SDK service clients](tier1-rust-sdk.md)
- Need a UI? → [Tier 1 + ViUI](viui-guide.md)
- Need C interop? → [Tier 1 `ffi-posix` profile](tier1b-c-zig.md)
