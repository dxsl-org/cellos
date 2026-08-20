# Cellos App Development Guide

> How to choose a development model and write applications for Cellos.
> For syscall reference, see [api-reference.md](api-reference.md);
> for kernel internals, see [system-architecture.md](system-architecture.md).

**Version**: v0.4.0 | **Last updated**: 2026-08-19

---

## What is a Cell App?

A Cellos application runs as a **Cell**. For native apps, Cellos chooses an
execution **tier** first, then a runtime profile and SDK modules inside that
tier. Use `tier` only for execution/isolation classes; use `runtime profile` for
`rust-no-std`, future `rust-std`, POSIX/FFI, and Lua; use `SDK module` for
developer APIs.

---

## Execution Tiers

| Tier | Canonical name | Runtime profiles | Isolation | Current status | When to use |
|------|----------------|------------------|-----------|----------------|-------------|
| **Tier 1** | Trusted SAS Cell | `rust-no-std` shipped; `rust-std` planned; `ffi-posix` and `lua` trusted profiles | Shared SAS + LBI; fleet posture depends on signing/admission | Current native app path | Trusted first-party/platform cells, drivers, services, UI, embedded/robot apps. |
| **Tier 2** | Native Domain Cell | Same native Cell shape as Tier 1 | Private MMU domain; copied/domain-explicit IPC | Accepted design, **not implemented** | Future untrusted native third-party code without source disclosure. |
| **Tier 3** | VM Guest | `linux-guest` | Hypervisor / Stage-2 | ARM64 guest path exists; broader platform work tracked separately | Legacy Linux/POSIX stacks, fork-heavy apps, and untrusted workloads before Tier 2 exists. |

Legacy names: `Tier 1b` now means the Tier 1 `ffi-posix` or `lua` runtime
profile. `Tier 3b` now means the Tier 3 `linux-guest` profile. Guide filenames
keep the old names for link compatibility.

## SDK Modules

The SDK is one family, not a numbered set of tiers:

| SDK area | Examples | Applies to | Current maturity |
|---|---|---|---|
| Foundation | manifest, syscall ABI, lifecycle entrypoint | Tier 1 and future Tier 2 native Cells | Shipped for current native Cells |
| Runtime profiles | `rust-no-std`, future `rust-std`, `ffi-posix`, `lua` | Profile-specific setup | `rust-no-std`, trusted `ffi-posix`, and Lua exist; `rust-std` is planned for G4 |
| Service clients | VFS, net, IPC, service discovery | Tier 1 and future Tier 2 native Cells | Available in the native SDK; coverage remains service-specific |
| UI/graphics | ViUI, signal API, surfaces | Native UI Cells | ViUI path exists |
| Middleware/helpers | AppContext, wrappers, RAII handles | Native app ergonomics | Available incrementally; not a separate SDK tier |
| Tooling | signing, manifest checks, image/build helpers | Build and release | Development tooling exists; fleet production key/admission provisioning is not complete |
| Guest integration | VirtIO/proxy contracts | Tier 3 VM guests | ARM64 path exists; strict guest verification is KVM/hardware-gated |

---

## Decision Tree: Which Tier?

```
┌─ "I have existing C/C++/Zig code"
│  └─ Use Tier 1 ffi-posix profile (legacy: Tier 1b) if trusted.
│
├─ "I want to write Rust"
│  ├─ "Need VFS, network, or IPC?"
│  │  └─ Use Tier 1 + service-client SDK modules
│  ├─ "Building a UI or dashboard?"
│  │  └─ Use Tier 1 + ViUI
│  ├─ "Handling cryptographic keys?"
│  │  └─ Use Tier 1 + Silo API (G2+ hardware capability)
│  └─ "Just syscalls and linked libraries?"
│     └─ Use Tier 1 rust-no-std profile
│
├─ "I want quick scripting / dynamic code"
│  └─ Use Tier 1 lua profile (legacy: Tier 1b Lua)
│
├─ "I need untrusted native code without a VM"
│  └─ Tier 2 is the accepted destination, but not implemented yet.
│
└─ "I have a legacy Linux binary / fork() is essential"
   └─ Use Tier 3 linux-guest profile (legacy: Tier 3b)
```

