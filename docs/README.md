# Cellos Documentation Index

**Version**: v0.2.1-dev (Mycelium Era) | **Last updated**: 2026-10-20

---

## Start Here

The sole active program is **Cell-to-Cell Anywhere on Intel x86-64**, using one fixed headless hardware configuration. Read [ADR-0022](decisions/0022-intel-x86-64-c2c-only-direction.md) and [current focus](roadmap/current-focus.md) before choosing work: every task must identify a direct C2C-on-Intel deliverable, dependency, or necessary regression.

The current contract-cutover owner is [ADR-0024](decisions/0024-cell-kernel-authority-contract.md)
and [Spec 25](specs/25-cell-kernel-authority-contract.md), with the
[detailed implementation plan](../.agents/261011-1200-cell-contract-authority/plan.md).
Related unfinished execution is scoped and locked by
[CELL-CONTRACT-0024](../.agents/cell-contract-0024-locks.md); old phase links and
admissions cannot bypass that lock. Architecture approval is not concrete ABI or remote activation approval.

The accepted [Cellbox CLI architecture](decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md)
selects a modular single executable, typed pipelines, essential operational tools
and selective Ratatui TUI outside viUI. Read the [design](cellbox-cli.md) and
[pending implementation plan](../.agents/261020-1750-cellbox-cli/plan.md).
This is documentation/planning approval, not a code/ABI unlock or an IDE program.

Use the [root README](../README.md) ([Tiếng Việt](../README_VN.md)) for key
features and implementation status; the [architecture guide](system-architecture.md)
for component ownership and flows; Spec 25/26 for normative requirements.
Current runtime evidence belongs to the roadmap/portfolio and lane guides, not
feature descriptions or a diagram.

The approved target keeps one Cell on one node (one physical/VM kernel instance);
a cluster has multiple nodes. Broker Cell owns bounded peer sessions/reconnect and
direct/relay routing, service-net owns IP/TCP, protected authority owns trust, and
kernel owns local primitives. P08 now includes a scoped placement/autoscaling
controller Cell for one application across selected nodes within the admitted
cluster (not federation), with durable application-wide reservations and explicit destination
admission, Ready routing and safe drain. Its authority is single active, fenced
and fixed-home: unknown creates retain budget, with no timeout promotion or
implicit remote fallback. C2C hides transport, not failures. P10 qualifies actual
hundreds of concurrent clients, not hundreds of Cells or promised throughput.
The ten phases remain pending and all 38 locks remain; this is documentation-only
approval, not implementation, ABI ratification or remote activation.

[Spec 26](specs/26-workload-placement-scaling-contract.md) completes the workload
contract: opt-in logical endpoints, per-ingress router Cells with bounded weighted
round-robin, explicit node placement, accounted control-plane/replica shares and
sample-driven autoscaling. P08 delivers these in gated internal slices; P10 must
observe load-driven spawn and safe scale-down, not merely pre-created replicas.

The target includes Tier 1 native Cells, Tier 2 C/C++ domains, and Tier 3 VM participants through explicit adapters across local/LAN/relay paths, not transparent arbitrary-app distribution. Retained x86 Tier 2/runtime evidence is profile-specific, not production qualification; Intel VMX remains incomplete and no physical x86 HCL entry is qualified. Existing security, ABI, purchase, remote-activation, and production gates remain unchanged.

GUI, browser, AI, robotics, general-purpose OS expansion, and new AMD/ARM/RISC-V platform work are paused as independent programs. The reference lists below preserve existing contracts, code documentation, and historical evidence; listing a document does not activate its old roadmap. Necessary shared-code regressions and existing ARM protected-authority evidence may support the Intel program without reopening another platform program.

| File | Purpose |
|------|---------|
| [ADR-0022](decisions/0022-intel-x86-64-c2c-only-direction.md) | Canonical sole-program decision |
| [ADR-0023](decisions/0023-local-service-generation-binding.md) | Local service binding axis `(cell_id, generation)`; additive `LookupServiceBound = 429` (Law-1 FROZEN) |
| [ADR-0024](decisions/0024-cell-kernel-authority-contract.md) | Node-local Cell/Kernel identity, object-capability authority, scoped cluster placement/autoscaling and execution freeze |
| [ADR-0027](decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md) | Unified desktop: session tabs, Spotlight HUD, six-zone snap, Linear theme, Files, GUI terminal, single-app Tier 3 guest |
| [ADR-0028](decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md) | Accepted CLI-first cellbox, typed data, editor/search/top/SSH and reusable terminal/Ratatui boundaries; implementation pending |
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
| [cellbox-cli.md](cellbox-cli.md) | Accepted CLI, pipeline, terminal/TUI, selected-tool and migration design; not a frozen wire ABI |
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
| [specs/25-cell-kernel-authority-contract.md](specs/25-cell-kernel-authority-contract.md) | Selected Cell/Kernel/authority contract v1.3; independent budget enforcement and AC-01..AC-50; implementation/ABI gates remain |
| [specs/26-workload-placement-scaling-contract.md](specs/26-workload-placement-scaling-contract.md) | v1.1: placement, bounded routing, Budget Authority/genesis/live claims, challenge freshness and homogeneous-domain autoscaling; runtime pending |
