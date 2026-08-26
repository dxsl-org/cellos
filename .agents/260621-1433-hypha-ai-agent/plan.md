# Hypha — ViCell's First Real Application

> **Hypha** (sợi nấm): a single living thread of the *Mycelium* that threads through every Cell
> and coordinates them — a native Tier-1 Rust AI agent. The coordinating intelligence of the
> Cell network, expressed as an app that can only exist *because* of ViCell's properties.

## Why this app (not another demo)

Existing cells (`hello-cell`, `robot-dashboard`, `https-demo`, …) are **demos** — they prove a
single primitive works. Hypha is the first app that is (a) genuinely useful and (b) showcases
what makes ViCell *unique*:

- **LBI capability isolation made tangible** — each tool is a kernel-enforced sandbox Cell; the
  agent itself holds no dangerous authority.
- **Never-die in action** — kill the LLM gateway mid-conversation, supervisor respawns, agent
  reconnects via service lookup, conversation continues.
- **Zero-copy IPC at scale** — multi-KB prompts move via Grant, not message-copy.
- **Natural-language robot control** — ties straight into the G1 graduation robot demo.

## Strategic role: Hypha drives OS completion

Hypha is deliberately ambitious. Building it **surfaces the missing modules of ViCell**. Every
phase reveals gaps in `ostd`/kernel that a real app needs but demos never exercised. Those gaps
are tracked in **[os-gaps.md](./os-gaps.md)** and filled incrementally. The app is both the
deliverable *and* the forcing function for maturing the OS.

## Documents

| Doc | Purpose |
|-----|---------|
| [vision.md](./vision.md) | **Capability ceiling & long-horizon vision** — how far Cell-calling-Cell goes, Hypha vs Haily, the "new species" reframe |
| [architecture.md](./architecture.md) | Cell topology, agentic loop, IPC protocols, capability model, red-team |
| [os-gaps.md](./os-gaps.md) | **Living register** of missing OS modules Hypha surfaces — the core workflow |
| [phase-00-llm-gateway.md](./phase-00-llm-gateway.md) | P0: hand-rolled HTTPS LLM client Cell |
| [phase-01-core.md](./phase-01-core.md) | P1: agent brain — interactive chat loop |

## Phase roadmap

Each phase must boot/run. Phases are detailed into their own `phase-NN-*.md` file *when they
become the active phase* (incremental planning — we do not pre-spec everything).

| Phase | Name | Goal | Status |
|-------|------|------|--------|
| **P0** | `llm-gateway` | Hand-roll HTTPS POST to an LLM endpoint (host proxy); one-shot completion; no tools. Prove network path. Extends `https-demo`. | ✅ Code builds + integration test added; live round-trip needs host proxy (user step) |
| **P1** | `hypha` core | Shell/UART input → llm-gateway → print reply. Plain chat, conversation in heap. | ✅ Code builds + `hypha-boot` integration test (banner+prompt+exit); live chat needs host proxy |
| **P2** | tool protocol | `AgentToolRequest/Response` typed IPC; `tool-fs` (read/write/list `/data`); agentic loop with 1 tool. | ✅ **COMPLETE** — boot run #3 confirmed full 2-round-trip loop: `list_dir /bin` → 13 binaries returned |
| **P3** | `tool-sys` + `tool-spawn` | Agent inspects the system and launches other Cells → real "OS agent". Needs name-based tool discovery. | ✅ Code complete — `hypha-p3-boot` integration test added; live boot run #4 needs mock proxy |
| **P4** | `tool-peripheral` 🎯 | Robot demo: NL sensor/actuator control (SHT3x I2C + GPIO/PWM). **G1 showcase.** | 🔜 Ready (plan written 2026-07-12) — see [phase-04](./phase-04-tool-peripheral.md) |
| **P5** | persistence/memory | Conversation + facts to `/data`; context trimming. (Haily KMS analog) | 📋 Planned |
| **P6** | ViUI chat (optional) | On-screen chat surface (robot-dashboard pattern). | 📋 Backlog |
| **P7** | G3 NPU backend | Swap llm-gateway backend to local NPU model via Tier 1b. | 📋 G3 |

## Key dependencies & decisions

- **LLM source** (top risk): start with a **host-side LLM proxy** reached via QEMU NAT
  (10.0.2.2). Public-internet DNS over NAT is unverified. Self-contained operation waits for
  G3 on-device NPU. — *Decided 2026-06-21.*
- **Serialization**: postcard for IPC (matches the rest of the system); JSON for LLM/tool args
  (needs a no_std JSON dep — see os-gaps).
- **Concurrency**: tools run **sequentially** in v1 (single-cell async). Parallelism later.
- **App home**: `cells/apps/hypha/` (real app, not a demo under `cells/demos/`).
- **Repo layout — NOT a git submodule** *(decided 2026-06-21)*. Hypha is a **cluster of normal
  workspace-member crates**, grouped by directory (see below). Rationale: Hypha is the OS
  forcing function — filling each os-gap is an **atomic commit spanning both Hypha and
  `ostd`/`api`/kernel**; a submodule turns every such change into a fragile two-repo pointer
  dance. Also: bare-metal cells cannot be independent of the kernel ABI (they need `.ld` + a
  bases-map VA entry + `gen_disk` embedding), and the workspace lists members explicitly
  ([Cargo.toml:17-115](../../Cargo.toml#L17-L115)) — a separate `[workspace]` root would have to
  be `exclude`d (cf. `tools/vi-compiler`) and lose shared `[workspace.dependencies]`.
  - **Revisit trigger**: after **P4** (G1 showcase), once the ABI Hypha needs has stabilized and
    os-gap churn slows. If independent release/CI or open-sourcing Hypha is then wanted, extract
    via `git filter-repo`. To keep that cheap: keep all Hypha code + `agent-proto` in a clean
    subtree that never reaches up into the parent except the unavoidable `ostd`/`api` deps.

### Crate layout (workspace members to add as built)
```
cells/apps/hypha/
├── README.md
├── llm-gateway/     # P0  → member cells/apps/hypha/llm-gateway
├── core/            # P1  → member cells/apps/hypha/core
└── tools/{fs,sys,spawn,peripheral,net}/   # P2+ → one member each
libs/agent-proto/    # shared IPC types → member libs/agent-proto
```
Each new crate adds its path to [Cargo.toml](../../Cargo.toml) `members` (apps under the
`# Cells - Apps` group, `agent-proto` under `libs`).

## Naming hierarchy (no collision with the era codename)

```
ViCell          = the organism (the OS)
 └─ Mycelium     = this development era's whole network  (era codename — README.md:8)
     └─ Hypha    = one living thread coordinating every Cell  (this app)
```