---

## Guides by Tier

- **[Tier 1 Rust (Bare)](guides/tier1-rust-bare.md)** — Minimal entry point, syscall allowlists, manifest declaration.
- **[Tier 1 Rust + SDK modules](guides/tier1-rust-sdk.md)** — AppContext, VFS/network clients, service discovery.
- **[Tier 1 Rust + ViUI](guides/viui-guide.md)** — Signal API, .vi DSL, compositor surfaces (see `system-architecture.md` §6).
- **[Tier 1 + Silo API](guides/tier1-silo.md)** — SiloHandle, cryptographic isolation, ARM64/x86 only.
- **[Tier 1 FFI/POSIX profile](guides/tier1b-c-zig.md)** — legacy `Tier 1b` guide: POSIX shim vs mlibc.
- **[Tier 1 Lua profile](guides/tier1b-lua.md)** — legacy `Tier 1b` guide: interpreter cell, VFS bindings, restricted stdlib.
- **[Tier 3 Linux guest profile](guides/tier3b-linux-vm.md)** — legacy `Tier 3b` guide: full kernel in hypervisor.

---

## SAS Laws Apply to All Cells

All Cells (regardless of tier) must respect the **8 Coding Laws** in [code-standards.md](code-standards.md):

| Law | Rule |
|-----|------|
| **Law 2** | Owned buffers (`Box<[u8]>`) across async; never `&mut [u8]`. |
| **Law 4** | Cells forbid `unsafe` (no exceptions for `#[no_mangle] main` in app_entry!). |
| **Law 5** | No `mod.rs` files — use `foo.rs` parallel to `foo/`. |
| **Law 8** | Implement `Drop` for all resources; no process cleanup. |

---

## Build & Run

```bash
# In a Cell directory (e.g., cells/apps/hello-cell):
cargo build --release --target riscv64gc-unknown-none-elf

# Run on QEMU:
./run.ps1   # Uses scripts/run-qemu-riscv64.sh internally
```

For multi-arch builds (ARM64, x86), see [getting-started.md](getting-started.md) § Build.

---

## Examples

- **Tier 1 bare**: `cells/demos/hello-cell/src/main.rs`
- **Tier 1 + SDK service clients**: `cells/demos/sdk-demo/src/main.rs`
- **Tier 1 + ViUI**: `cells/apps/robot-dashboard/src/main.rs`
- **Tier 1 Silo API**: `cells/tests/silo-test/src/main.rs`
- **Tier 1 ffi-posix profile (mlibc)**: `cells/tests/mlibc-smoke/src/main.rs`
- **Tier 1 ffi-posix profile (POSIX shim)**: `cells/tests/posix-shim-test/src/main.rs`
- **Tier 1 lua profile**: `cells/runtimes/lua/src/main.rs`

---

## Next Steps

1. Pick your tier from the **Decision Tree** above.
2. Read the corresponding **Guide**.
3. Copy a canonical example from the list above.
4. Adapt for your use case.
5. See [api-reference.md](api-reference.md) for syscall details.

---

## FAQ

**Q: Can I use the Rust standard library?**
A: Not yet in native Cells. Today use `ostd` and `rust-no-std`; G4 tracks a
future pure-Rust `std` profile. For trusted C/POSIX interop, use the Tier 1
`ffi-posix` profile.

**Q: Do I need to write unsafe code?**
A: No. Cells forbid `unsafe` at the crate root (Law 4). Only syscall entry points (`app_entry!` generated code) use it, and it is isolated.

**Q: How do I talk to other Cells?**
A: Use IPC. See [api-reference.md](api-reference.md) § IPC for syscalls (`sys_send`, `sys_recv`); Tier 1 SDK service clients provide ergonomic wrappers.

**Q: Can I spawn other Cells?**
A: Only `/bin/*` Cells with `spawn = true` in the manifest. See Phase 30 (project-roadmap.md).

**Q: What about real-time performance?**
A: Tier 1 is native (~1 μs syscall latency on QEMU). Use `sys_heartbeat()` for watchdog-style deadlines.
