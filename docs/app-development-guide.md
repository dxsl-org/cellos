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
| **Tier 1** | Trusted SAS Cell | `rust-no-std` shipped; pure-Rust `rust-std` in-tree; `ffi-posix` and `lua` trusted profiles | Shared SAS + LBI; fleet posture depends on signing/admission | Current native app path | Trusted first-party/platform cells, drivers, services, UI, embedded/robot apps. |
| **Tier 2** | Native Domain Cell | Same native Cell shape as Tier 1; `cpp-freestanding` (planned) | Private MMU domain; copied/domain-explicit IPC | Paged-domain mechanism is shipped and routed by artifact class (unsigned, `FFI`, `UNTRUSTED`) on RV64/AArch64/x86_64, and the on-path admission control landed ([ADR-0019](decisions/0019-tier2-admission-control-on-path.md)): one policy gate at the publication point, explicit per-profile posture, device/DMA authority refused. Physical containment, DMA quarantine and production approvals remain open, and no capability is qualified in the ledger yet | Containment target for untrusted/FFI native code in development profiles; fleet profiles deny domain-class artifacts. Treat deployment as gated by the acceptance ledger, and never silently assign such code to Tier 1. |
| **Tier 3** | VM Guest | `linux-guest` | Hypervisor / Stage-2 | ARM64 guest path exists; broader platform work tracked separately | Legacy Linux/POSIX stacks, fork-heavy apps and supported untrusted guest workloads under target-specific qualification. |

Legacy names: `Tier 1b` now means the Tier 1 `ffi-posix` or `lua` runtime
profile. `Tier 3b` now means the Tier 3 `linux-guest` profile. Guide filenames
keep the old names for link compatibility.

## SDK Modules

The SDK is one family, not a numbered set of tiers:

| SDK area | Examples | Applies to | Current maturity |
|---|---|---|---|
| Foundation | manifest, syscall ABI, lifecycle entrypoint | Tier 1 and future Tier 2 native Cells | Shipped for current native Cells |
| Runtime profiles | `rust-no-std`, `rust-std`, `ffi-posix`, `lua`, `cpp-freestanding` (planned) | Profile-specific setup | `rust-no-std`, in-tree `rust-std` (pure-Rust PAL), trusted `ffi-posix`, and Lua exist; `cpp-freestanding` is planned (ADR-0018 §2.3) |
| Service clients | VFS, net, IPC, service discovery | Tier 1 and future Tier 2 native Cells | Available in the native SDK; coverage remains service-specific |
| UI/graphics | ViUI, signal API, surfaces, desktop environment | Native UI Cells | ViUI and Desktop environment (`desktop`) exist |
| Middleware/helpers | AppContext, wrappers, RAII handles | Native app ergonomics | Available incrementally; not a separate SDK tier |
| Tooling | signing, manifest checks, image/build helpers | Build and release | Development tooling exists; fleet production key/admission provisioning is not complete |
| Guest integration | VirtIO/proxy contracts | Tier 3 VM guests | ARM64 path exists; strict guest verification is KVM/hardware-gated |

---
Tier 1 branches below require trusted code. Tier 2 routing exists and is exercised — a signed
`FFI`/`UNTRUSTED` cell or an unsigned cell runs in a private domain, asserted by
`tests/integration/tests/tier2_fault_isolation.rs`, `aarch64-boot.rs`, and `x86_64-boot.rs` —
and the admission control that gates it is on the path since
[ADR-0019](decisions/0019-tier2-admission-control-on-path.md) landed: one policy evaluated at
the publication point, explicit posture per build profile, and device/DMA authority refused
because a domain root cannot map it. A denial is final — no task, no domain, no SAS fallback.
Development profiles enable admission; a fleet-secure profile leaves it disabled, so a
domain-class artifact is denied rather than admitted anywhere. Qualification claims (`PASS`) are
still gated by the acceptance ledger, so treat Tier 2 deployment as gated by evidence, never by
this paragraph.


## Decision Tree: Which Tier?

```
┌─ "I have existing C/C++/Zig code"
│  └─ Use Tier 1 ffi-posix profile (legacy: Tier 1b) if the code is trusted.
│     C++ uses the `cpp-freestanding` subset profile (planned, ADR-0018 §2.3).
│     Untrusted C/C++ belongs in Tier 2 once its admission control lands (ADR-0019).
│
├─ "I want to write Rust"
│  ├─ "Need VFS, network, or IPC?"
│  │  └─ Use Tier 1 + service-client SDK modules
│  ├─ "Building a UI or dashboard?"
│  │  └─ Use Tier 1 + ViUI
│  ├─ "Handling cryptographic keys?"
│  │  └─ Use a purpose-specific KMS client; the Silo backend is not a public API.
│  └─ "Just syscalls and linked libraries?"
│     └─ Use Tier 1 rust-no-std profile
│
├─ "I want quick scripting / dynamic code"
│  └─ Use Tier 1 lua profile (legacy: Tier 1b Lua)
│
├─ "I need to view documents (HTML, PDF, Markdown, text, images)"
│  └─ Use Ocel (Tier 2, cells/apps/ocel). See ADR-0017.
│
├─ "I need a full web browser (Gmail, YouTube, web apps)"
│  └─ Use Tier 3 Chrome via hypervisor. See ADR-0017.
│
├─ "I need untrusted native code without a VM"
│  └─ Tier 2 is the target: routing and the on-path admission control are live in development
│     profiles (ADR-0019). Claims remain ledger-gated, so check the acceptance matrix first.
│
└─ "I have a legacy Linux binary / fork() is essential"
   └─ Use Tier 3 linux-guest profile (legacy: Tier 3b)
```

---

## Guides by Tier

- **[Tier 1 Rust (Bare)](guides/tier1-rust-bare.md)** — Minimal entry point, syscall allowlists, manifest declaration.
- **[Tier 1 Rust + SDK modules](guides/tier1-rust-sdk.md)** — AppContext, VFS/network clients, service discovery.
- **[Tier 1 Rust + ViUI](guides/viui-guide.md)** — Signal API, .vi DSL, compositor surfaces (see `system-architecture.md` §6).
- **[Development Silo provider](guides/tier1-silo.md)** — KMS-mediated `DEV_REFERENCE` AArch64 QEMU evidence only; no public Silo API or production/hardware custody claim.
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
- **Development Silo evidence**: `cells/tests/silo-test/src/main.rs` (KMS-mediated, `DEV_REFERENCE`, AArch64 QEMU only)
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
