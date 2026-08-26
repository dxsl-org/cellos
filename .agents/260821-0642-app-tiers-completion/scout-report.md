# Scout Report — App Tiers Completion

## Scope

Umbrella program for `.agents/TODO.md:5-71` items C2–C9. It coordinates independently approved child plans; it does not authorize implementation.

## Verified Current State

- Tier, profile, SDK module, layer, and G-stage are separate axes (`docs/specs/18-cell-trust-tiers.md:32-47`).
- One Native SDK family serves Tier 1 and future Tier 2 (`docs/specs/05-application.md:25-27`).
- Kernel ABI is frozen; additions require 2× explicit confirmation (`libs/api/src/abi.rs:2-12`).
- Manifest v2 is current; v1 is upcast (`libs/api/src/abi/manifest_flags.rs:10-14`, `libs/api/src/abi/manifest_parse.rs:32-76`).
- Tier 1 `rust-no-std` ships; `rust-std` is planned G4 (`docs/specs/18-cell-trust-tiers.md:55-63`).
- Production Tier 1 admission is incomplete (`docs/specs/18-cell-trust-tiers.md:89-121`).
- Tier 2 is unimplemented and gated by Spec 22 (`docs/specs/22-native-domain-cell-implementation-gate.md:3-31`).
- Tier 3 is lane-specific: ARM64 shipped, AMD MVP, Intel incomplete, RISC-V hardware-blocked (`docs/specs/05-application.md:361-409`).
- QEMU evidence cannot substitute for physical qualification (`docs/project-roadmap.md:67-73`).

## Verified Call Paths and Consumers

- Governed ELF: `kernel/src/loader.rs:115-192` → `task::spawn_from_mem`; memory spawn: `kernel/src/loader/mem_spawn_gate.rs:30-64` → `kernel/src/task.rs:1047`.
- Bootstrap exception: `kernel/src/main.rs:871-877`.
- Signature/policy: `kernel/src/signing.rs:21-64`, `kernel/src/policy.rs:85-170`.
- Manifest consumers: `libs/api/src/abi/manifest_parse.rs:9-76`, `libs/api/src/abi/manifest_tests.rs:1-35`, `kernel/src/loader/elf_tests.rs:331-493`.
- SDK anchors: `libs/ostd/src/app.rs:136`, `libs/ostd/src/clients.rs:28-29`, `libs/api/src/services.rs:21-25`.
- Hypervisor: `hal/traits/hypervisor/src/lib.rs:69`, `kernel/src/hypervisor/registry.rs:554-612`, `cells/services/hypervisor/src/vmm.rs:9-89`.

## Risk-First Conclusions

C2 precedes C8. C3, C5, and C8 may then parallelize. C6 stays default-off and separate. C7 splits into v2 tooling and v3 ABI; v3 waits for Tier 2 proof and 2× approval. C9 consumes only pinned evidence.

## Blockers

Physical x86/VF2/Pioneer need hardware; AArch64 test hooks have a semihosting blocker (`docs/project-roadmap.md:67-73`). RISC-V Tier 3 awaits H-extension (`docs/specs/05-application.md:390-395`). Owner consent and production anchors remain design-only (`docs/specs/18b-cell-admission-consent-adr.md:129-178`).

## Verification Classification

Full tier: Fact Checker, Flow Tracer, Scope Auditor, and Contract Verifier. Planned paths are `[UNVERIFIED]`.
