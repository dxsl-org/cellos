# Cellos Documentation Index

**Version**: v0.2.1-dev (Mycelium Era) | **Last updated**: 2026-10-08

---

## Start Here

The sole active program is **Cell-to-Cell Anywhere on Intel x86-64**, using one fixed headless hardware configuration. Read [ADR-0022](decisions/0022-intel-x86-64-c2c-only-direction.md) and [current focus](roadmap/current-focus.md) before choosing work: every task must identify a direct C2C-on-Intel deliverable, dependency, or necessary regression.

The target includes Tier 1 native Cells, Tier 2 C/C++ domains, and Tier 3 VM participants through explicit adapters across local/LAN/relay paths, not transparent arbitrary-app distribution. Intel VMX is incomplete, x86 Tier 2 admission evidence is test-only with a C++ shim gap, and no physical x86 HCL entry is qualified. Existing security, ABI, purchase, remote-activation, and production gates remain unchanged.

GUI, browser, AI, robotics, general-purpose OS expansion, and new AMD/ARM/RISC-V platform work are paused as independent programs. The reference lists below preserve existing contracts, code documentation, and historical evidence; listing a document does not activate its old roadmap. Necessary shared-code regressions and existing ARM protected-authority evidence may support the Intel program without reopening another platform program.

| File | Purpose |
|------|---------|
| [ADR-0022](decisions/0022-intel-x86-64-c2c-only-direction.md) | Canonical sole-program decision |
| [ADR-0023](decisions/0023-local-service-generation-binding.md) | Local service binding axis `(cell_id, generation)`; additive `LookupServiceBound = 429` (Law-1 FROZEN) |
| [roadmap/current-focus.md](roadmap/current-focus.md) | Current Intel C2C priorities and evidence gates |
| [QEMU q35 x86-64](../boards/qemu/q35-x86_64/README.md) | Documented x86 build/test path; software evidence, not physical qualification |
| [getting-started.md](getting-started.md) | Setup, build, run, first contribution |
| [app-development-guide.md](app-development-guide.md) | Write/build/run/test a Cell application (worked examples) |
| [codebase-summary.md](codebase-summary.md) | Quick reference: LOC, crates, features |
| [FAQ.md](FAQ.md) | Common questions about architecture |

---

## Project

| File | Purpose |
|------|---------|
| [project-overview-pdr.md](project-overview-pdr.md) | Vision, requirements, success metrics |
| [project-roadmap.md](project-roadmap.md) | Roadmap entrypoint and links to split roadmap files |
| [roadmap/README.md](roadmap/README.md) | Roadmap folder index |
| [roadmap/current-focus.md](roadmap/current-focus.md) | Active stage, current gates, and next work |
| [roadmap/hardware-tracks.md](roadmap/hardware-tracks.md) | Intel qualification dependencies and retained/paused hardware lanes |
| [roadmap/product-stages.md](roadmap/product-stages.md) | Historical G1-G5 overlay, subject to the sole-program decision |
| [roadmap/runtime-and-platform-tracks.md](roadmap/runtime-and-platform-tracks.md) | Runtime dependencies and retained/paused platform overlays |
| [roadmap/technical-milestones.md](roadmap/technical-milestones.md) | Current milestone snapshot |
| [roadmap/completed-history.md](roadmap/completed-history.md) | Condensed completion history |
| [roadmap/open-risk-register.md](roadmap/open-risk-register.md) | Confirmed open code/readiness risks |
| [project-changelog.md](project-changelog.md) | History of changes per phase |

---

## Architecture & Standards

| File | Purpose |
|------|---------|
| [system-architecture.md](system-architecture.md) | System layers, kernel, HAL, IPC |
| [decisions/0003-application-tier-taxonomy.md](decisions/0003-application-tier-taxonomy.md) | Canonical application tier, runtime profile, and SDK terminology |
| [code-standards.md](code-standards.md) | The 8 Coding Laws + conventions |
| [PATTERNS.md](PATTERNS.md) | Common Rust patterns for Cellos |
| [security-model.md](security-model.md) | STRIDE analysis, known limitations |

