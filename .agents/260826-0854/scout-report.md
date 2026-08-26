# Scout Report

## Project Type

- Rust `no_std` microkernel workspace with Cell services/apps, HAL/board crates, host tools, QEMU/integration harnesses, and Cargo build tooling.

## Relevant Modules

- `cells/services/kms/`, `cells/services/silo/`, `cells/guests/silo-guest/`, `libs/types/src/kms*`, `libs/types/src/silo.rs` — active KMS/Silo session ownership; do not touch concurrently.
- `fuzz/src/elf_parser.rs` and `fuzz/Cargo.toml` — isolated P1 candidate: the fuzz target references `xmas_elf` without declaring it.
- `cells/apps/hypha/llm-gateway/src/http.rs` — isolated P1 candidate: explicitly P0-grade HTTP/JSON parsing needs malformed/escaping/duplicate-key tests and hardening.
- `tools/vi-compiler/src/codegen.rs` and `tools/vi-compiler/tests/codegen_tests.rs` — isolated P1 candidate: unknown properties are silently skipped and need diagnostics plus regression tests.
- `cells/demos/robot-demo/src/mqtt.rs` — isolated P2 candidate: packet-size and MQTT remaining-length boundaries lack focused tests.
- `hal/arch/x86/src/x86_64/idt.rs` — isolated P2 candidate: per-vector exception stubs can replace current vector/error-code inference.
- `kernel/src/task.rs` — useful but higher-conflict VFS work: canonicalization, `chdir`, and `rename` remain incomplete.
- `.agents/260825-1726-kms-silo-production-root/phase-06-select-production-root-product.md` — no-code product-selection gate explicitly allowed to run in parallel with KMS software phases.

## Patterns & Conventions

- **Architecture:** mixed microkernel + layered + event-driven — kernel/HAL/boards own privileged mechanisms, typed `libs/types` contracts cross Cell boundaries, services own policy/state, and apps communicate through IPC.
- Public cross-Cell wire contracts live in shared type crates; concurrent work should avoid changing shared enums or ABI ordering.
- Tests are colocated in Rust modules or under `tests/integration`; hardware qualification distinguishes compile/QEMU evidence from physical-board evidence.
- Worktree code changes are confined to the KMS/Silo ownership set observed on 2026-08-26, so unrelated slices should keep file ownership disjoint.

## Docs & In-Flight Plans

- `.agents/260825-1726-kms-silo-production-root/plan.md` — Phases 1-2 recorded complete; Phases 3-8 pending; Phase 6 may run in parallel and Phase 3 requires explicit approval.
- `.agents/260825-sdk-delivery/plan.md` — Phase 06 is partial, but its remaining relay/client closure depends on the KMS/Silo identity lifecycle.
- `.agents/260823-rpi3-hardware-completion/plan.md` — SD, sensor, and HDMI lanes are recorded in progress and are independent of KMS, but physical evidence is required.
- `docs/roadmap/open-risk-register.md` — contains stale claims for net socket owner binding and Lua/WASM syscall allowlists; current code already includes `SocketOwner`, `LookupService`, and `StateRestore`.

## APIs / Schemas / Contracts

- `cells/services/net/src/socket_table.rs` — `SocketOwner { cell_id, generation }` now owner-binds socket lookup/removal; risk-register text must be verified before any new fix is planned.
- `cells/runtimes/lua/src/main.rs` and `cells/tools/wasm/src/main.rs` — manifests already declare `LookupService` and `StateRestore`.
- `fuzz/src/elf_parser.rs` — host fuzz entry expects the `xmas_elf::ElfFile` parser contract.
- `tools/vi-compiler/src/codegen.rs` — generated ViUI constructors and property diagnostics form the public developer-tooling behavior.

## Unresolved Questions

- Confirm whether the KMS session will immediately advance into Phase 3/4; if yes, reserve `cells/services/net`, `cells/services/net-broker`, `libs/ostd/src/clients`, and `tools/relay-*` too.
- Confirm available physical RPi3/VF2/Pioneer/x86 hardware before selecting a hardware-evidence lane.
- The fuzz dependency mismatch should be reproduced with the repo's intended nightly/fuzz command before implementation.
