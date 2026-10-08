# Cellos

[![CI](https://github.com/dxsl-org/cellos/actions/workflows/ci.yml/badge.svg)](https://github.com/dxsl-org/cellos/actions/workflows/ci.yml)
[![Ko-fi](https://img.shields.io/badge/Ko--fi-Donate-%23FF5E5B?logo=ko-fi)](https://ko-fi.com/dxsl_org)
[🌐 Tiếng Việt](./README_VN.md)

**Cellos is a Rust-native research OS with one active direction: Cell-to-Cell Anywhere on Intel x86-64.**

The target is explicit Cell-to-Cell communication across local execution, LAN, and a gated relay on one fixed, headless Intel hardware configuration. Software participates through defined contracts and adapters, not transparent distribution of arbitrary applications. Trusted native Cells use a shared Single Address Space (SAS); Rust language-based isolation is not a blanket hardware or security boundary.

The [Intel-only direction (ADR-0022)](./docs/decisions/0022-intel-x86-64-c2c-only-direction.md) and [current focus](./docs/roadmap/current-focus.md) govern all work. Every task must name a direct C2C-on-Intel deliverable, dependency, or necessary regression. GUI, browser, AI, robotics, general-purpose OS expansion, and new AMD/ARM/RISC-V platform work are paused as independent programs; existing code, evidence, and necessary cross-architecture regressions are retained.

---

## ✨ What Makes Cellos Unique? (Compared to traditional OSes)

The architecture provides mechanisms to evaluate for that program, with distinct evidence and trust boundaries:

*   **Cellular Single Address Space (SAS):** Trusted components can share memory and transfer ownership of buffers. This does not promise zero-copy transport across tier or machine boundaries.
*   **Language-Based Isolation (LBI):** Safe Rust checks reduce memory-safety risks within the reviewed compiler/kernel/unsafe-code trust base. They do not isolate arbitrary untrusted binaries from the SAS.
*   **Heap Snapshot research:** Snapshot-format work exists, but capture and restore remain disabled in shipping images pending quiescence and a real board save/reset/restore/resume witness; instant-on is not a shipped guarantee.
*   **Three-tier target:** Native Cells, C/C++ paged domains, and VM guests are intended C2C participants through explicit adapters. This is a research and qualification direction, not a claim that all three tiers work end-to-end on Intel today.

---

## 🎯 Vision & Positioning: What is Cellos (and what is it not)?

The sole active goal is **Cell-to-Cell Anywhere on a fixed, headless Intel x86-64 configuration**, not a general-purpose desktop, robotics, or multi-market OS program.

*   **One hardware model first:** Qualify one exact configuration before a second machine of the same model. Target requirements include VT-x/EPT, VT-d, COM1, HPET, and the exact firmware/device contract in the [Hardware Compatibility List](./docs/hardware-compatibility-list.md).
*   **Local, LAN, and relay are separate gates:** Existing identity, authorization, protected-authority, and production gates remain in force. This decision authorizes neither purchases nor ABI/security relaxation, automatic remote execution, or production activation.
*   **No arbitrary-app transparency:** Existing applications require a defined port or adapter; a VM guest does not automatically become a distributed Cell.

### The 3-Tier Execution Model

All three tiers belong to the C2C target, with different trust boundaries:
1.  **Tier 1 (Core & Native Cell):** Trusted native Cells in the shared SAS, subject to signing and reviewed trust constraints.
2.  **Tier 2 (Paged Domain Cell):** Hardware-paged domains for C/C++ and other workloads requiring isolation. x86 admission evidence is test-only; the C++ shim gap remains open.
3.  **Tier 3 (VM Guest):** VM-backed participants connected through explicit guest adapters. Intel VMX is incomplete, and AMD SVM evidence on QEMU does not qualify Intel. The [Guest Guide](./docs/guides/tier3b-linux-vm.md) is a technical reference; the [earlier browser decision](./docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md) is not an active browser program.

---

## 🚀 Project Status: `v0.2.1-dev` (Mycelium)

Active direction: **Intel x86-64 Cell-to-Cell Anywhere**. Existing architecture evidence is retained below; it is not a list of parallel development programs.

| Target | Role / evidence | Limits |
|--------|-----------------|--------|
| `x86_64-unknown-none` (Intel) | **Sole active target**; QEMU q35 boot/CPL3 evidence | [q35 instructions](./boards/qemu/q35-x86_64/README.md); Intel VMX incomplete; no qualified physical x86 HCL entry. |
| `riscv64gc-unknown-none-elf` | Existing reference/QEMU lane; new platform work paused | Retain evidence and necessary shared-code regressions; not the primary direction. |
| `aarch64-unknown-none` | Existing boot and exact-device evidence; new platform work paused | Protected-authority evidence may inform Intel dependencies, not authorize a new ARM program. |
| `riscv32imc-unknown-none-elf` | Existing Cellos-Nano QEMU boot evidence; new platform work paused | Retain implementation and necessary regressions. |

QEMU is software/integration evidence, not physical hardware qualification. The supported `igb` IDs (`8086:10c9` in QEMU and flash-backed i210 `8086:1533`) do not establish that an actual machine is qualified.

---

## Getting Started

**Active x86 path:** Follow the checked-in [QEMU q35 x86-64 build and test instructions](./boards/qemu/q35-x86_64/README.md). That lane is a software witness, not Intel VMX completion or physical-PC certification.

### Legacy RV64 quickstart reference

The commands below are retained for the existing RV64 reference lane, not the Intel program's default build. They require **Rust nightly**, `qemu-system-riscv64`, and Python 3/PowerShell.

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
3. **Preserve Architecture Boundaries:** Use `VAddr`/`PAddr` types, never hardcode pointer sizes. Keeping existing cross-architecture code correct does not activate other hardware programs.
4. **Unsafe Management:** Safe Cells use `#![forbid(unsafe_code)]`; reviewed driver/FFI exemptions remain explicit. Kernel/HAL unsafe operations require documented `// SAFETY:` invariants.
5. **Modern Module Style:** Use `foo.rs` alongside a `foo/` directory. `mod.rs` is forbidden.
6. **Cellos Naming:** Traits and Types use the `Vi` prefix (Virtual Interface, e.g., `ViDriver`). Files use `snake_case`.
7. **Trait Objects:** At system boundaries, use static polymorphism via `Arc<dyn ViDriver + Send + Sync>`.
8. **RAII - Clean Up Explicitly:** Cells are responsible for their own resource cleanup (Drop). There is no process-based cleanup due to the shared SAS nature.

👉 Read the details in [CONTRIBUTING.md](./CONTRIBUTING.md) and [code-standards.md](./docs/code-standards.md).

---

## 📚 Documentation

Cellos has a transparent specification and architecture system. Before working on a new subsystem, please read the corresponding documentation:

*   **Active Direction:** [ADR-0022](./docs/decisions/0022-intel-x86-64-c2c-only-direction.md) | [current-focus.md](./docs/roadmap/current-focus.md)
*   **Getting Started:** [getting-started.md](./docs/getting-started.md) | [project-roadmap.md](./docs/project-roadmap.md)
*   **Architecture:** [system-architecture.md](./docs/system-architecture.md) | [hardware-dev-guide.md](./docs/hardware-dev-guide.md)
*   **System Specs:** From context ([00-context.md](./docs/specs/00-context.md)) to memory ([02-memory.md](./docs/specs/02-memory.md)), application tiers ([05-application.md](./docs/specs/05-application.md)), networking, VFS... (Found in `docs/specs/`).

---

**Have an idea or code to contribute?** Run `cargo clippy -- -D warnings` and `cargo test --all` before creating a PR!

Cellos extends thanks for the great ideas from: *Theseus OS* (SAS & Live Evolution), *Asterinas* (FrameKernel Safety), *Tock* (Embedded traits), and *Redox OS* (Microkernel IPC).