---

## Reference

| File | Purpose |
|------|---------|
| [api-reference.md](api-reference.md) | Syscall ABI, trait definitions, examples |
| [performance-report.md](performance-report.md) | Hardware-qualified IPC latency targets and QEMU regression tracking |
| [code-metrics.generated.md](code-metrics.generated.md) | Generated kernel-size metrics owner; canonical moving counts |
| [app-tier-acceptance-ledger.json](app-tier-acceptance-ledger.json) | Authoritative content-addressed app-tier qualification ledger |
| [app-tier-acceptance-matrix.md](app-tier-acceptance-matrix.md) | Review-only projection of the app-tier ledger |

---

## Feature Guides

| File | Purpose |
|------|---------|
| [scripting-guide.md](scripting-guide.md) | Lua 5.4 usage; historical MicroPython status |
| [ocel-viewer.md](guides/ocel-viewer.md) | Ocel native viewer, its Tier 2 script service, and the QEMU lane |
| [hotswap-guide.md](hotswap-guide.md) | Live Cell upgrade protocol |
| [vfs-api.md](vfs-api.md) | VFS IPC opcodes and protocol |
| [network-api.md](network-api.md) | Network service IPC, DHCP, socket API |
| [display-api.md](display-api.md) | Compositor IPC, surface lifecycle |
| [input-api.md](input-api.md) | Input service IPC, KeySym, focus |

---

## Design Specifications

Internal design docs — read before implementing a subsystem.

| File | Topic |
|------|-------|
| [specs/00-context.md](specs/00-context.md) | Prime directive, coding laws, workflow |
| [specs/00-fork.md](specs/00-fork.md) | Strategy for forking external code |
| [specs/01-core.md](specs/01-core.md) | Cellular model, symbol table, security |
| [specs/02-memory.md](specs/02-memory.md) | SAS layout, quota, metadata registry |
| [specs/03-runtime.md](specs/03-runtime.md) | IPC, async/await, hot-swap, boot optimization |
| [specs/04-hardware.md](specs/04-hardware.md) | Multi-arch HAL, SMP |
| [specs/05-application.md](specs/05-application.md) | Application tiers, runtime profiles, and SDK modules |
| [specs/06-graphics.md](specs/06-graphics.md) | Compositor, framebuffer, input dispatch |
| [specs/07-networking.md](specs/07-networking.md) | Network stack, smoltcp, zero-copy |
| [specs/08-power.md](specs/08-power.md) | Power states, hibernation, thermal |
| [specs/09-vfs.md](specs/09-vfs.md) | VFS traits, dual-filesystem, direct I/O |
| [specs/10-testing.md](specs/10-testing.md) | Test strategy, QEMU harness, coverage |
| [specs/11-shell.md](specs/11-shell.md) | Shell design, ELF execution, zero-copy ls |
| [specs/12-reliability.md](specs/12-reliability.md) | Never-die axes, supervisor restart, RT bounds |
| [specs/13-peripherals.md](specs/13-peripherals.md) | GPIO/I2C/SPI/UART/CAN driver cells |
| [specs/14-distributed.md](specs/14-distributed.md) | Swarm/cluster lifecycle, lease, split-brain |
| [specs/14-viui.md](specs/14-viui.md) | ViUI reactive Signal Tree + .vi DSL |
| [specs/15-kernel-boundary.md](specs/15-kernel-boundary.md) | Kernel whitelist/blacklist law + theory |
| [specs/16-rustc-tcb.md](specs/16-rustc-tcb.md) | rustc as TCB — LBI guarantees, limits, policies |
| [specs/17-ipc-wire-contract.md](specs/17-ipc-wire-contract.md) | Cell IPC: framing, recv-mask, byte-0 registry |
| [specs/23-native-sdk-contract.md](specs/23-native-sdk-contract.md) | Ratified Native SDK family contract and evidence matrix |
| [specs/24-ai-inference-architecture.md](specs/24-ai-inference-architecture.md) | Ratified AI Inference & Small Language Model Architecture |
