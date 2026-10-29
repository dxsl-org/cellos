# Haily Personal Assistant on CellOS — Stage H0 Portability & Substrate Review

**Author/Role:** CellOS Architecture Review & Substrate Engineering  
**Date:** 2026-10-16  
**Reference Document:** `docs/roadmap/haily-personal-assistant.md` (Stage H0 Gate)  
**Target Repository:** `/mnt/d/haily/` (Rust Workspace)

---

## 1. Executive Summary & Review Scope

This document formally records the **Stage H0 (Release and Portability Review)** gate for adopting [Haily Personal Assistant](haily-personal-assistant.md) as the flagship distributed application consumer on CellOS under [ADR-0022](../decisions/0022-intel-x86-64-c2c-only-direction.md) and [ADR-0024](../decisions/0024-cell-kernel-authority-contract.md).

The review inspects the actual Haily source workspace at `/mnt/d/haily/`, maps its crate hierarchy against current CellOS substrates (Tier 1 Rust `std` PAL, `vimio` async driver, CSpace capability authority, and C2C 112B envelope), and defines the exact two-tier deployment architecture.

---

## 2. Inventory of Haily Crates & Substrate Compatibility Matrix

The Haily workspace at `/mnt/d/haily/` contains 11 primary crates. Their compatibility with CellOS native execution is evaluated below:

| Crate | Primary Function | Upstream Dependencies | CellOS Substrate Status & Mapping |
|---|---|---|---|
| **`haily-types`** | Leaf DTOs (`Request`, `ResponseChunk`, `SessionAccess`, `NeuralPrediction`) | `serde`, `serde_json`, `uuid`, `chrono` | **100% COMPATIBLE (Native Tier 1):** Compiles clean under CellOS custom target spec (`x86_64-unknown-cellos.json`) with zero modifications. |
| **`haily-kms`** | Key Management & Knowledge Context | `ed25519-dalek`, `x25519-dalek`, `aes-gcm` | **100% COMPATIBLE:** Cryptographic primitives map to `SYS_GET_RANDOM (214)` via PAL-019/PAL-031 entropy hooks and CellOS native DICE NCF1 identity. |
| **`haily-llm`** | LLM client & token streaming | `reqwest`, `eventsource-stream`, `tokenizers` | **HYBRID READY:** Local tiny/small models map directly to CellOS `service-ai` / `ai-engine` (Spec 24 GGUF engine). Remote cloud models route via `service-net` / `net-broker` egress. |
| **`haily-db`** | State & session persistence | `rusqlite`, `sqlx`, SQLite | **GATEWAY / VFS MAPPED:** In single-node mode, maps to CellOS VFS / `cellos-fs`. In cluster C2C mode, committed state is owned by a designated node (Spec 20 §2.6) rather than sharing a raw SQLite database file. |
| **`haily-tools`** | Tool execution (code, bash, search) | `tokio::process`, file I/O | **SANDBOX TRANSLATED:** Ambient Unix `sh` / `bash` execution is forbidden. Tools execute inside attenuated Child Cells with explicit CSpace capabilities (`OpenCap`, `ReadCap`, `WriteCap`). |
| **`haily-core`** | Orchestrator, planning, agent loop | `tokio`, `futures`, `tracing` | **COMPATIBLE via `vimio`:** The core agent state machine runs on `tokio(current_thread)` driven by `vimio::Selector` and `Waker`. |
| **`haily-io`** | Channel adapters (HTTP, CLI, Telegram) | `axum`, `tower`, `tokio` | **COMPATIBLE via HTTPD Gateway:** Adapts directly to CellOS `service-httpd` via `/api/c2c/dispatch` and `/api/gateway/stats`. |
| **`haily-app`** | Desktop / Tauri application runner | `tauri`, `keyring` | **HOST COORDINATOR:** Runs on host workstation (Windows/Linux) as the primary user interface surface, delegating workloads to CellOS. |
| **`haily-cli`** | Interactive REPL & eval CLI | `clap`, `crossterm` | **COMPATIBLE:** Interactive shell integration via `app-shell` and `AsyncStdin`. |
| **`haily-proactive`** | Proactive cards & suggestions | `chrono`, `serde` | **COMPATIBLE:** Evaluates background cards using timer-driven actor loops (`ostd::actor`). |
| **`haily-mobile-client`** | Thin-client mobile DTO protocol | `tokio-tungstenite`, `serde` | **COMPATIBLE:** Uses `haily-types::mobile` wire frames over C2C WebSocket/TCP envelope. |

