# Cellos

[![CI](https://github.com/dxsl-org/cellos/actions/workflows/ci.yml/badge.svg)](https://github.com/dxsl-org/cellos/actions/workflows/ci.yml)
[![Ko-fi](https://img.shields.io/badge/Ko--fi-Donate-%23FF5E5B?logo=ko-fi)](https://ko-fi.com/dxsl_org)
[🌐 Tiếng Việt](./README_VN.md)

**The next-generation Rust-native OS designed for embedded systems, RTOS, robotics, and dedicated servers/PCs.** 

Instead of organizing software into bulky traditional processes, Cellos architects the system into **Cells**. Cells share a Single Address Space (SAS) and are fully isolated by Rust's powerful type system, delivering maximum performance without compromising safety.

---

## ✨ What Makes Cellos Unique? (Compared to traditional OSes)

Instead of following traditional monolithic or pure microkernel paradigms, Cellos introduces:

*   **Cellular Single Address Space (SAS):** Eliminates expensive hardware MMU context switches for trusted components. Inter-cell communication (IPC) is virtually zero-copy through direct Rust ownership transfer.
*   **Language-Based Isolation (LBI):** Safety isn't enforced by costly hardware boundaries, but by Rust's strict compiler type system (`#![forbid(unsafe_code)]`). Memory violations are caught and blocked at compile time.
*   **Instant-On & Heap Snapshot:** Built-in capability to snapshot and restore the heap memory state, enabling lightning-fast boot times and rapid recovery for embedded devices.
*   **3-Tier Hybrid Architecture:** Seamlessly run fully trusted native code (Tier 1), dynamically sandboxed untrusted code via hardware MMU (Tier 2), or a full legacy OS like Linux in a hardware-isolated VM (Tier 3) — all governed dynamically by the same micro-scheduler.

---

## 🎯 Vision & Positioning: What is Cellos (and what is it not)?

Cellos was born with a clear goal: **Performance and reliability for dedicated hardware.** We are not racing to build a general-purpose operating system.

*   ✅ **Born for specialized hardware:** Cellos shines on embedded systems, robotics, servers running core services, or kiosk/appliance PCs with a focused mission.
*   ✅ **The future of RTOS & Low Latency:** Focuses on strict resource control and real-time predictability, managed by an ultra-lightweight nano-kernel.
*   ❌ **Not a Linux/Windows desktop replacement:** We are not trying to build an OS to run everyday software or support every random keyboard/mouse on the market.
*   ❌ **No legacy hardware bloat:** Cellos refuses to bloat the codebase to maintain backward compatibility with thousands of obsolete devices. Hardware support is a strict contract: specific boards, microcontrollers, and firmware. (Running successfully on QEMU does not imply physical hardware certification — see [Hardware Policy](./docs/hardware-compatibility-list.md)).

### The 3-Tier Execution Model
Rust-native is the soul of the project, but Cellos is pragmatic enough to handle complex needs through a multi-tier architecture:
1.  **Tier 1 (Core & Native Cell):** Maximum speed in the shared memory space (SAS). Absolutely trusted.
2.  **Tier 2 (Paged Domain Cell):** Runs native code in private hardware MMU pages to sandbox software needing strict hardware boundaries (C-FFI, unverified code).
3.  **Tier 3 (VM Guest - The Escape Hatch):** Runs a full Guest OS (like Linux) inside a virtual machine. **This is not Cellos' main goal**, but a specialized solution for running a full web browser or legacy applications requiring `fork()`/JIT. See [Browser Decision](./docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md) and [Guest Guide](./docs/guides/tier3b-linux-vm.md).

---

## 🚀 Project Status: `v0.2.1-dev` (Mycelium)

Active development phase: **G1 — Robot & Embedded** (Focusing on ARM64/RV64 SBCs and RV32 MCUs). Phase **G2 — Server & Specialized PC** will expand to multi-core and x86_64 machines.

| Target | Status | Notes |
|--------|--------|-------|
| `riscv64gc-unknown-none-elf` | ✅ **Primary** | Full boot support and all core services available. |
| `aarch64-unknown-none` | ✅ Boot | Scheduler reached; full G1 bring-up is in progress. |
| `x86_64-unknown-none` | ✅ Boot | CPL3 transition gate passed on QEMU q35. (See [q35 Docs](./boards/qemu/q35-x86_64/README.md)). No physical x86 machine is officially verified yet. |
| `riscv32imc-unknown-none-elf`| ✅ Boot | Cellos-Nano · Verified S-mode boot on QEMU. |

*Note:* Successful execution on QEMU serves as architectural proof, not a 100% operational guarantee on un-tuned physical boards.

---

## ⚡ 5-Minute Quick Start

To build Cellos, you need: **Rust nightly**, `qemu-system-riscv64`, and Python 3/PowerShell.

```powershell
# 1. Clone the repository
git clone https://github.com/dxsl-org/cellos.git
cd cellos

# 2. Build kernel (Rust handles PIC flags automatically, don't set globally)
cargo build --release

# 3. Create FAT32 disk image and boot QEMU
./gen_disk.ps1
./run.ps1        # Use Ctrl+A X to exit QEMU
```
*The Cellos command-line interface will appear. Try typing `ls /bin`, `date`, `cat /proc/version`!*

---

## 🧩 Source Structure & Architecture

```text
Cellos/
├── kernel/             Nano-kernel: Scheduler, memory, VFS, Cell loader, IPC
├── hal/                Abstraction layer & hardware mechanisms (RISC-V, ARM, x86)
├── boards/             Board identities, firmware contracts, and wiring
├── libs/               Shared libraries: ABI, ostd, ViUI, HTTP
├── cells/              Distributed software: apps, demos, drivers, tools, services
├── tests/integration/  Integration tests (Host-driven & QEMU)
└── docs/               Design specs & development guides
```

Before contributing or developing, please review the system design at [system-architecture.md](./docs/system-architecture.md) and the security model at [security-model.md](./docs/security-model.md).

---

## ⚖️ The 8 Coding Laws of Cellos

To maintain our uncompromising vision of memory safety and modular design, the entire codebase strictly adheres to:

1. **Interface is Sacred:** Changing `libs/api/` requires high consensus (2x review).
2. **Owned Buffers for Async:** Always use `Box<[u8]>` instead of borrowed `&mut [u8]` for data crossing IPC/async boundaries.
3. **Multi-Architecture:** Use `VAddr`/`PAddr` types, never hardcode pointer sizes.
4. **Unsafe Management:** Cells strictly forbid `unsafe` (`#![forbid(unsafe_code)]`). If the kernel must use it, it requires a `// SAFETY:` note.
5. **Modern Module Style:** Use `foo.rs` alongside a `foo/` directory. `mod.rs` is forbidden.
6. **Cellos Naming:** Traits and Types use the `Vi` prefix (Virtual Interface, e.g., `ViDriver`). Files use `snake_case`.
7. **Trait Objects:** At system boundaries, use static polymorphism via `Arc<dyn ViDriver + Send + Sync>`.
8. **RAII - Clean Up Explicitly:** Cells are responsible for their own resource cleanup (Drop). There is no process-based cleanup due to the shared SAS nature.

👉 Read the details in [CONTRIBUTING.md](./CONTRIBUTING.md) and [code-standards.md](./docs/code-standards.md).

---

## 📚 Documentation

Cellos has a transparent specification and architecture system. Before working on a new subsystem, please read the corresponding documentation:

*   **Getting Started:** [getting-started.md](./docs/getting-started.md) | [project-roadmap.md](./docs/project-roadmap.md)
*   **Architecture:** [system-architecture.md](./docs/system-architecture.md) | [hardware-dev-guide.md](./docs/hardware-dev-guide.md)
*   **System Specs:** From context ([00-context.md](./docs/specs/00-context.md)) to memory ([02-memory.md](./docs/specs/02-memory.md)), application tiers ([05-application.md](./docs/specs/05-application.md)), networking, VFS... (Found in `docs/specs/`).

---

**Have an idea or code to contribute?** Run `cargo clippy -- -D warnings` and `cargo test --all` before creating a PR!

Cellos extends thanks for the great ideas from: *Theseus OS* (SAS & Live Evolution), *Asterinas* (FrameKernel Safety), *Tock* (Embedded traits), and *Redox OS* (Microkernel IPC).
