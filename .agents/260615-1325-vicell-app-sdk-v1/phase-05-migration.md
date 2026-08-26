# Phase 05 — Cell Migration + Integration Test

**Status**: 📋 Planned  
**Priority**: P2  
**Estimate**: ~1 day  
**Depends on**: Phase 04 (AppContext complete)

## Context Links

- Codebase: [cells/apps/hello/src/main.rs](../../cells/apps/hello/src/main.rs) · [tests/integration/tests/boot.rs](../../tests/integration/tests/boot.rs) · [tests/integration/src/](../../tests/integration/src/)
- Pattern: existing integration test setup (QemuRunner) in boot.rs

## Overview

Phase 05 validates the entire SDK end-to-end with two deliverables:

1. **`sdk-demo` cell** — a new cell that exercises the full stack: `run_app` + `ServiceRef::call` to VFS + `MessageHandler` impl. Proves the SDK works in a real cell.
2. **`sdk_app_context` integration test** — boots the system, spawns `sdk-demo`, and verifies it exits cleanly with the expected log output.

Existing cells (`hello`, `vfs-test`, etc.) are NOT force-migrated — YAGNI. They're simpler without the SDK for trivial cases.

## Key Insights

- A new cell exercises the SDK more honestly than migrating `hello` (which is trivial and the SDK would add verbosity)
- sdk-demo should exercise the **non-trivial parts**: `ServiceRef::call` to VFS + a small `MessageHandler` for its own IPC endpoint
- Integration test reuses `QemuRunner::boot_riscv()` pattern from existing tests
- The test needs a recognizable log probe in sdk-demo so CI can grep for it
- sdk-demo does NOT need to be spawned by init's supervisor list — it can be a direct `spawn_from_path("/bin/sdk-demo")` from the integration test via shell command

## Requirements

- `cells/apps/sdk-demo/` — new cell with `Cargo.toml`, `.ld` linker script, `src/main.rs`
- sdk-demo flow:
  1. Resolves VFS via `ctx.vfs.resolve()`
  2. Calls `ctx.vfs.call(&VfsRequest::Stat { path: "/etc/config" })` or similar
  3. Prints `[sdk-demo] VFS stat ok` on success (CI grep target)
  4. Exits cleanly via `run_app` implicit exit
- Integration test `sdk_app_context` in `tests/integration/tests/boot.rs`:
  1. Boot system
  2. Wait for shell prompt
  3. Send `sdk-demo` command (or verify it's auto-spawned by init in a test config)
  4. Assert `[sdk-demo] VFS stat ok` appears in output

## Architecture

### sdk-demo cell structure

```
cells/apps/sdk-demo/
├── Cargo.toml          # name = "sdk-demo"; dep on ostd, api
├── sdk-demo.ld         # linker script (copy from hello.ld, adjust BASE)
└── src/
    └── main.rs
```

`src/main.rs`:
```rust
#![no_std]
#![no_main]
extern crate ostd;
use ostd::prelude::*;
use ostd::app::{run_app, AppContext};
use api::vfs::{VfsRequest, VfsResponse};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Send, Recv, LookupService];

#[no_mangle]
pub fn main() {
    run_app(|ctx: &mut AppContext| {
        match ctx.vfs.call::<VfsRequest, VfsResponse>(&VfsRequest::Stat { path: "/etc/config" }) {
            Ok(_) => ostd::io::println("[sdk-demo] VFS stat ok"),
            Err(e) => ostd::io::println("[sdk-demo] VFS stat err"),
        }
    });
}
```

### Workspace integration

Add `sdk-demo` to `Cargo.toml` workspace members and to `kernel/build.rs` (or equivalent) cell list so it's embedded in the disk image.

### Integration test

```rust
// tests/integration/tests/boot.rs
#[test]
fn sdk_app_context() {
    let mut qemu = QemuRunner::boot_riscv();
    qemu.wait_for("ViCell >", 30);
    qemu.send_line("sdk-demo");  // shell command runs the cell
    qemu.assert_output("[sdk-demo] VFS stat ok", 10);
}
```

## Related Code Files

- **New**: `cells/apps/sdk-demo/Cargo.toml`, `cells/apps/sdk-demo/sdk-demo.ld`, `cells/apps/sdk-demo/src/main.rs`
- **Modify**: root `Cargo.toml` — add `cells/apps/sdk-demo` to workspace
- **Modify**: disk image build script — add sdk-demo binary to `/bin/sdk-demo`
- **Modify**: `tests/integration/tests/boot.rs` — add `sdk_app_context` test

## Implementation Steps

1. Create `cells/apps/sdk-demo/` structure (Cargo.toml + linker script + main.rs)
2. Add to workspace Cargo.toml
3. Add to disk image build (check `build.rs` or `mkfat32_inplace.py` call site)
4. Write `sdk_app_context` integration test
5. Run `cargo check` on sdk-demo
6. Run integration test locally: confirm `[sdk-demo] VFS stat ok` in output
7. Verify no regressions in existing tests (`cargo test -p integration`)

## Todo List

- [ ] Create cells/apps/sdk-demo/Cargo.toml
- [ ] Create cells/apps/sdk-demo/sdk-demo.ld (linker script)
- [ ] Create cells/apps/sdk-demo/src/main.rs with run_app + ctx.vfs.call
- [ ] Add sdk-demo to workspace Cargo.toml
- [ ] Add sdk-demo to disk image build
- [ ] Write sdk_app_context integration test
- [ ] cargo check sdk-demo clean
- [ ] Integration test passes: [sdk-demo] VFS stat ok observed
- [ ] No regressions in existing boot.rs tests

## Success Criteria

1. `sdk-demo` cell builds without errors or warnings
2. CI test `sdk_app_context` passes (green)
3. All pre-existing integration tests still pass
4. The full SDK can be demonstrated in a single code snippet ≤ 15 LOC

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Linker script BASE address conflict with existing cells | Copy from `hello.ld`, use a unique BASE address (check `docs/syscall-allowlist-and-build-pitfalls.md` memory — cells need distinct bases in the linker map) |
| `VfsRequest::Stat` may not exist or have different field name | Check `libs/api/src/ipc.rs` for actual VfsRequest variants; adjust to a real op (e.g., `VfsRequest::Open`) |
| sdk-demo not in init's spawn list → only runnable via shell | Acceptable for v1 test — shell `sdk-demo` command works; add to init spawn list in a follow-up if needed |
| Integration test grep for `[sdk-demo] VFS stat ok` — formatting must match exactly | Hardcode the probe string in sdk-demo and the test; no runtime formatting |

## Security Considerations

sdk-demo declares minimal caps (`block_io=false, network=false, spawn=false`) — follows least-privilege manifest pattern. `LookupService` is in the allowlist (bit 37, open to all).