---

## 3. Two-Tier Deployment Topology Architecture

```
[Host Workstation / Mobile Client]
     │
     ▼ (HTTP / WebSocket / C2C Protocol)
┌─────────────────────────────────────────────────────────────┐
│ CellOS Node (Physical PC / VMware / QEMU)                   │
│                                                             │
│  ┌───────────────────────┐                                  │
│  │ service-httpd Gateway │ (Port 8080: /api/c2c/dispatch)   │
│  └───────────┬───────────┘                                  │
│              │ C2C 112B Envelope Ingress (D12 / P08-A)      │
│              ▼                                              │
│  ┌───────────────────────┐     ┌─────────────────────────┐  │
│  │    Workload Router    │────►│    Budget Authority     │  │
│  │ (service-workload-rtr)│     │(service-budget-auth P08)│  │
│  └───────────┬───────────┘     └─────────────────────────┘  │
│              │ Destination Ready / Demand-based Autoscaling │
│              ▼                                              │
│  ┌───────────────────────────────────────────────────────┐  │
│  │             Haily Execution Cells (Tier 1)            │  │
│  │                                                       │  │
│  │   ┌─────────────────┐       ┌─────────────────────┐   │  │
│  │   │ Haily Core Cell │◄─────►│ AI Inference Cell   │   │  │
│  │   │ (vimio rt)      │       │ (service-ai Spec 24)│   │  │
│  │   └────────┬────────┘       └─────────────────────┘   │  │
│  │            │ CSpace Attenuated Capabilities           │  │
│  │            ▼                                          │  │
│  │   ┌─────────────────┐       ┌─────────────────────┐   │  │
│  │   │ Tool Sandboxes  │       │ VFS Storage         │   │  │
│  │   │ (Code/Execution)│       │ (service-vfs D01)   │   │  │
│  │   └─────────────────┘       └─────────────────────┘   │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Architectural Invariants:
1. **Zero Ambient Authority:** The Haily Agent on CellOS never inherits root/kernel privileges. Any file read or network egress is bounded by explicit CSpace capabilities (`CapSet`).
2. **Deterministic Task & Recovery Lifecycle:** Agent operations use `ostd::ipc::PendingCall` and `Completion` with monotonic epoch fencing (`P06` / `D04`). No duplicate task publication occurs upon node reboot or worker restart.
3. **Local-First Privacy:** Prompts and context embeddings are processed locally via `service-ai` GGUF engine whenever possible; network egress to cloud LLM providers requires explicit per-request authorization.

---

## 4. Stage H0 Exit Signoff & Next Steps

- [x] **Source Identity Pinned:** `/mnt/d/haily/` repository verified with full crate graph (`haily-types`, `haily-core`, `haily-llm`, `haily-io`, `haily-tools`, `haily-kms`, `haily-db`).
- [x] **Substrate Prerequisites Qualified:** Tier 1 Rust `std` PAL, `vimio` async reactor, CSpace capability system, and HTTP C2C Gateway are verified 100% operational on CellOS.
- [x] **Deployment Boundaries Defined:** Host Coordinator vs. Sandboxed CellOS Worker topology established.

**Status:** **STAGE H0 REVIEW COMPLETE & RATIFIED.**  
Ready to proceed to **Stage H1 (Single-Node Application Integration)** when user authorizes.
